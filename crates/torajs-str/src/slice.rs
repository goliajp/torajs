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

use crate::alloc::StrBlock;
use crate::layout::{STR_DATA_OFF, STR_LEN_OFF};

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

// ============================================================
// Pure-Rust core
// ============================================================

/// Resolve `s.slice(start, end)` to a `(lo, hi)` byte-index pair
/// with `0 <= lo <= hi <= len`. Negative inputs wrap; positive
/// inputs clamp.
///
/// Per ES §22.1.3.21:
/// - `start < 0` → `max(len + start, 0)`; `start >= 0` → `min(start, len)`
/// - same for `end`
/// - if `end < start` after the above, the range is empty (`hi = lo`)
#[inline]
pub fn slice_range(start: i64, end: i64, len: u64) -> (u64, u64) {
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
    (lo as u64, hi as u64)
}

// ============================================================
// extern "C" wrapper
// ============================================================

/// `s.slice(start, end)` — fresh Str holding `s[start..end]`,
/// with `slice` negative-wrap + clamp semantics.
///
/// # Safety
///
/// `s` must be a valid Str heap block. Returned pointer is a
/// fresh refcount=1 Str block owned by the caller.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_slice(s: *const u8, start: i64, end: i64) -> *mut u8 {
    let len = unsafe { str_len(s) };
    let (lo, hi) = slice_range(start, end, len);
    let new_len = hi - lo;
    let mut block = StrBlock::alloc(new_len);
    if new_len > 0 {
        let src = unsafe { str_bytes(s, len) };
        let dst = unsafe { block.as_bytes_mut(new_len) };
        dst.copy_from_slice(&src[lo as usize..hi as usize]);
    }
    block.into_raw()
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn ffi_slice_basic() {
        let s = make_str(b"hello");
        let r = unsafe { __torajs_str_slice(s, 1, 4) };
        assert_eq!(read_str(r), b"ell");
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(r) };
    }

    #[test]
    fn ffi_slice_negative_wrap() {
        let s = make_str(b"hello");
        let r = unsafe { __torajs_str_slice(s, -3, 5) };
        assert_eq!(read_str(r), b"llo");
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(r) };
    }

    #[test]
    fn ffi_slice_no_swap_yields_empty() {
        let s = make_str(b"hello");
        let r = unsafe { __torajs_str_slice(s, 3, 1) };
        assert_eq!(read_str(r), b"");
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(r) };
    }

    #[test]
    fn ffi_slice_full() {
        let s = make_str(b"hello");
        let r = unsafe { __torajs_str_slice(s, 0, 5) };
        assert_eq!(read_str(r), b"hello");
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(r) };
    }
}
