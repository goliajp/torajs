//! `__torajs_str_concat` — fresh Str holding `a ++ b`.
//!
//! Port of `ssa_inkwell::define_str_concat` (P3.1-g.4, 2026-05-23).
//! Pool-aware alloc plus encoding-aware payload writes; both operand
//! Strs are read-only and the caller's drops still fire on them.
//!
//! Returns NULL only if alloc panics on OOM (matches the IR shape:
//! str_alloc_pooled abort-on-OOM). Inputs are valid Str heap blocks
//! OR NULL — a NULL operand denotes the JS `undefined` value and
//! concats as the text "undefined" (RC-4 F1b-2, see the fn body).
//!
//! ## P11.1-S2.1 encoding-aware
//!
//! Pre-S2 every Str payload was raw byte tape so concat was a
//! single `length = a.length + b.length` alloc + two
//! `copy_from_slice`. Post-S2 Str payloads are either Latin-1 (one
//! byte per code unit) or UTF-16 LE (two little-endian bytes per
//! code unit), and `length` is the code unit count. The combined
//! string's encoding is decided per ECMAScript-style "widest of
//! the inputs":
//!
//! - both Latin-1 → Latin-1 result, byte-wise copy of both
//!   payloads back-to-back.
//! - both UTF-16  → UTF-16  result, byte-wise copy of both u16-
//!   pair payloads back-to-back (each payload is already a flat
//!   little-endian byte stream so a single `copy_from_slice` per
//!   side suffices).
//! - mixed        → UTF-16 result. The Latin-1 side is widened
//!   per code unit to the matching u16 (`b → (b as u16)` little-
//!   endian, top byte zero); the UTF-16 side copies verbatim.
//!
//! ASCII concat stays on the all-Latin-1 fast path with the same
//! shape as pre-S2; only mixed-encoding concats pay the widen cost.

use crate::block::StrBlock;
use crate::layout::{STR_DATA_OFF, STR_FLAG_IS_LATIN1, STR_LEN_OFF};
use torajs_rc::HeapHeader;

// ============================================================
// Layout-aware FFI helpers (sub-module-local)
// ============================================================

#[inline]
pub(crate) unsafe fn str_len(p: *const u8) -> u32 {
    unsafe { (p.add(STR_LEN_OFF) as *const u32).read() }
}

#[inline]
pub(crate) unsafe fn str_payload<'a>(p: *const u8, byte_count: usize) -> &'a [u8] {
    unsafe { core::slice::from_raw_parts(p.add(STR_DATA_OFF), byte_count) }
}

#[inline]
pub(crate) unsafe fn str_is_latin1(p: *const u8) -> bool {
    let header = unsafe { &*(p as *const HeapHeader) };
    (header.flags & STR_FLAG_IS_LATIN1) != 0
}

/// Write `src` (Latin-1 payload, one byte per code unit) into
/// `dst` widened to UTF-16 LE (two bytes per code unit, high byte
/// zero). `dst.len()` must equal `src.len() * 2`.
#[inline]
pub(crate) fn widen_latin1_into_utf16(src: &[u8], dst: &mut [u8]) {
    debug_assert_eq!(dst.len(), src.len() * 2);
    for (i, &b) in src.iter().enumerate() {
        dst[i * 2] = b;
        dst[i * 2 + 1] = 0;
    }
}

// ============================================================
// extern "C" wrapper
// ============================================================

/// `a + b` for Str operands. Allocates a fresh refcount=1 result
/// holding `a` followed by `b`, with encoding picked per the
/// module-doc widest-of-inputs rule.
///
/// # Safety
///
/// `a` and `b` must be valid Str heap blocks. Returned pointer is
/// a fresh refcount=1 Str block owned by the caller.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_concat(a: *const u8, b: *const u8) -> *mut u8 {
    // RC-4 F1b-2 — a NULL ptr in a Str-typed slot denotes the JS
    // `undefined` value (uncaptured regex group, `[.., undefined]`
    // array-literal slot, typed OOB elem load); ES §13.15.3
    // ToString(undefined) concats the text "undefined". Cold path:
    // materialize the literal, recurse, drop the temp.
    if a.is_null() || b.is_null() {
        let undef = unsafe { crate::literals::__torajs_undefined_to_str() };
        let r = if a.is_null() && b.is_null() {
            unsafe { __torajs_str_concat(undef, undef) }
        } else if a.is_null() {
            unsafe { __torajs_str_concat(undef, b) }
        } else {
            unsafe { __torajs_str_concat(a, undef) }
        };
        unsafe { crate::str_drop::__torajs_str_drop(undef) };
        return r;
    }
    let a_len = unsafe { str_len(a) };
    let b_len = unsafe { str_len(b) };
    let total_len = a_len + b_len;
    let a_is_latin1 = unsafe { str_is_latin1(a) };
    let b_is_latin1 = unsafe { str_is_latin1(b) };
    let out_is_latin1 = a_is_latin1 && b_is_latin1;
    let mut block = StrBlock::alloc_with_encoding(total_len, out_is_latin1);
    if total_len == 0 {
        return block.into_raw();
    }
    let a_byte_cnt = if a_is_latin1 {
        a_len as usize
    } else {
        (a_len as usize) * 2
    };
    let b_byte_cnt = if b_is_latin1 {
        b_len as usize
    } else {
        (b_len as usize) * 2
    };
    let out_byte_cnt = if out_is_latin1 {
        total_len as usize
    } else {
        (total_len as usize) * 2
    };
    let a_out_byte_cnt = if out_is_latin1 {
        a_len as usize
    } else {
        (a_len as usize) * 2
    };
    let b_out_byte_cnt = if out_is_latin1 {
        b_len as usize
    } else {
        (b_len as usize) * 2
    };
    let dst = unsafe { block.as_bytes_mut(out_byte_cnt as u32) };
    if a_byte_cnt > 0 {
        let a_payload = unsafe { str_payload(a, a_byte_cnt) };
        if a_is_latin1 == out_is_latin1 {
            dst[..a_out_byte_cnt].copy_from_slice(a_payload);
        } else {
            // a is Latin-1 but output is UTF-16 — widen.
            widen_latin1_into_utf16(a_payload, &mut dst[..a_out_byte_cnt]);
        }
    }
    if b_byte_cnt > 0 {
        let b_payload = unsafe { str_payload(b, b_byte_cnt) };
        let b_slot = &mut dst[a_out_byte_cnt..a_out_byte_cnt + b_out_byte_cnt];
        if b_is_latin1 == out_is_latin1 {
            b_slot.copy_from_slice(b_payload);
        } else {
            // b is Latin-1 but output is UTF-16 — widen.
            widen_latin1_into_utf16(b_payload, b_slot);
        }
    }
    block.into_raw()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::__torajs_str_free;
    use crate::layout::byte_capacity;
    use alloc::vec::Vec;

    fn make_latin1(payload: &[u8]) -> *mut u8 {
        let mut b = StrBlock::alloc_with_encoding(payload.len() as u32, true);
        let dst = unsafe { b.as_bytes_mut(payload.len() as u32) };
        dst.copy_from_slice(payload);
        b.into_raw()
    }

    fn make_utf16(units: &[u16]) -> *mut u8 {
        let length = units.len() as u32;
        let byte_cap = byte_capacity(length, false);
        let mut b = StrBlock::alloc_with_encoding(length, false);
        let dst = unsafe { b.as_bytes_mut(byte_cap) };
        for (i, &u) in units.iter().enumerate() {
            let le = u.to_le_bytes();
            dst[i * 2] = le[0];
            dst[i * 2 + 1] = le[1];
        }
        b.into_raw()
    }

    fn read_str(p: *const u8) -> (Vec<u8>, bool, u32) {
        let len = unsafe { str_len(p) };
        let is_latin1 = unsafe { str_is_latin1(p) };
        let byte_cnt = if is_latin1 {
            len as usize
        } else {
            (len as usize) * 2
        };
        let payload = unsafe { str_payload(p, byte_cnt) }.to_vec();
        (payload, is_latin1, len)
    }

    #[test]
    fn concat_basic() {
        let a = make_latin1(b"foo");
        let b = make_latin1(b"bar");
        let r = unsafe { __torajs_str_concat(a, b) };
        let (payload, is_latin1, len) = read_str(r);
        assert_eq!(payload, b"foobar");
        assert!(is_latin1);
        assert_eq!(len, 6);
        unsafe { __torajs_str_free(a) };
        unsafe { __torajs_str_free(b) };
        unsafe { __torajs_str_free(r) };
    }

    #[test]
    fn concat_first_empty() {
        let a = make_latin1(b"");
        let b = make_latin1(b"bar");
        let r = unsafe { __torajs_str_concat(a, b) };
        let (payload, _, len) = read_str(r);
        assert_eq!(payload, b"bar");
        assert_eq!(len, 3);
        unsafe { __torajs_str_free(a) };
        unsafe { __torajs_str_free(b) };
        unsafe { __torajs_str_free(r) };
    }

    #[test]
    fn concat_second_empty() {
        let a = make_latin1(b"foo");
        let b = make_latin1(b"");
        let r = unsafe { __torajs_str_concat(a, b) };
        let (payload, _, len) = read_str(r);
        assert_eq!(payload, b"foo");
        assert_eq!(len, 3);
        unsafe { __torajs_str_free(a) };
        unsafe { __torajs_str_free(b) };
        unsafe { __torajs_str_free(r) };
    }

    #[test]
    fn concat_both_empty() {
        let a = make_latin1(b"");
        let b = make_latin1(b"");
        let r = unsafe { __torajs_str_concat(a, b) };
        let (payload, _, len) = read_str(r);
        assert_eq!(payload, b"");
        assert_eq!(len, 0);
        unsafe { __torajs_str_free(a) };
        unsafe { __torajs_str_free(b) };
        unsafe { __torajs_str_free(r) };
    }

    #[test]
    fn concat_two_utf16_yields_utf16() {
        // "中" (U+4E2D) + "文" (U+6587) → UTF-16 LE 4 bytes,
        // length=2.
        let a = make_utf16(&[0x4E2D]);
        let b = make_utf16(&[0x6587]);
        let r = unsafe { __torajs_str_concat(a, b) };
        let (payload, is_latin1, len) = read_str(r);
        assert!(!is_latin1);
        assert_eq!(len, 2);
        assert_eq!(payload, [0x2D, 0x4E, 0x87, 0x65]);
        unsafe { __torajs_str_free(a) };
        unsafe { __torajs_str_free(b) };
        unsafe { __torajs_str_free(r) };
    }

    #[test]
    fn concat_latin1_then_utf16_widens_latin1_side() {
        // "ab" Latin-1 + "中" UTF-16 → UTF-16 LE 6 bytes, length=3.
        let a = make_latin1(b"ab");
        let b = make_utf16(&[0x4E2D]);
        let r = unsafe { __torajs_str_concat(a, b) };
        let (payload, is_latin1, len) = read_str(r);
        assert!(!is_latin1);
        assert_eq!(len, 3);
        // a widened: 'a' (0x61, 0x00), 'b' (0x62, 0x00).
        // b verbatim: 0x2D, 0x4E.
        assert_eq!(payload, [0x61, 0x00, 0x62, 0x00, 0x2D, 0x4E]);
        unsafe { __torajs_str_free(a) };
        unsafe { __torajs_str_free(b) };
        unsafe { __torajs_str_free(r) };
    }

    #[test]
    fn concat_utf16_then_latin1_widens_latin1_side() {
        // "中" UTF-16 + " " Latin-1 → UTF-16 LE 4 bytes, length=2.
        let a = make_utf16(&[0x4E2D]);
        let b = make_latin1(b" ");
        let r = unsafe { __torajs_str_concat(a, b) };
        let (payload, is_latin1, len) = read_str(r);
        assert!(!is_latin1);
        assert_eq!(len, 2);
        assert_eq!(payload, [0x2D, 0x4E, 0x20, 0x00]);
        unsafe { __torajs_str_free(a) };
        unsafe { __torajs_str_free(b) };
        unsafe { __torajs_str_free(r) };
    }
}
