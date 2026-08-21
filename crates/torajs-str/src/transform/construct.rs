//! Str builders — `s.repeat(n)` · `s.charAt(i)` · `s.at(i)` ·
//! `String.fromCharCode(n)` · `String.fromCodePoint(n)` ·
//! `s.substring(start, end)` · `s.substr(start, length)`.
//!
//! Six per-op fns; all produce a fresh string-like heap value.
//! Two distinct heap return shapes:
//!
//! - **Str return** (`repeat` / `at` / `fromCharCode` / `substring`
//!   / `substr`): single pool-aware `StrBlock::alloc` + payload
//!   write.
//! - **Substr return** (`charAt`): zero-copy 1-byte view into the
//!   parent Str via [`crate::substr::__torajs_substr_create`].
//!   `charAt` is the legacy method shape; `at` is ES2022 and
//!   returns an independent Str instead.
//!
//! Spec corner notes (all preserved bit-for-bit from the C
//! original):
//! - **`repeat`** `n < 0` clamps to 0 (no RangeError yet — v0
//!   subset; spec would throw).
//! - **`substring`** swaps `start > end` and clamps both to `[0,
//!   len]`. Negative inputs clamp to 0 (does NOT wrap like
//!   `slice`).
//! - **`substr`** (annexB legacy) wraps negative `start` to `max
//!   (size + start, 0)`; `length` clamps to remaining.
//! - **`charAt`** OOB returns an empty 0-len value per §22.1.3.2;
//!   **`at`** OOB answers the immortal undefined sentinel per
//!   §22.1.3.1 step 6 (RFC 20260707); **`s[i]`** index reads route
//!   through [`__torajs_str_index_view`] and answer the
//!   Substr-shaped sentinel on OOB (ES §10.4.3 [[Get]]).
//! - **`fromCharCode`** truncates `n` to 1 code unit
//!   (`n & 0xFFFF`) per spec §22.1.2.1; picks Latin-1 / UTF-16 LE
//!   encoding canonically.
//! - **`fromCodePoint`** accepts a full codepoint in
//!   `[0, 0x10FFFF]` per spec §22.1.2.2; values above 0xFFFF
//!   surrogate-pair encode to 2 UTF-16 LE code units. Out-of-range
//!   inputs raise a catchable `RangeError` via
//!   `__torajs_throw_range_error` — the SSA-side `emit_throw_check`
//!   after the call propagates.
//!
//! IR-side surface (declared in `ssa_lower::lower`, intrinsic
//! noalias-whitelisted on the LLVM-era backend):
//! `__torajs_str_repeat` · `_char_at` · `_at` · `_from_char_code`
//! · `_from_code_point` · `_substring` · `_substr`.

use core::ffi::c_void;

use crate::block::StrBlock;
use crate::layout::{STR_DATA_OFF, STR_FLAG_IS_LATIN1, STR_LEN_OFF};
use crate::substr::__torajs_substr_create;
use torajs_rc::HeapHeader;

unsafe extern "C" {
    /// `torajs-throw`'s cross-TU `RangeError` raise — records a
    /// pending throw via TLS and returns normally; the SSA-emitted
    /// `emit_throw_check` at the call site propagates to user
    /// `try/catch`. Used by `__torajs_str_from_code_point` for
    /// invalid codepoints (ES §22.1.2.2).
    fn __torajs_throw_range_error(msg: *const u8);
}

// ============================================================
// Layout-aware FFI helpers (sub-module-local)
// ============================================================

#[inline]
unsafe fn str_view<'a>(p: *const u8) -> (&'a [u8], u32, bool) {
    let length = unsafe { (p.add(STR_LEN_OFF) as *const u32).read() };
    let header = unsafe { &*(p as *const HeapHeader) };
    let is_latin1 = (header.flags & STR_FLAG_IS_LATIN1) != 0;
    let byte_cnt = if is_latin1 {
        length as usize
    } else {
        (length as usize) * 2
    };
    let payload = unsafe { core::slice::from_raw_parts(p.add(STR_DATA_OFF), byte_cnt) };
    (payload, length, is_latin1)
}

/// Allocate a fresh Str with the same encoding as the source view
/// and copy the requested code-unit range. `byte_lo` and
/// `byte_hi` are byte offsets into the source's payload (already
/// translated via code_unit × stride at the call site).
#[inline]
fn alloc_str_slice(payload: &[u8], byte_lo: usize, byte_hi: usize, is_latin1: bool) -> *mut u8 {
    // Range carving can drop every supra-Latin-1 unit — narrow per
    // the canonical-encoding invariant (chunk A2).
    crate::alloc_canonical::alloc_units_canonical(&payload[byte_lo..byte_hi], is_latin1)
}

#[inline]
fn alloc_empty_str() -> *mut u8 {
    StrBlock::alloc(0).into_raw()
}

// ============================================================
// Pure-Rust cores
// ============================================================

/// Total bytes for `s.repeat(n)`. Returns 0 if `n <= 0`. Uses
/// `wrapping_mul` to match the C subset's silent-overflow contract
/// (the v0 caller is expected to pass sane `n`).
#[inline]
pub fn repeat_out_len(s_len: u32, n: i64) -> u32 {
    if n <= 0 {
        0
    } else {
        s_len.wrapping_mul(n as u32)
    }
}

/// `s.substring(start, end)` index normalization. Clamps both to
/// `[0, len]`, swaps if `start > end`. Returns `(lo, hi)` such
/// that `lo <= hi <= len`.
#[inline]
pub fn substring_range(start: i64, end: i64, len: u32) -> (u32, u32) {
    let ilen = len as i64;
    let mut s = clamp_to_range(start, ilen);
    let mut e = clamp_to_range(end, ilen);
    if s > e {
        core::mem::swap(&mut s, &mut e);
    }
    (s as u32, e as u32)
}

#[inline]
fn clamp_to_range(v: i64, max: i64) -> i64 {
    if v < 0 {
        0
    } else if v > max {
        max
    } else {
        v
    }
}

/// `s.substr(start, length)` (annexB legacy) range. Wraps negative
/// `start`, clamps `length` to remaining. Returns `(lo, len)`
/// where `lo + len <= size`.
#[inline]
pub fn substr_range(start: i64, length: i64, size: u32) -> (u32, u32) {
    let isize = size as i64;
    let mut s = if start < 0 { isize + start } else { start };
    if s < 0 {
        s = 0;
    }
    if s > isize {
        s = isize;
    }
    let avail = isize - s;
    let mut len = if length > avail { avail } else { length };
    if len < 0 {
        len = 0;
    }
    (s as u32, len as u32)
}

/// `s.at(i)` index resolution. Negative `i` wraps to `len + i`;
/// returns `None` if out of bounds.
#[inline]
pub fn at_resolve(i: i64, len: u32) -> Option<u32> {
    let ilen = len as i64;
    let adj = if i < 0 { ilen + i } else { i };
    if adj < 0 || adj >= ilen {
        None
    } else {
        Some(adj as u32)
    }
}

// ============================================================
// extern "C" wrappers
// ============================================================

/// `s.repeat(n)` — fresh Str holding `s` concatenated `n` times.
/// `n == 0` yields the empty Str.
///
/// Per ES §22.1.3.16 steps 3-4, the spec throws `RangeError` when
/// `n < 0` or when `n` is `+Infinity`. The SSA arm lowers the
/// argument via `FpToSi` so floating-point Infinity reaches us as
/// `i64::MAX` (`fcvtzs +Inf` on aarch64 / `cvttsd2si` on x86_64);
/// we treat that as the Infinity sentinel and throw the same error
/// as the negative path. NaN coerces to `0` via the same
/// FpToSi rule, matching V8/JSC's "ToInteger(NaN) = 0 → empty
/// string" observable behavior.
///
/// `n == i64::MAX` from a legitimate user literal (extremely
/// unlikely — the byte-cost would overflow `u32` anyway) gets
/// routed to the throw path; bun / V8 also raise on such inputs
/// because the resulting string can't be allocated.
///
/// The message matches bun's so conformance diff-tests can rely
/// on `(e as Error).message`.
///
/// # Safety
///
/// `s` must be a valid Str heap block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_repeat(s: *const u8, n: i64) -> *mut u8 {
    if n < 0 || n == i64::MAX {
        unsafe {
            __torajs_throw_range_error(
                b"String.prototype.repeat argument must be greater than or equal to 0 and not be Infinity\0"
                    .as_ptr(),
            )
        };
        // Caller's emit_throw_check picks up the TLS pending throw
        // immediately on return. The empty Str result is unreachable
        // in user code but keeps the SSA Call result type stable.
        let block = StrBlock::alloc_with_encoding(0, true);
        return block.into_raw();
    }
    let (s_payload, s_length, is_latin1) = unsafe { str_view(s) };
    let out_length = repeat_out_len(s_length, n);
    let mut block = StrBlock::alloc_with_encoding(out_length, is_latin1);
    if out_length == 0 || s_length == 0 {
        return block.into_raw();
    }
    let s_byte_cnt = s_payload.len();
    let out_byte_cnt = if is_latin1 {
        out_length as usize
    } else {
        (out_length as usize) * 2
    };
    let dst = unsafe { block.as_bytes_mut(out_byte_cnt as u32) };
    let times = (out_length / s_length) as usize;
    for k in 0..times {
        dst[k * s_byte_cnt..(k + 1) * s_byte_cnt].copy_from_slice(s_payload);
    }
    block.into_raw()
}

/// `s.charAt(i)` — returns a **Substr** (1-byte zero-copy view
/// into the parent Str) for in-range `i`, or a length-0 Substr
/// for OOB. Negative `i` is NOT wrapped (use `at` for wrap
/// semantics).
///
/// # Safety
///
/// `s` may be null (a NULL parent yields an empty Substr); when
/// non-null it must be a valid Str heap block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_char_at(s: *mut u8, i: i64) -> *mut u8 {
    let parent = s as *mut c_void;
    if s.is_null() {
        return unsafe { __torajs_substr_create(parent, 0, 0) } as *mut u8;
    }
    let (_, length, _is_latin1) = unsafe { str_view(s) };
    if i < 0 || (i as u64) >= length as u64 {
        return unsafe { __torajs_substr_create(parent, 0, 0) } as *mut u8;
    }
    // Substr `(offset, len)` are JS code units post-P11.1-S5; the
    // view spans exactly one code unit at code-unit index `i`.
    unsafe { __torajs_substr_create(parent, i as u64, 1) as *mut u8 }
}

/// `s[i]` — string INDEX read (ES §10.4.3 [[Get]]). Unlike charAt
/// (§22.1.3.2, which answers "" — see [`__torajs_str_char_at`]),
/// an out-of-range index answers JS `undefined`: the immortal
/// Substr-shaped sentinel (RFC 20260707 residual chunk). A nullish
/// receiver (NULL / the Str sentinel) also answers the sentinel —
/// nullish-propagating and deref-safe; the spec TypeError guard
/// face is ledgered separately.
///
/// # Safety
///
/// `s` may be null; when non-null it must be a valid Str heap
/// block or the Str sentinel.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_index_view(s: *mut u8, i: i64) -> *mut u8 {
    if s.is_null() || crate::undef_sentinel::is_undef(s) {
        return crate::undef_sentinel::substr_undef_ptr();
    }
    let (_, length, _is_latin1) = unsafe { str_view(s) };
    if i < 0 || (i as u64) >= length as u64 {
        return crate::undef_sentinel::substr_undef_ptr();
    }
    unsafe { __torajs_substr_create(s as *mut c_void, i as u64, 1) as *mut u8 }
}

/// `s.at(i)` — ES2022 single-char Str. Negative `i` wraps; OOB
/// answers JS `undefined` per §22.1.3.1 step 6 — the immortal Str
/// sentinel (RFC 20260707; pre-fix answered "" which bun prints as
/// an empty line).
///
/// # Safety
///
/// `s` must be a valid Str heap block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_at(s: *const u8, i: i64) -> *mut u8 {
    let (payload, length, is_latin1) = unsafe { str_view(s) };
    match at_resolve(i, length) {
        None => crate::undef_sentinel::undef_ptr(),
        Some(idx) => {
            let stride = if is_latin1 { 1usize } else { 2usize };
            let byte_off = (idx as usize) * stride;
            alloc_str_slice(payload, byte_off, byte_off + stride, is_latin1)
        }
    }
}

/// `String.fromCharCode(n)` — 1-code-unit Str holding `n & 0xFFFF`
/// per ES spec §22.1.2.1 (truncate to 16-bit unsigned). Codepoints
/// ≤ 0xFF pick the Latin-1 encoding; everything else (including
/// surrogate halves) becomes a UTF-16 LE Str.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_from_char_code(n: i64) -> *mut u8 {
    let cu = (n & 0xFFFF) as u16;
    if cu <= 0xFF {
        let mut block = StrBlock::alloc_with_encoding(1, true);
        unsafe { block.as_bytes_mut(1) }.copy_from_slice(&[cu as u8]);
        block.into_raw()
    } else {
        let mut block = StrBlock::alloc_with_encoding(1, false);
        unsafe { block.as_bytes_mut(2) }.copy_from_slice(&cu.to_le_bytes());
        block.into_raw()
    }
}

/// `String.fromCodePoint(n)` per ES spec §22.1.2.2. Accepts integer
/// codepoints in `[0, 0x10FFFF]`; anything outside that range (or
/// non-integer — caller path is typed `i64`, so fractional inputs
/// can't reach us, but negative values can) raises a catchable
/// `RangeError` via the cross-TU `__torajs_throw_range_error`
/// helper. The SSA-side `emit_throw_check` after our call propagates.
///
/// Encoding picks:
/// - BMP, `n ≤ 0xFF` → Latin-1 1 code unit
/// - BMP, `0x100 ≤ n ≤ 0xFFFF` → UTF-16 LE 1 code unit (includes
///   isolated surrogate halves — spec allows them since `fromCodePoint`
///   does not enforce well-formed UTF-16 distinction here)
/// - Supplementary, `0x10000 ≤ n ≤ 0x10FFFF` → UTF-16 LE 2 code
///   units (surrogate pair):
///   - `high = 0xD800 + ((n - 0x10000) >> 10)`
///   - `low  = 0xDC00 + ((n - 0x10000) & 0x3FF)`
///
/// On invalid input, returns an empty string sentinel — caller's
/// `emit_throw_check` makes the value unreachable in well-formed
/// programs, so the actual payload doesn't matter, only that the
/// return is type-correct (`*mut u8`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_from_code_point(n: i64) -> *mut u8 {
    if !(0..=0x10FFFF).contains(&n) {
        unsafe {
            __torajs_throw_range_error(b"Invalid code point\0".as_ptr());
        }
        return alloc_empty_str();
    }
    if n <= 0xFF {
        let mut block = StrBlock::alloc_with_encoding(1, true);
        unsafe { block.as_bytes_mut(1) }.copy_from_slice(&[n as u8]);
        block.into_raw()
    } else if n <= 0xFFFF {
        let mut block = StrBlock::alloc_with_encoding(1, false);
        unsafe { block.as_bytes_mut(2) }.copy_from_slice(&(n as u16).to_le_bytes());
        block.into_raw()
    } else {
        // Supplementary plane — encode as surrogate pair, 2 UTF-16 LE
        // code units. SpiderMonkey / V8 / JSC textbook formula.
        let offset = (n - 0x10000) as u32;
        let high: u16 = 0xD800 + ((offset >> 10) as u16);
        let low: u16 = 0xDC00 + ((offset & 0x3FF) as u16);
        let mut block = StrBlock::alloc_with_encoding(2, false);
        let dst = unsafe { block.as_bytes_mut(4) };
        dst[0..2].copy_from_slice(&high.to_le_bytes());
        dst[2..4].copy_from_slice(&low.to_le_bytes());
        block.into_raw()
    }
}

/// ES §22.1.2.1 step 2 for `String.fromCharCode`, over the Number the
/// spec actually hands this step: `ToUint16`.
///
/// `ToUint16` (§7.1.7) maps NaN, ±0 and ±∞ to +0, and everything else
/// to `truncate(n) mod 2^16`. An i64 parameter cannot express that:
/// the caller's ToInteger fold turns ±∞ into `i64::MAX`, whose low 16
/// bits are `0xFFFF`, so `String.fromCharCode(Infinity)` used to be
/// U+FFFF instead of U+0000. Doing the coercion where the Number is
/// still a Number removes the whole class.
///
/// # Safety
///
/// Trivially safe; `unsafe extern "C"` only for the FFI boundary.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_from_char_code_f64(n: f64) -> *mut u8 {
    // `% 65536.0` on an already-truncated finite f64 is exact (both
    // operands are integral and well inside the 2^53 range), so this
    // never rounds the way an i64 cast of a huge value would.
    let u16_val = if !n.is_finite() || n == 0.0 {
        0i64
    } else {
        let m = n.trunc() % 65536.0;
        let m = if m < 0.0 { m + 65536.0 } else { m };
        m as i64
    };
    unsafe { __torajs_str_from_char_code(u16_val) }
}

/// ES §22.1.2.2 step 2 for `String.fromCodePoint`, over the Number
/// the spec actually hands this step.
///
/// Step 2.a is `ToNumber(next)` and step 2.b rejects a *non-integral
/// Number* — so `fromCodePoint(3.14)` and `fromCodePoint('3.14')` are
/// both a `RangeError`, and only an f64 ABI can tell 3.14 from 3. The
/// i64 sibling below cannot: whoever truncates on the way in has
/// already thrown away the fact the check is about. Callers that used
/// to reconstruct it by passing `-1` as a poison value now just call
/// this.
///
/// NaN is rejected by `trunc() != n` (which NaN fails against itself)
/// and ±∞ by `is_finite`, both before the cast — so the `as i64`
/// below only ever sees a value already inside the code-point range.
///
/// # Safety
///
/// Trivially safe; `unsafe extern "C"` only for the FFI boundary.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_from_code_point_f64(n: f64) -> *mut u8 {
    if !n.is_finite() || n.trunc() != n || !(0.0..=1114111.0).contains(&n) {
        unsafe {
            __torajs_throw_range_error(b"Invalid code point\0".as_ptr());
        }
        return alloc_empty_str();
    }
    unsafe { __torajs_str_from_code_point(n as i64) }
}

/// `s.substring(start, end)` — slice's pre-ES5 sibling. Negative
/// inputs clamp to 0 (not wrap), and `start > end` is silently
/// swapped before slicing.
///
/// # Safety
///
/// `s` must be a valid Str heap block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_substring(s: *const u8, start: i64, end: i64) -> *mut u8 {
    let (payload, length, is_latin1) = unsafe { str_view(s) };
    let (lo, hi) = substring_range(start, end, length);
    let stride = if is_latin1 { 1usize } else { 2usize };
    alloc_str_slice(
        payload,
        (lo as usize) * stride,
        (hi as usize) * stride,
        is_latin1,
    )
}

/// `s.substr(start, length)` — AnnexB legacy. Negative `start`
/// wraps; `length` clamps to remaining bytes.
///
/// # Safety
///
/// `s` must be a valid Str heap block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_substr(s: *const u8, start: i64, length: i64) -> *mut u8 {
    let (payload, size, is_latin1) = unsafe { str_view(s) };
    let (lo, out_len) = substr_range(start, length, size);
    let stride = if is_latin1 { 1usize } else { 2usize };
    let byte_lo = (lo as usize) * stride;
    let byte_hi = ((lo + out_len) as usize) * stride;
    alloc_str_slice(payload, byte_lo, byte_hi, is_latin1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    #[test]
    fn repeat_out_len_basic() {
        assert_eq!(repeat_out_len(3, 4), 12);
        assert_eq!(repeat_out_len(5, 0), 0);
        assert_eq!(repeat_out_len(5, -3), 0);
        assert_eq!(repeat_out_len(0, 100), 0);
    }

    #[test]
    fn substring_range_basics() {
        assert_eq!(substring_range(0, 5, 10), (0, 5));
        assert_eq!(substring_range(-3, 7, 10), (0, 7)); // negative clamps
        assert_eq!(substring_range(7, 3, 10), (3, 7)); // start > end swap
        assert_eq!(substring_range(100, 200, 10), (10, 10)); // both past len
        assert_eq!(substring_range(5, 5, 10), (5, 5)); // empty slice
    }

    #[test]
    fn substr_range_basics() {
        assert_eq!(substr_range(0, 3, 10), (0, 3));
        assert_eq!(substr_range(-2, 5, 10), (8, 2)); // negative wrap, length clamped
        assert_eq!(substr_range(-100, 3, 10), (0, 3)); // wrap saturates to 0
        assert_eq!(substr_range(20, 5, 10), (10, 0)); // start past len
        assert_eq!(substr_range(3, -2, 10), (3, 0)); // negative length
        assert_eq!(substr_range(2, i64::MAX, 10), (2, 8)); // length clamps to remaining
    }

    #[test]
    fn at_resolve_basics() {
        assert_eq!(at_resolve(0, 5), Some(0));
        assert_eq!(at_resolve(4, 5), Some(4));
        assert_eq!(at_resolve(-1, 5), Some(4));
        assert_eq!(at_resolve(-5, 5), Some(0));
        assert_eq!(at_resolve(5, 5), None);
        assert_eq!(at_resolve(-6, 5), None);
        assert_eq!(at_resolve(0, 0), None);
    }

    // ============================================================
    // FFI round-trip tests
    // ============================================================

    use crate::block::__torajs_str_free;

    fn make_str(payload: &[u8]) -> *mut u8 {
        let mut b = StrBlock::alloc(payload.len() as u32);
        let dst = unsafe { b.as_bytes_mut(payload.len() as u32) };
        dst.copy_from_slice(payload);
        b.into_raw()
    }

    fn read_str(p: *const u8) -> Vec<u8> {
        let (payload, _, _) = unsafe { str_view(p) };
        payload.to_vec()
    }

    #[test]
    fn ffi_repeat_basic() {
        let s = make_str(b"ab");
        let r = unsafe { __torajs_str_repeat(s, 3) };
        assert_eq!(read_str(r), b"ababab");
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(r) };
    }

    #[test]
    fn ffi_repeat_zero_and_negative() {
        // ES §22.1.3.16 — `n == 0` yields empty Str; `n < 0` raises
        // a RangeError. In production (tr build) the runtime calls
        // `__torajs_throw_range_error` which sets a TLS pending throw
        // for the caller's emit_throw_check. In cargo test we use the
        // lib.rs stub which flips `STUB_THROW_RAISED` instead — same
        // observable effect for the unit-test invariant.
        use core::sync::atomic::Ordering;
        crate::STUB_THROW_RAISED.store(false, Ordering::SeqCst);
        let s = make_str(b"hi");
        let r0 = unsafe { __torajs_str_repeat(s, 0) };
        assert_eq!(read_str(r0), b"");
        assert!(
            !crate::STUB_THROW_RAISED.load(Ordering::SeqCst),
            "n=0 must not throw"
        );
        let rn = unsafe { __torajs_str_repeat(s, -5) };
        assert_eq!(read_str(rn), b"");
        assert!(
            crate::STUB_THROW_RAISED.load(Ordering::SeqCst),
            "n<0 must raise a RangeError"
        );
        crate::STUB_THROW_RAISED.store(false, Ordering::SeqCst);
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(r0) };
        unsafe { __torajs_str_free(rn) };
    }

    #[test]
    fn ffi_from_char_code_truncates_to_codeunit() {
        // P11.1-S2.5 — `fromCharCode` now follows spec §22.1.2.1
        // (`n & 0xFFFF`) and the result encoding is picked
        // canonically: codepoints ≤ 0xFF → Latin-1; otherwise
        // UTF-16 LE single code unit.
        let r = unsafe { __torajs_str_from_char_code(65) };
        let r2 = unsafe { __torajs_str_from_char_code(0x141) }; // U+0141 (Ł)
        assert_eq!(read_str(r), b"A");
        // U+0141 little-endian → [0x41, 0x01].
        assert_eq!(read_str(r2), [0x41, 0x01]);
        unsafe { __torajs_str_free(r) };
        unsafe { __torajs_str_free(r2) };
    }

    #[test]
    fn ffi_from_code_point_bmp_latin1() {
        // BMP ≤ 0xFF → Latin-1 1 code unit (byte-identical to ASCII for `A`).
        let r = unsafe { __torajs_str_from_code_point(65) };
        assert_eq!(read_str(r), b"A");
        unsafe { __torajs_str_free(r) };
    }

    #[test]
    fn ffi_from_code_point_bmp_utf16() {
        // BMP 0x100-0xFFFF → UTF-16 LE 1 code unit. U+4E2D ("中") LE = [0x2D, 0x4E].
        let r = unsafe { __torajs_str_from_code_point(0x4E2D) };
        assert_eq!(read_str(r), [0x2D, 0x4E]);
        unsafe { __torajs_str_free(r) };
    }

    #[test]
    fn ffi_from_code_point_supplementary_surrogate_pair() {
        // U+1F600 (😀). offset = 0xF600. high = 0xD800 + 0x3D = 0xD83D.
        // low = 0xDC00 + 0x200 = 0xDE00. UTF-16 LE bytes = [3D D8 00 DE].
        let r = unsafe { __torajs_str_from_code_point(0x1F600) };
        assert_eq!(read_str(r), [0x3D, 0xD8, 0x00, 0xDE]);
        unsafe { __torajs_str_free(r) };
    }

    #[test]
    fn ffi_from_code_point_max_supplementary() {
        // U+10FFFF — boundary. offset = 0xFFFFF.
        // high = 0xD800 + (0xFFFFF >> 10) = 0xD800 + 0x3FF = 0xDBFF
        // low  = 0xDC00 + (0xFFFFF & 0x3FF) = 0xDFFF
        let r = unsafe { __torajs_str_from_code_point(0x10FFFF) };
        assert_eq!(read_str(r), [0xFF, 0xDB, 0xFF, 0xDF]);
        unsafe { __torajs_str_free(r) };
    }

    #[test]
    fn ffi_substring_basic_and_swap() {
        let s = make_str(b"hello");
        let r = unsafe { __torajs_str_substring(s, 1, 4) };
        let rs = unsafe { __torajs_str_substring(s, 4, 1) }; // swap → same as 1..4
        assert_eq!(read_str(r), b"ell");
        assert_eq!(read_str(rs), b"ell");
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(r) };
        unsafe { __torajs_str_free(rs) };
    }

    #[test]
    fn ffi_substring_negative_clamps_to_zero() {
        let s = make_str(b"hello");
        let r = unsafe { __torajs_str_substring(s, -3, 3) };
        assert_eq!(read_str(r), b"hel");
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(r) };
    }

    #[test]
    fn ffi_substr_basic() {
        let s = make_str(b"hello");
        let r = unsafe { __torajs_str_substr(s, 1, 3) };
        assert_eq!(read_str(r), b"ell");
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(r) };
    }

    #[test]
    fn ffi_substr_negative_wraps() {
        let s = make_str(b"hello");
        let r = unsafe { __torajs_str_substr(s, -3, 2) };
        assert_eq!(read_str(r), b"ll");
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(r) };
    }

    #[test]
    fn ffi_at_positive_and_negative() {
        let s = make_str(b"hello");
        let r0 = unsafe { __torajs_str_at(s, 0) };
        let rn = unsafe { __torajs_str_at(s, -1) };
        // OOB answers the immortal undefined sentinel per §22.1.3.1
        // step 6 (RFC 20260707) — identity, never freed.
        let roob = unsafe { __torajs_str_at(s, 100) };
        assert_eq!(read_str(r0), b"h");
        assert_eq!(read_str(rn), b"o");
        assert!(crate::undef_sentinel::is_undef(roob));
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(r0) };
        unsafe { __torajs_str_free(rn) };
    }

    #[test]
    fn ffi_str_index_view_in_range_and_oob() {
        let s = make_str(b"ab");
        let hit = unsafe { __torajs_str_index_view(s, 1) };
        let (payload, len, _) = unsafe { crate::substr_methods::substr_view(hit as *const u8) };
        assert_eq!(len, 1);
        assert_eq!(payload, b"b");
        unsafe { crate::substr::__torajs_substr_drop(hit as *mut core::ffi::c_void) };
        // OOB / negative / nullish receiver → the Substr sentinel.
        let oob = unsafe { __torajs_str_index_view(s, 5) };
        assert!(crate::undef_sentinel::is_substr_undef(oob));
        let neg = unsafe { __torajs_str_index_view(s, -1) };
        assert!(crate::undef_sentinel::is_substr_undef(neg));
        let on_null = unsafe { __torajs_str_index_view(core::ptr::null_mut(), 0) };
        assert!(crate::undef_sentinel::is_substr_undef(on_null));
        let on_undef = unsafe { __torajs_str_index_view(crate::undef_sentinel::undef_ptr(), 0) };
        assert!(crate::undef_sentinel::is_substr_undef(on_undef));
        unsafe { __torajs_str_free(s) };
    }
}
