//! `__torajs_str_slice` — `s.slice(start, end)`.
//!
//! Port of `ssa_inkwell::define_str_slice` (P3.1-g.5, 2026-05-23).
//! Returns a fresh **Str** (not a Substr view); the IR-side
//! `__torajs_substr_slice` is a separate fn on the Substr layout
//! and stays in `substr.rs`.
//!
//! Negative-index semantics differ from `substring`:
//!
//! - `slice` **wraps** negative inputs to `max(len + n, 0)`
//! - `substring` clamps negative inputs to 0 (and swaps if
//!   `start > end`; `slice` does NOT swap — out-of-order ranges
//!   yield empty).
//!
//! Both clamp positive inputs to `[0, len]` and produce a fresh
//! allocation holding `s[start..end]`.
//!
//! ## P11.1-S2.5 encoding-aware
//!
//! `start` / `end` are interpreted as ES code-unit indices (per
//! `String.prototype.slice` spec). The source's `length` is the
//! code-unit count; the byte offsets into the source's payload
//! come from `index × stride` (1 for Latin-1, 2 for UTF-16). The
//! result is allocated with the **same** encoding as the source —
//! slicing never changes the codepoint set, so the canonical
//! encoding picked for the source is also right for the slice.

use crate::block::StrBlock;
use crate::layout::{STR_DATA_OFF, STR_FLAG_IS_LATIN1, STR_LEN_OFF};
use torajs_rc::HeapHeader;

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

// ============================================================
// Pure-Rust core
// ============================================================

/// Resolve `s.slice(start, end)` to a `(lo, hi)` code-unit index
/// pair with `0 <= lo <= hi <= len`. Negative inputs wrap;
/// positive inputs clamp.
///
/// Per ES §22.1.3.21:
/// - `start < 0` → `max(len + start, 0)`; `start >= 0` → `min(start, len)`
/// - same for `end`
/// - if `end < start` after the above, the range is empty (`hi = lo`)
#[inline]
pub fn slice_range(start: i64, end: i64, len: u32) -> (u32, u32) {
    let ilen = len as i64;
    let lo = if start < 0 {
        (ilen + start).max(0)
    } else {
        start.min(ilen)
    };
    let hi_raw = if end < 0 {
        (ilen + end).max(0)
    } else {
        end.min(ilen)
    };
    let hi = hi_raw.max(lo);
    (lo as u32, hi as u32)
}

// ============================================================
// extern "C" wrapper
// ============================================================

/// `s.slice(start, end)` — fresh Str holding `s[start..end]` in
/// code-unit terms, with `slice` negative-wrap + clamp semantics.
///
/// # Safety
///
/// `s` must be a valid Str heap block. Returned pointer is a
/// fresh refcount=1 Str block owned by the caller.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_slice(s: *const u8, start: i64, end: i64) -> *mut u8 {
    let (payload, length, is_latin1) = unsafe { str_view(s) };
    let (lo, hi) = slice_range(start, end, length);
    let new_length = hi - lo;
    let stride = if is_latin1 { 1 } else { 2 };
    let mut block = StrBlock::alloc_with_encoding(new_length, is_latin1);
    if new_length > 0 {
        let lo_b = (lo as usize) * stride;
        let hi_b = (hi as usize) * stride;
        let byte_cnt = (hi_b - lo_b) as u32;
        let dst = unsafe { block.as_bytes_mut(byte_cnt) };
        dst.copy_from_slice(&payload[lo_b..hi_b]);
    }
    block.into_raw()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::__torajs_str_free;
    use alloc::vec::Vec;

    fn make_str(payload: &[u8]) -> *mut u8 {
        let mut b = StrBlock::alloc(payload.len() as u32);
        let dst = unsafe { b.as_bytes_mut(payload.len() as u32) };
        dst.copy_from_slice(payload);
        b.into_raw()
    }

    fn read_payload(p: *const u8) -> Vec<u8> {
        let (payload, _, _) = unsafe { str_view(p) };
        payload.to_vec()
    }

    #[test]
    fn range_positive_basic() {
        assert_eq!(slice_range(1, 4, 10), (1, 4));
        assert_eq!(slice_range(0, 10, 10), (0, 10));
        assert_eq!(slice_range(5, 5, 10), (5, 5));
    }

    #[test]
    fn range_negative_wraps() {
        assert_eq!(slice_range(-3, 10, 10), (7, 10));
        assert_eq!(slice_range(-2, -1, 10), (8, 9));
        assert_eq!(slice_range(-100, 5, 10), (0, 5));
    }

    #[test]
    fn range_over_clamps() {
        assert_eq!(slice_range(20, 30, 10), (10, 10));
        assert_eq!(slice_range(5, 100, 10), (5, 10));
    }

    #[test]
    fn range_end_lt_start_yields_empty() {
        assert_eq!(slice_range(7, 3, 10), (7, 7));
        assert_eq!(slice_range(-2, -5, 10), (8, 8));
    }

    #[test]
    fn ffi_slice_basic_latin1() {
        let s = make_str(b"hello");
        let r = unsafe { __torajs_str_slice(s, 1, 4) };
        assert_eq!(read_payload(r), b"ell");
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(r) };
    }

    #[test]
    fn ffi_slice_negative_wrap() {
        let s = make_str(b"hello");
        let r = unsafe { __torajs_str_slice(s, -3, 5) };
        assert_eq!(read_payload(r), b"llo");
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(r) };
    }

    #[test]
    fn ffi_slice_no_swap_yields_empty() {
        let s = make_str(b"hello");
        let r = unsafe { __torajs_str_slice(s, 3, 1) };
        assert_eq!(read_payload(r), b"");
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(r) };
    }

    #[test]
    fn ffi_slice_full() {
        let s = make_str(b"hello");
        let r = unsafe { __torajs_str_slice(s, 0, 5) };
        assert_eq!(read_payload(r), b"hello");
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(r) };
    }

    #[test]
    fn ffi_slice_utf16_round_trips() {
        // Build a UTF-16 Str "AB" (length=2, payload="\x41\x00\x42\x00")
        // and slice [1..2] → "B".
        let mut b = StrBlock::alloc_with_encoding(2, false);
        unsafe { b.as_bytes_mut(4) }.copy_from_slice(&[0x41, 0x00, 0x42, 0x00]);
        let s = b.into_raw();
        let r = unsafe { __torajs_str_slice(s, 1, 2) };
        // Result payload should be the second u16 little-endian.
        assert_eq!(read_payload(r), [0x42, 0x00]);
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(r) };
    }
}
