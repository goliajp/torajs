//! Str whitespace trim — `s.trim()` / `s.trimStart()` / `s.trimEnd()`.
//!
//! **ASCII whitespace set**: space / tab (`\t`) / LF (`\n`) / CR
//! (`\r`) / VT (`\x0b`) / FF (`\x0c`). Matches the pre-rewrite C
//! `is_trim_ws_` predicate bit-for-bit. Unicode whitespace beyond
//! ASCII (NBSP `\xA0`, U+2028 line separator, etc.) is NOT
//! trimmed — same as the prior subset behavior.
//!
//! Bun-parity: holds for any input whose surrounding whitespace is
//! drawn from the ASCII set above. JS spec actually trims a wider
//! WhiteSpace + LineTerminator union (ES §22.1.3.32), but the
//! shipped curated fixture set uses only the ASCII subset (and
//! conformance has already been green on this contract through
//! P3.0 onward).
//!
//! IR-side surface (declared in `ssa_lower::lower`, intrinsic
//! noalias-whitelisted in `ssa_inkwell::is_alloc_intrinsic`):
//! - `__torajs_str_trim(s) -> Str`
//! - `__torajs_str_trim_start(s) -> Str`
//! - `__torajs_str_trim_end(s) -> Str`

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
// Pure-Rust cores
// ============================================================

/// ASCII trim-whitespace predicate. Single byte test — LLVM lowers
/// to a small jump-table or branchless compare-chain at `-O3`.
#[inline]
pub fn is_trim_ws(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

/// First non-whitespace byte index. Returns `s.len()` if every
/// byte is whitespace.
#[inline]
pub fn trim_start_idx(s: &[u8]) -> usize {
    let mut lo = 0;
    while lo < s.len() && is_trim_ws(s[lo]) {
        lo += 1;
    }
    lo
}

/// One past the last non-whitespace byte index, restricted to
/// `min..=s.len()`. Returns `min` if every byte in that range is
/// whitespace. `min` lets `trim` share this with `trim_end` while
/// avoiding a double-scan of the leading whitespace.
#[inline]
pub fn trim_end_idx(s: &[u8], min: usize) -> usize {
    let mut hi = s.len();
    while hi > min && is_trim_ws(s[hi - 1]) {
        hi -= 1;
    }
    hi
}

/// Allocate a fresh Str block holding `src[range]`. Shared by all
/// three trim wrappers so the alloc + copy + into_raw boilerplate
/// is one line at each call site.
#[inline]
unsafe fn alloc_slice(src: &[u8]) -> *mut u8 {
    let out_len = src.len() as u64;
    let mut block = StrBlock::alloc(out_len);
    if !src.is_empty() {
        let dst = unsafe { block.as_bytes_mut(out_len) };
        dst.copy_from_slice(src);
    }
    block.into_raw()
}

// ============================================================
// extern "C" wrappers — preserve pre-rewrite ABI bit-for-bit
// ============================================================

/// `s.trimStart()` — drop leading ASCII whitespace.
///
/// # Safety
///
/// `s` must be a valid Str heap block. Returned pointer is a fresh
/// refcount=1 Str block owned by the caller.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_trim_start(s: *const u8) -> *mut u8 {
    let len = unsafe { str_len(s) };
    let src = unsafe { str_bytes(s, len) };
    let lo = trim_start_idx(src);
    unsafe { alloc_slice(&src[lo..]) }
}

/// `s.trimEnd()` — drop trailing ASCII whitespace.
///
/// # Safety
///
/// See [`__torajs_str_trim_start`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_trim_end(s: *const u8) -> *mut u8 {
    let len = unsafe { str_len(s) };
    let src = unsafe { str_bytes(s, len) };
    let hi = trim_end_idx(src, 0);
    unsafe { alloc_slice(&src[..hi]) }
}

/// `s.trim()` — drop both leading and trailing ASCII whitespace.
///
/// # Safety
///
/// See [`__torajs_str_trim_start`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_trim(s: *const u8) -> *mut u8 {
    let len = unsafe { str_len(s) };
    let src = unsafe { str_bytes(s, len) };
    let lo = trim_start_idx(src);
    let hi = trim_end_idx(src, lo);
    unsafe { alloc_slice(&src[lo..hi]) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ws_predicate_recognizes_ascii_set() {
        for c in [b' ', b'\t', b'\n', b'\r', 0x0bu8, 0x0cu8] {
            assert!(is_trim_ws(c), "{c:#04x} should be ws");
        }
    }

    #[test]
    fn ws_predicate_rejects_non_ws() {
        for c in [b'a', b'Z', b'0', b'.', 0x00u8, 0xa0u8, 0xffu8] {
            assert!(!is_trim_ws(c), "{c:#04x} should NOT be ws");
        }
    }

    #[test]
    fn start_idx_basic() {
        assert_eq!(trim_start_idx(b"   hello"), 3);
        assert_eq!(trim_start_idx(b"hello"), 0);
        assert_eq!(trim_start_idx(b""), 0);
        assert_eq!(trim_start_idx(b"   "), 3);
        assert_eq!(trim_start_idx(b" \t\n\r\x0b\x0cX"), 6);
    }

    #[test]
    fn end_idx_basic() {
        assert_eq!(trim_end_idx(b"hello   ", 0), 5);
        assert_eq!(trim_end_idx(b"hello", 0), 5);
        assert_eq!(trim_end_idx(b"", 0), 0);
        assert_eq!(trim_end_idx(b"   ", 0), 0);
        assert_eq!(trim_end_idx(b"X \t\n\r\x0b\x0c", 0), 1);
    }

    #[test]
    fn end_idx_respects_min_bound() {
        // min == post-leading-ws idx — emulates `trim()` second scan.
        assert_eq!(trim_end_idx(b"   ", 3), 3);
        assert_eq!(trim_end_idx(b"  hi  ", 2), 4);
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
    fn ffi_trim_strips_both_sides() {
        let s = make_str(b"   hello   ");
        let r = unsafe { __torajs_str_trim(s) };
        assert_eq!(read_str(r), b"hello");
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(r) };
    }

    #[test]
    fn ffi_trim_start_keeps_trailing() {
        let s = make_str(b"   hello   ");
        let r = unsafe { __torajs_str_trim_start(s) };
        assert_eq!(read_str(r), b"hello   ");
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(r) };
    }

    #[test]
    fn ffi_trim_end_keeps_leading() {
        let s = make_str(b"   hello   ");
        let r = unsafe { __torajs_str_trim_end(s) };
        assert_eq!(read_str(r), b"   hello");
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(r) };
    }

    #[test]
    fn ffi_trim_all_whitespace_yields_empty() {
        let s = make_str(b"  \t\n\r\x0b\x0c  ");
        let r = unsafe { __torajs_str_trim(s) };
        assert_eq!(read_str(r), b"");
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(r) };
    }

    #[test]
    fn ffi_trim_no_ws_yields_passthrough() {
        let s = make_str(b"hello");
        let r = unsafe { __torajs_str_trim(s) };
        assert_eq!(read_str(r), b"hello");
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(r) };
    }
}
