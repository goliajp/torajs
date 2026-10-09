//! `__torajs_str_concat` — fresh Str holding `a ++ b`.
//!
//! Port of `ssa_inkwell::define_str_concat` (P3.1-g.4, 2026-05-23).
//! Pool-aware alloc plus encoding-aware payload writes; both operand
//! Strs are read-only and the caller's drops still fire on them. The
//! copy itself is [`crate::join`]'s.
//!
//! Returns NULL only if alloc panics on OOM (matches the IR shape:
//! str_alloc_pooled abort-on-OOM). Inputs are valid Str heap blocks
//! OR NULL — a NULL operand denotes the JS `null` value and concats
//! as the text "null" (see the fn body).
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

use crate::join::{Side, fits_one_str, join_general, join_or, too_long_keeping};
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
    // RFC 20260707 chunk 4 — a NULL ptr in a Str-typed slot denotes
    // the JS `null` value; ES §13.15.3 ToString(null) concats the text
    // "null". This arm read it as `undefined` under the pre-sentinel
    // 591 convention, when one runtime value carried both; chunk 2
    // gave `undefined` the immortal sentinel cell and left NULL to
    // mean what it says. Every producer named in the old comment
    // (uncaptured capture group, `[.., undefined]` literal slot, typed
    // OOB elem load) spells the sentinel now.
    if a.is_null() || b.is_null() {
        return unsafe { concat_null(a, b) };
    }
    unsafe {
        join_or(Side::of_str(a), Side::of_str(b), |ptr, _, _| {
            too_long_sharing(ptr)
        })
    }
}

/// Raise the RangeError for a concat past the longest Str and answer
/// the left cell itself with one more reference; see
/// [`too_long_shared`]. The cell is found back from its payload
/// pointer, so the hot path keeps nothing live for this call it almost
/// never makes.
///
/// # Safety
///
/// `payload` is the payload of a valid Str heap block
/// ([`Side::of_str`]).
#[cold]
#[inline(never)]
pub(crate) unsafe fn too_long_sharing(payload: *const u8) -> *mut u8 {
    unsafe { too_long_shared(payload.sub(STR_DATA_OFF)) }
}

/// Raise the RangeError for a concat past the longest Str and answer
/// the left cell `a` with one more reference: the caller stores the
/// answer before its throw check runs, so `s = s + x` leaves `s` as it
/// was, and no copy of a side that long is made.
///
/// # Safety
///
/// `a` is a valid Str heap block.
#[cold]
#[inline(never)]
pub(crate) unsafe fn too_long_shared(a: *const u8) -> *mut u8 {
    unsafe {
        crate::max_len::raise_too_long();
        torajs_rc::__torajs_rc_inc(a as *mut core::ffi::c_void);
    }
    a as *mut u8
}

/// `__torajs_str_concat` with a null operand: it reads as the text
/// "null".
#[cold]
#[inline(never)]
unsafe fn concat_null(a_cell: *const u8, b: *const u8) -> *mut u8 {
    let (a, b) = unsafe { (Side::of_nullable_str(a_cell), Side::of_nullable_str(b)) };
    if !fits_one_str(a, b) {
        if !a_cell.is_null() {
            return unsafe { too_long_shared(a_cell) };
        }
        return unsafe { too_long_keeping(a.ptr, a.len, a.flags) };
    }
    unsafe { join_general(a, b) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::{__torajs_str_free, StrBlock};
    use crate::layout::{STR_FLAG_IS_ASCII, byte_capacity};
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
    fn concat_is_known_ascii_only_when_both_sides_are() {
        let mark = |p: *mut u8| {
            unsafe { (&mut *(p as *mut HeapHeader)).flags |= STR_FLAG_IS_ASCII };
            p
        };
        let marked =
            |p: *const u8| unsafe { &*(p as *const HeapHeader) }.flags & STR_FLAG_IS_ASCII != 0;
        let a = mark(make_latin1(b"ab"));
        let b = mark(make_latin1(b"cd"));
        let unknown = make_latin1(b"ef");
        let both = unsafe { __torajs_str_concat(a, b) };
        let one = unsafe { __torajs_str_concat(a, unknown) };
        assert!(marked(both));
        assert!(!marked(one));
        for p in [a, b, unknown, both, one] {
            unsafe { __torajs_str_free(p) };
        }
    }

    #[test]
    fn a_concat_past_the_longest_string_raises_and_answers_its_left_cell() {
        let a = make_latin1(b"keep");
        let b = make_latin1(b"");
        // only the length is read before the bound refuses the join
        let set_len = |p: *mut u8, n: u32| unsafe { (p.add(STR_LEN_OFF) as *mut u32).write(n) };
        set_len(b, crate::max_len::STR_MAX_LEN as u32);
        crate::STUB_THROW_RAISED.store(false, core::sync::atomic::Ordering::SeqCst);
        let out = unsafe { __torajs_str_concat(a, b) };
        assert!(crate::STUB_THROW_RAISED.load(core::sync::atomic::Ordering::SeqCst));
        assert_eq!(out, a, "the left cell itself comes back");
        assert_eq!(unsafe { &*(a as *const HeapHeader) }.refcount, 2);
        set_len(b, 0);
        unsafe { crate::str_drop::__torajs_str_drop(out) };
        for p in [a, b] {
            unsafe { __torajs_str_free(p) };
        }
    }

    #[test]
    fn a_literal_or_number_tail_past_the_longest_string_answers_the_left_cell() {
        use crate::concat_lit::test_support::lit1;
        use core::sync::atomic::Ordering::SeqCst;
        let a = make_latin1(b"keep");
        // only the length is read before the bound refuses the join
        unsafe { (a.add(STR_LEN_OFF) as *mut u32).write(crate::max_len::STR_MAX_LEN as u32) };
        let (bits, meta) = lit1(b"x");
        let joins: [&dyn Fn() -> *mut u8; 2] = [
            &|| unsafe { crate::concat_lit::__torajs_str_concat_lit(a, bits, meta) },
            &|| unsafe { crate::concat_num::__torajs_str_concat_i64(a, 7) },
        ];
        for join in joins {
            crate::STUB_THROW_RAISED.store(false, SeqCst);
            let out = join();
            assert!(crate::STUB_THROW_RAISED.load(SeqCst));
            assert_eq!(out, a, "the left cell itself comes back");
            assert_eq!(unsafe { &*(a as *const HeapHeader) }.refcount, 2);
            unsafe { crate::str_drop::__torajs_str_drop(out) };
        }
        unsafe { (a.add(STR_LEN_OFF) as *mut u32).write(4) };
        unsafe { __torajs_str_free(a) };
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
    fn an_empty_utf16_side_leaves_the_width_to_the_other() {
        let empty = make_utf16(&[]);
        let b = make_latin1(b"b");
        unsafe { (&mut *(b as *mut HeapHeader)).flags |= STR_FLAG_IS_ASCII };
        for r in unsafe { [__torajs_str_concat(empty, b), __torajs_str_concat(b, empty)] } {
            assert_eq!(read_str(r), (b"b".to_vec(), true, 1));
            assert!(unsafe { &*(r as *const HeapHeader) }.flags & STR_FLAG_IS_ASCII != 0);
            unsafe { __torajs_str_free(r) };
        }
        let both = unsafe { __torajs_str_concat(empty, empty) };
        assert_eq!(read_str(both).2, 0);
        for p in [empty, b, both] {
            unsafe { __torajs_str_free(p) };
        }
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
