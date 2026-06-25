//! Substr `trim` / `trimStart` / `trimEnd` / `trim_into` FFI shims +
//! the `trim_cu_bounds` helper they share. Extracted from
//! `substr_methods.rs` to keep that file under the 500-prod-LOC
//! file-size hard limit (`rules/common/file-size.md`). Pure
//! mechanical pull, no semantic change.

use core::ffi::c_void;

use torajs_rc::__torajs_rc_inc;

use crate::substr::{
    __torajs_substr_create, FLAG_SUBSTR_INLINE, SUBSTR_LEN_OFF, SUBSTR_OFFSET_OFF,
    SUBSTR_PARENT_OFF,
};
use crate::substr_methods::{substr_offset, substr_parent, substr_view};

#[inline]
fn substr_is_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0B | 0x0C)
}

/// Compute trim bounds for Substr `v` and return the
/// `(parent, code_unit_offset, code_unit_len)` triple that the
/// `_trim_*` family hands to [`__torajs_substr_create`] / writes
/// into the `_trim_into` stack buffer. `do_start` / `do_end` toggle
/// which sides scan (`_trim_start` skips end, `_trim_end` skips
/// start, `_trim` does both).
///
/// Encoding-aware (P11.1-S5): Latin-1 = byte-stride scan; UTF-16 LE
/// = u16-stride scan with high-byte-zero + low-byte-in-WS predicate.
/// Byte indices from the scan are divided by the parent's stride to
/// yield code-unit `(lo, hi)`.
#[inline]
unsafe fn trim_cu_bounds(v: *const u8, do_start: bool, do_end: bool) -> (*mut c_void, u64, u64) {
    let (payload, _cu_len, is_latin1) = unsafe { substr_view(v) };
    let v_off = unsafe { substr_offset(v) };
    let parent = unsafe { substr_parent(v) } as *mut c_void;
    let byte_len = payload.len();
    let (lo_byte, hi_byte) = if is_latin1 {
        let mut lo = 0usize;
        if do_start {
            while lo < byte_len && substr_is_ws(payload[lo]) {
                lo += 1;
            }
        }
        let mut hi = byte_len;
        if do_end {
            while hi > lo && substr_is_ws(payload[hi - 1]) {
                hi -= 1;
            }
        }
        (lo, hi)
    } else {
        let mut lo = 0usize;
        if do_start {
            while lo + 1 < byte_len && payload[lo + 1] == 0 && substr_is_ws(payload[lo]) {
                lo += 2;
            }
        }
        let mut hi = byte_len;
        if do_end {
            while hi >= lo + 2 && payload[hi - 1] == 0 && substr_is_ws(payload[hi - 2]) {
                hi -= 2;
            }
        }
        (lo, hi)
    };
    let stride = if is_latin1 { 1 } else { 2 };
    let cu_lo = (lo_byte / stride) as u64;
    let cu_hi = (hi_byte / stride) as u64;
    (parent, v_off + cu_lo, cu_hi - cu_lo)
}

/// `substr.trim()` — narrow leading + trailing ASCII whitespace.
/// Encoding-aware via [`trim_cu_bounds`]; the returned Substr
/// references the SAME root parent.
///
/// # Safety
/// `v` is a live `*const Substr`. Returned pointer is a fresh
/// Substr (rc=1).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_substr_trim(v: *const u8) -> *mut c_void {
    let (parent, off, len) = unsafe { trim_cu_bounds(v, true, true) };
    unsafe { __torajs_substr_create(parent, off, len) }
}

/// `substr.trimStart()` — encoding-aware via [`trim_cu_bounds`].
///
/// # Safety
/// See [`__torajs_substr_trim`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_substr_trim_start(v: *const u8) -> *mut c_void {
    let (parent, off, len) = unsafe { trim_cu_bounds(v, true, false) };
    unsafe { __torajs_substr_create(parent, off, len) }
}

/// `substr.trimEnd()` — encoding-aware via [`trim_cu_bounds`].
///
/// # Safety
/// See [`__torajs_substr_trim`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_substr_trim_end(v: *const u8) -> *mut c_void {
    let (parent, off, len) = unsafe { trim_cu_bounds(v, false, true) };
    unsafe { __torajs_substr_create(parent, off, len) }
}

/// Stack-write variant of [`__torajs_substr_trim`] — writes the
/// trimmed view into a caller-provided 32-byte buffer instead of
/// heap-allocating a fresh `SubstrBlock`. The buffer carries
/// [`FLAG_SUBSTR_INLINE`] so a subsequent `__torajs_substr_drop`
/// follows the INLINE branch (dec parent rc only, no pool push,
/// no free); the symmetric `rc_inc` is performed here.
///
/// P11.1-S5 — `len@8` / `offset@24` are written in JS code units
/// via [`trim_cu_bounds`].
///
/// # Safety
///
/// `v` must be a live `*const Substr`. `out_buf` must be a writable
/// 32-byte aligned region (typically a caller `alloca [32 x i8]`).
/// Caller MUST eventually invoke `__torajs_substr_drop(out_buf)`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_substr_trim_into(v: *const u8, out_buf: *mut u8) {
    let (parent, off, len) = unsafe { trim_cu_bounds(v, true, true) };
    // Header u64: refcount=0 @0..4, type_tag=Tag::Str=0 @4..6,
    // flags=FLAG_SUBSTR_INLINE @6..8 packed little-endian.
    let header_u64 = (FLAG_SUBSTR_INLINE as u64) << 48;
    unsafe {
        (out_buf as *mut u64).write(header_u64);
        (out_buf.add(SUBSTR_LEN_OFF) as *mut u64).write(len);
        (out_buf.add(SUBSTR_PARENT_OFF) as *mut *mut c_void).write(parent);
        (out_buf.add(SUBSTR_OFFSET_OFF) as *mut u64).write(off);
    }
    unsafe { __torajs_rc_inc(parent) };
}
