//! Str builders — `s.repeat(n)` · `s.charAt(i)` · `s.at(i)` ·
//! `String.fromCharCode(n)` · `s.substring(start, end)` ·
//! `s.substr(start, length)`.
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
//! - **`at` / `charAt`** OOB returns an empty 0-len value (not
//!   undefined — would require `Nullable<string>`).
//! - **`fromCharCode`** truncates `n` to 1 byte (`n & 0xff`).
//!   Non-ASCII code points need UTF-8 encoding which v0 doesn't
//!   model.
//!
//! IR-side surface (declared in `ssa_lower::lower`, intrinsic
//! noalias-whitelisted in `ssa_inkwell::is_alloc_intrinsic`):
//! `__torajs_str_repeat` · `_char_at` · `_at` · `_from_char_code`
//! · `_substring` · `_substr`.

use std::ffi::c_void;

use crate::alloc::StrBlock;
use crate::layout::{STR_DATA_OFF, STR_LEN_OFF};
use crate::substr::__torajs_substr_create;

// ============================================================
// Layout-aware FFI helpers (sub-module-local)
// ============================================================

#[inline]
unsafe fn str_len(p: *const u8) -> u64 {
    unsafe { (p.add(STR_LEN_OFF) as *const u64).read() }
}

#[inline]
unsafe fn str_bytes<'a>(p: *const u8, len: u64) -> &'a [u8] {
    unsafe { core::slice::from_raw_parts(p.add(STR_DATA_OFF), len as usize) }
}

#[inline]
fn alloc_str(payload: &[u8]) -> *mut u8 {
    let out_len = payload.len() as u64;
    let mut block = StrBlock::alloc(out_len);
    if !payload.is_empty() {
        let dst = unsafe { block.as_bytes_mut(out_len) };
        dst.copy_from_slice(payload);
    }
    block.into_raw()
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
pub fn repeat_out_len(s_len: u64, n: i64) -> u64 {
    if n <= 0 {
        0
    } else {
        s_len.wrapping_mul(n as u64)
    }
}

/// `s.substring(start, end)` index normalization. Clamps both to
/// `[0, len]`, swaps if `start > end`. Returns `(lo, hi)` such
/// that `lo <= hi <= len`.
#[inline]
pub fn substring_range(start: i64, end: i64, len: u64) -> (u64, u64) {
    let ilen = len as i64;
    let mut s = clamp_to_range(start, ilen);
    let mut e = clamp_to_range(end, ilen);
    if s > e {
        core::mem::swap(&mut s, &mut e);
    }
    (s as u64, e as u64)
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
pub fn substr_range(start: i64, length: i64, size: u64) -> (u64, u64) {
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
    (s as u64, len as u64)
}

/// `s.at(i)` index resolution. Negative `i` wraps to `len + i`;
/// returns `None` if out of bounds.
#[inline]
pub fn at_resolve(i: i64, len: u64) -> Option<u64> {
    let ilen = len as i64;
    let adj = if i < 0 { ilen + i } else { i };
    if adj < 0 || adj >= ilen {
        None
    } else {
        Some(adj as u64)
    }
}

// ============================================================
// extern "C" wrappers
// ============================================================

/// `s.repeat(n)` — fresh Str holding `s` concatenated `n` times.
/// `n <= 0` yields the empty Str.
///
/// # Safety
///
/// `s` must be a valid Str heap block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_repeat(s: *const u8, n: i64) -> *mut u8 {
    let s_len = unsafe { str_len(s) };
    let out_len = repeat_out_len(s_len, n);
    let mut block = StrBlock::alloc(out_len);
    if out_len == 0 || s_len == 0 {
        return block.into_raw();
    }
    let s_payload = unsafe { str_bytes(s, s_len) };
    let dst = unsafe { block.as_bytes_mut(out_len) };
    let s_used = s_len as usize;
    let times = (out_len / s_len) as usize;
    for k in 0..times {
        dst[k * s_used..(k + 1) * s_used].copy_from_slice(s_payload);
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
    let len = unsafe { str_len(s) };
    if i < 0 || (i as u64) >= len {
        return unsafe { __torajs_substr_create(parent, 0, 0) } as *mut u8;
    }
    unsafe { __torajs_substr_create(parent, i as u64, 1) as *mut u8 }
}

/// `s.at(i)` — ES2022 single-char Str. Negative `i` wraps;
/// OOB returns the empty Str.
///
/// # Safety
///
/// `s` must be a valid Str heap block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_at(s: *const u8, i: i64) -> *mut u8 {
    let len = unsafe { str_len(s) };
    match at_resolve(i, len) {
        None => alloc_empty_str(),
        Some(idx) => {
            let byte = unsafe { str_bytes(s, len) }[idx as usize];
            alloc_str(&[byte])
        }
    }
}

/// `String.fromCharCode(n)` — 1-byte Str holding `n & 0xff`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_from_char_code(n: i64) -> *mut u8 {
    let byte = (n & 0xff) as u8;
    alloc_str(&[byte])
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
    let len = unsafe { str_len(s) };
    let (lo, hi) = substring_range(start, end, len);
    let payload = unsafe { str_bytes(s, len) };
    alloc_str(&payload[lo as usize..hi as usize])
}

/// `s.substr(start, length)` — AnnexB legacy. Negative `start`
/// wraps; `length` clamps to remaining bytes.
///
/// # Safety
///
/// `s` must be a valid Str heap block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_substr(s: *const u8, start: i64, length: i64) -> *mut u8 {
    let size = unsafe { str_len(s) };
    let (lo, out_len) = substr_range(start, length, size);
    let payload = unsafe { str_bytes(s, size) };
    alloc_str(&payload[lo as usize..(lo + out_len) as usize])
}

#[cfg(test)]
mod tests {
    use super::*;

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

    use crate::alloc::__torajs_str_free;

    fn make_str(payload: &[u8]) -> *mut u8 {
        let mut b = StrBlock::alloc(payload.len() as u64);
        let dst = unsafe { b.as_bytes_mut(payload.len() as u64) };
        dst.copy_from_slice(payload);
        b.into_raw()
    }

    fn read_str(p: *const u8) -> Vec<u8> {
        let len = unsafe { str_len(p) };
        unsafe { str_bytes(p, len) }.to_vec()
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
        let s = make_str(b"hi");
        let r0 = unsafe { __torajs_str_repeat(s, 0) };
        let rn = unsafe { __torajs_str_repeat(s, -5) };
        assert_eq!(read_str(r0), b"");
        assert_eq!(read_str(rn), b"");
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(r0) };
        unsafe { __torajs_str_free(rn) };
    }

    #[test]
    fn ffi_from_char_code_truncates_to_byte() {
        let r = unsafe { __torajs_str_from_char_code(65) };
        let r2 = unsafe { __torajs_str_from_char_code(0x141) }; // truncates to 0x41 = 'A'
        assert_eq!(read_str(r), b"A");
        assert_eq!(read_str(r2), b"A");
        unsafe { __torajs_str_free(r) };
        unsafe { __torajs_str_free(r2) };
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
        let roob = unsafe { __torajs_str_at(s, 100) };
        assert_eq!(read_str(r0), b"h");
        assert_eq!(read_str(rn), b"o");
        assert_eq!(read_str(roob), b"");
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(r0) };
        unsafe { __torajs_str_free(rn) };
        unsafe { __torajs_str_free(roob) };
    }
}
