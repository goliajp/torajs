//! Str whitespace trim — `s.trim()` / `s.trimStart()` / `s.trimEnd()`.
//!
//! **ASCII whitespace set**: space / tab (`\t`) / LF (`\n`) / CR
//! (`\r`) / VT (`\x0b`) / FF (`\x0c`). Matches the pre-rewrite C
//! `is_trim_ws_` predicate bit-for-bit. Unicode whitespace beyond
//! ASCII (NBSP `\xA0`, U+2028 line separator, etc.) is NOT
//! trimmed — same as the prior subset behavior.
//!
//! P11.1-S2.5 — encoding-aware iteration: for a Latin-1 payload the
//! per-byte ASCII whitespace test runs as before; for a UTF-16 LE
//! payload each candidate code unit is read as a little-endian u16
//! and only matched against the ASCII whitespace set when its high
//! byte is zero (BMP code unit value ≤ 0xFF). Source and result
//! encodings always match.
//!
//! IR-side surface (declared in `ssa_lower::lower`, intrinsic
//! noalias-whitelisted in `ssa_inkwell::is_alloc_intrinsic`):
//! - `__torajs_str_trim(s) -> Str`
//! - `__torajs_str_trim_start(s) -> Str`
//! - `__torajs_str_trim_end(s) -> Str`

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
// Pure-Rust cores
// ============================================================

/// ASCII trim-whitespace predicate. Single byte test — LLVM lowers
/// to a small jump-table or branchless compare-chain at `-O3`.
#[inline]
pub fn is_trim_ws(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

/// First non-whitespace **byte** offset in a Latin-1 payload.
/// Returns `s.len()` if every byte is whitespace.
#[inline]
pub fn trim_start_idx(s: &[u8]) -> usize {
    let mut lo = 0;
    while lo < s.len() && is_trim_ws(s[lo]) {
        lo += 1;
    }
    lo
}

/// One past the last non-whitespace **byte** offset in a Latin-1
/// payload, restricted to `min..=s.len()`. Returns `min` if every
/// byte in that range is whitespace.
#[inline]
pub fn trim_end_idx(s: &[u8], min: usize) -> usize {
    let mut hi = s.len();
    while hi > min && is_trim_ws(s[hi - 1]) {
        hi -= 1;
    }
    hi
}

/// UTF-16 LE variant of [`trim_start_idx`] — steps by 2 bytes per
/// code unit. Only u16s whose high byte is zero and whose low byte
/// matches the ASCII whitespace set are dropped, mirroring the
/// Latin-1 path.
#[inline]
fn trim_start_idx_utf16(payload: &[u8]) -> usize {
    let mut lo = 0;
    while lo + 1 < payload.len() && payload[lo + 1] == 0 && is_trim_ws(payload[lo]) {
        lo += 2;
    }
    lo
}

#[inline]
fn trim_end_idx_utf16(payload: &[u8], min: usize) -> usize {
    let mut hi = payload.len();
    while hi >= min + 2 && payload[hi - 1] == 0 && is_trim_ws(payload[hi - 2]) {
        hi -= 2;
    }
    hi
}

/// Allocate a fresh Str block holding `src[range]` under
/// `is_latin1`. The byte range must already be aligned to the
/// encoding's code-unit stride.
#[inline]
fn alloc_payload(src: &[u8], is_latin1: bool) -> *mut u8 {
    let stride: u32 = if is_latin1 { 1 } else { 2 };
    let byte_cnt = src.len() as u32;
    let length = byte_cnt / stride;
    let mut block = StrBlock::alloc_with_encoding(length, is_latin1);
    if !src.is_empty() {
        let dst = unsafe { block.as_bytes_mut(byte_cnt) };
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
    let (payload, _, is_latin1) = unsafe { str_view(s) };
    let lo = if is_latin1 {
        trim_start_idx(payload)
    } else {
        trim_start_idx_utf16(payload)
    };
    alloc_payload(&payload[lo..], is_latin1)
}

/// `s.trimEnd()` — drop trailing ASCII whitespace.
///
/// # Safety
///
/// See [`__torajs_str_trim_start`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_trim_end(s: *const u8) -> *mut u8 {
    let (payload, _, is_latin1) = unsafe { str_view(s) };
    let hi = if is_latin1 {
        trim_end_idx(payload, 0)
    } else {
        trim_end_idx_utf16(payload, 0)
    };
    alloc_payload(&payload[..hi], is_latin1)
}

/// `s.trim()` — drop both leading and trailing ASCII whitespace.
///
/// # Safety
///
/// See [`__torajs_str_trim_start`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_trim(s: *const u8) -> *mut u8 {
    let (payload, _, is_latin1) = unsafe { str_view(s) };
    let (lo, hi) = if is_latin1 {
        let lo = trim_start_idx(payload);
        (lo, trim_end_idx(payload, lo))
    } else {
        let lo = trim_start_idx_utf16(payload);
        (lo, trim_end_idx_utf16(payload, lo))
    };
    alloc_payload(&payload[lo..hi], is_latin1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

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

    #[test]
    fn utf16_trim_start_strips_ascii_ws_pairs() {
        // "  AB" in UTF-16 LE = [0x20 0x00 0x20 0x00 0x41 0x00 0x42 0x00]
        let payload = [0x20, 0x00, 0x20, 0x00, 0x41, 0x00, 0x42, 0x00];
        assert_eq!(trim_start_idx_utf16(&payload), 4);
    }

    #[test]
    fn utf16_trim_end_strips_ascii_ws_pairs() {
        // "AB  " in UTF-16 LE
        let payload = [0x41, 0x00, 0x42, 0x00, 0x20, 0x00, 0x20, 0x00];
        assert_eq!(trim_end_idx_utf16(&payload, 0), 4);
    }

    #[test]
    fn utf16_trim_preserves_non_ascii_ws() {
        // U+3000 (ideographic space) is whitespace in Unicode but not
        // in the ASCII trim set — UTF-16 LE bytes [0x00 0x30]. Must
        // NOT be trimmed.
        let payload = [0x00, 0x30, 0x41, 0x00];
        assert_eq!(trim_start_idx_utf16(&payload), 0);
        assert_eq!(trim_end_idx_utf16(&payload, 0), 4);
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

    fn read_payload(p: *const u8) -> Vec<u8> {
        let (payload, _, _) = unsafe { str_view(p) };
        payload.to_vec()
    }

    #[test]
    fn ffi_trim_strips_both_sides() {
        let s = make_str(b"   hello   ");
        let r = unsafe { __torajs_str_trim(s) };
        assert_eq!(read_payload(r), b"hello");
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(r) };
    }

    #[test]
    fn ffi_trim_start_keeps_trailing() {
        let s = make_str(b"   hello   ");
        let r = unsafe { __torajs_str_trim_start(s) };
        assert_eq!(read_payload(r), b"hello   ");
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(r) };
    }

    #[test]
    fn ffi_trim_end_keeps_leading() {
        let s = make_str(b"   hello   ");
        let r = unsafe { __torajs_str_trim_end(s) };
        assert_eq!(read_payload(r), b"   hello");
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(r) };
    }

    #[test]
    fn ffi_trim_all_whitespace_yields_empty() {
        let s = make_str(b"  \t\n\r\x0b\x0c  ");
        let r = unsafe { __torajs_str_trim(s) };
        assert_eq!(read_payload(r), b"");
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(r) };
    }

    #[test]
    fn ffi_trim_no_ws_yields_passthrough() {
        let s = make_str(b"hello");
        let r = unsafe { __torajs_str_trim(s) };
        assert_eq!(read_payload(r), b"hello");
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(r) };
    }
}
