//! Str case-folding — `s.toUpperCase()` / `s.toLowerCase()`.
//!
//! **ASCII-only fold**. Bytes `'a'..='z'` swap to `'A'..='Z'` on
//! upper, and vice versa on lower; every other byte (including
//! multi-byte UTF-8 continuation bytes and any `>= 0x80` lead) is
//! copied through unchanged. This matches the pre-rewrite C
//! `__torajs_str_to_upper` / `_to_lower` behavior and the byte-
//! level Str layout the rest of the runtime operates on.
//!
//! Bun parity: holds for any input that is entirely ASCII. For
//! mixed-Unicode strings tora intentionally diverges (no Unicode
//! case-folding table, by design — see `docs/stdlib.md`); the
//! P3.1-e fixture set restricts inputs to ASCII so bun-parity
//! checks remain meaningful.
//!
//! IR-side surface (declared in `ssa_lower::lower` and consumed by
//! the `toUpperCase` / `toLowerCase` method dispatch in
//! `lower_expr` + the alloc-intrinsic noalias whitelist in
//! `ssa_inkwell::is_alloc_intrinsic`): `__torajs_str_to_upper(s)`
//! and `__torajs_str_to_lower(s)`, both `Str -> Str`.

use crate::block::StrBlock;
use crate::layout::{STR_DATA_OFF, STR_FLAG_IS_LATIN1, STR_LEN_OFF};
use torajs_rc::HeapHeader;

// ============================================================
// Layout-aware FFI helpers (sub-module-local; see mod.rs for why)
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

/// ASCII upper-fold from `src` into `dst`. Both slices must be the
/// same length; this is a single linear pass with no branches on
/// the dominant ASCII-uppercase / ASCII-non-letter input.
#[inline]
pub fn to_upper_into(src: &[u8], dst: &mut [u8]) {
    debug_assert_eq!(src.len(), dst.len());
    for (i, &c) in src.iter().enumerate() {
        // Branchless idiom: `c.is_ascii_lowercase()` is the range
        // check `'a'..='z'`. Subtracting 32 maps to upper; the cmp
        // result is folded into a conditional move by LLVM at -O3.
        dst[i] = if c.is_ascii_lowercase() { c - 32 } else { c };
    }
}

/// ASCII lower-fold from `src` into `dst`. Mirror of
/// [`to_upper_into`].
#[inline]
pub fn to_lower_into(src: &[u8], dst: &mut [u8]) {
    debug_assert_eq!(src.len(), dst.len());
    for (i, &c) in src.iter().enumerate() {
        dst[i] = if c.is_ascii_uppercase() { c + 32 } else { c };
    }
}

// ============================================================
// extern "C" wrappers — preserve pre-rewrite ABI bit-for-bit
// ============================================================

/// `s.toUpperCase()` — ASCII fold, single pool-aware alloc.
///
/// # Safety
///
/// `s` must be a valid Str heap block (header + len at offsets
/// `0..STR_DATA_OFF`, then `len` payload bytes). The returned
/// pointer is a fresh refcount=1 Str block; ownership transfers
/// to the caller.
/// Encoding-aware ASCII upper-fold: walks the payload at the
/// encoding's stride and only folds when the code unit is an
/// ASCII lowercase letter (Latin-1: byte 'a'..='z'; UTF-16 LE:
/// low byte 'a'..='z' AND high byte 0). Non-ASCII codepoints
/// pass through unchanged — Unicode case folding lands in P11.5
/// alongside the Unicode data tables.
#[inline]
fn fold_case(src: &[u8], dst: &mut [u8], is_latin1: bool, upper: bool) {
    debug_assert_eq!(src.len(), dst.len());
    dst.copy_from_slice(src);
    if is_latin1 {
        for b in dst.iter_mut() {
            if upper && b.is_ascii_lowercase() {
                *b -= 32;
            } else if !upper && b.is_ascii_uppercase() {
                *b += 32;
            }
        }
    } else {
        let mut i = 0;
        while i + 1 < dst.len() {
            if dst[i + 1] == 0 {
                let b = dst[i];
                if upper && b.is_ascii_lowercase() {
                    dst[i] = b - 32;
                } else if !upper && b.is_ascii_uppercase() {
                    dst[i] = b + 32;
                }
            }
            i += 2;
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_to_upper(s: *const u8) -> *mut u8 {
    let (payload, length, is_latin1) = unsafe { str_view(s) };
    let mut block = StrBlock::alloc_with_encoding(length, is_latin1);
    let byte_cnt = payload.len() as u32;
    if byte_cnt > 0 {
        let dst = unsafe { block.as_bytes_mut(byte_cnt) };
        fold_case(payload, dst, is_latin1, true);
    }
    block.into_raw()
}

/// `s.toLowerCase()` — ASCII fold, single pool-aware alloc.
///
/// # Safety
///
/// See [`__torajs_str_to_upper`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_to_lower(s: *const u8) -> *mut u8 {
    let (payload, length, is_latin1) = unsafe { str_view(s) };
    let mut block = StrBlock::alloc_with_encoding(length, is_latin1);
    let byte_cnt = payload.len() as u32;
    if byte_cnt > 0 {
        let dst = unsafe { block.as_bytes_mut(byte_cnt) };
        fold_case(payload, dst, is_latin1, false);
    }
    block.into_raw()
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    #[test]
    fn upper_into_basic_ascii() {
        let mut dst = [0u8; 5];
        to_upper_into(b"hello", &mut dst);
        assert_eq!(&dst, b"HELLO");
    }

    #[test]
    fn upper_into_preserves_already_upper_and_non_letter() {
        let mut dst = [0u8; 11];
        to_upper_into(b"Hi! 123 \xFFxy", &mut dst);
        assert_eq!(&dst, b"HI! 123 \xFFXY");
    }

    #[test]
    fn upper_into_passes_through_non_ascii_bytes() {
        // 'é' (UTF-8: C3 A9) must NOT case-fold; both bytes are >= 0x80.
        let mut dst = [0u8; 5];
        to_upper_into(b"\xC3\xA9foo", &mut dst);
        assert_eq!(&dst, b"\xC3\xA9FOO");
    }

    #[test]
    fn lower_into_basic_ascii() {
        let mut dst = [0u8; 5];
        to_lower_into(b"HELLO", &mut dst);
        assert_eq!(&dst, b"hello");
    }

    #[test]
    fn lower_into_preserves_already_lower_and_non_letter() {
        let mut dst = [0u8; 11];
        to_lower_into(b"Hi! 123 \xFFxy", &mut dst);
        assert_eq!(&dst, b"hi! 123 \xFFxy");
    }

    #[test]
    fn lower_into_passes_through_non_ascii_bytes() {
        let mut dst = [0u8; 5];
        to_lower_into(b"\xC3\x89FOO", &mut dst);
        assert_eq!(&dst, b"\xC3\x89foo");
    }

    #[test]
    fn empty_input_yields_empty_output() {
        let mut dst = [0u8; 0];
        to_upper_into(b"", &mut dst);
        to_lower_into(b"", &mut dst);
        assert!(dst.is_empty());
    }

    #[test]
    fn upper_then_lower_round_trips_letters() {
        let input = b"AbCdEfG";
        let mut up = [0u8; 7];
        let mut down = [0u8; 7];
        to_upper_into(input, &mut up);
        to_lower_into(&up, &mut down);
        assert_eq!(&down, b"abcdefg");
    }

    // ============================================================
    // FFI round-trip tests — exercise the extern "C" wrappers
    // through a real Str block alloc → fold → free cycle.
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
    fn ffi_to_upper_roundtrips() {
        let s = make_str(b"hello, world!");
        let r = unsafe { __torajs_str_to_upper(s) };
        assert_eq!(read_payload(r), b"HELLO, WORLD!");
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(r) };
    }

    #[test]
    fn ffi_to_lower_roundtrips() {
        let s = make_str(b"HELLO, WORLD!");
        let r = unsafe { __torajs_str_to_lower(s) };
        assert_eq!(read_payload(r), b"hello, world!");
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(r) };
    }
}
