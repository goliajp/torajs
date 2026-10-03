//! `__torajs_str_append_lit` — `a ++ <short literal>` where the caller
//! hands over its reference to `a`.
//!
//! The literal travels by value, as [`crate::concat_lit`] lays out;
//! the ownership rule is [`crate::append`]'s. A cell nobody else holds
//! takes the literal into its slack — one or two stores and a new
//! length, with no call on the way — and grows by doubling when the
//! slack runs out. A shared cell, a `.rodata` literal or a view is
//! copied, and the reference that was handed over is released.
//!
//! A Latin-1 literal lands on a UTF-16 cell in place, widened as it is
//! written; a UTF-16 literal on a Latin-1 cell cannot, so that one
//! goes through the copy and comes back UTF-16.

use torajs_rc::HeapHeader;

use crate::append::is_solely_ours;
use crate::block::StrBlock;
use crate::concat_lit::{join_lit_general, lit_flags, lit_len, write_low_bytes, write_widened};
use crate::copy::copy_bytes;
use crate::join::Side;
use crate::layout::{
    STR_DATA_OFF, STR_FLAG_IS_ASCII, STR_FLAG_IS_LATIN1, STR_LEN_OFF, grow_capacity,
};
use crate::str_drop::__torajs_str_drop;

/// What an in-place append needs to know about the cell: its length,
/// the payload bytes that takes, and the bytes the literal adds.
struct Fit {
    len: u32,
    used: u32,
    need: u32,
    is_latin1: bool,
}

/// `Some` when `a` is a cell this call may write the literal onto
/// where it stands, room aside.
///
/// # Safety
///
/// `a` is a valid, non-null Str heap block.
#[inline(always)]
unsafe fn writable(a: *const u8, meta: u64) -> Option<Fit> {
    // SAFETY: caller's contract is a valid Str block at `a`.
    let header = unsafe { &*(a as *const HeapHeader) };
    let is_latin1 = header.flags & STR_FLAG_IS_LATIN1 != 0;
    // a Latin-1 cell cannot take a UTF-16 literal without widening
    let fits_width = !is_latin1 || lit_flags(meta) & STR_FLAG_IS_LATIN1 != 0;
    if !unsafe { is_solely_ours(header) } || !fits_width {
        return None;
    }
    let len = unsafe { (a.add(STR_LEN_OFF) as *const u32).read() };
    let shift = u32::from(!is_latin1);
    let used = len << shift;
    Some(Fit {
        len,
        used,
        need: used + (lit_len(meta) << shift),
        is_latin1,
    })
}

/// Write the literal after the `fit.used` payload bytes of `cell` and
/// record the new length.
///
/// # Safety
///
/// `cell` is a Str block this call alone holds, with room for
/// `fit.need` payload bytes.
#[inline(always)]
unsafe fn put(cell: *mut u8, fit: &Fit, bits: u64, meta: u64) {
    let n = lit_len(meta);
    let lit_flags = lit_flags(meta);
    unsafe {
        let dst = cell.add(STR_DATA_OFF + fit.used as usize);
        if fit.is_latin1 == (lit_flags & STR_FLAG_IS_LATIN1 != 0) {
            write_low_bytes(dst, bits, (fit.need - fit.used) as usize);
        } else {
            write_widened(dst, bits, n as usize);
        }
        (cell.add(STR_LEN_OFF) as *mut u32).write(fit.len + n);
        // the cell stays known-ASCII only if what landed on it is too
        if lit_flags & STR_FLAG_IS_ASCII == 0 {
            (*(cell as *mut HeapHeader)).flags &= !STR_FLAG_IS_ASCII;
        }
    }
}

/// `a + <literal>` where **the caller's reference to `a` is
/// consumed**. Answers an owned refcount-1 Str that aliases `a` when
/// the append happened in place.
///
/// # Safety
///
/// `a` is null or a valid Str heap block the caller owns one
/// reference to (released here); `bits` / `meta` describe a literal
/// as [`crate::concat_lit`] lays out.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_append_lit(a: *mut u8, bits: u64, meta: u64) -> *mut u8 {
    if !a.is_null()
        && let Some(fit) = unsafe { writable(a, meta) }
        && fit.need <= unsafe { StrBlock::from_raw(a).payload_capacity() }
    {
        unsafe { put(a, &fit, bits, meta) };
        return a;
    }
    unsafe { append_lit_general(a, bits, meta) }
}

/// [`__torajs_str_append_lit`] past its in-place shortcut: a null or
/// shared left side, a literal wider than the cell, a cell out of
/// room.
#[inline(never)]
unsafe fn append_lit_general(a: *mut u8, bits: u64, meta: u64) -> *mut u8 {
    if a.is_null() {
        return unsafe { join_lit_general(Side::NULL_TEXT, bits, meta, false) };
    }
    let Some(fit) = (unsafe { writable(a, meta) }) else {
        // copy, then release the reference this call was handed
        let r = unsafe { join_lit_general(Side::of_str(a), bits, meta, false) };
        unsafe { __torajs_str_drop(a) };
        return r;
    };
    let mut cell = a;
    if fit.need > unsafe { StrBlock::from_raw(a).payload_capacity() } {
        // move into a block with slack, so the appends after this one
        // land in place
        let kept = unsafe { (*(a as *const HeapHeader)).flags } & STR_FLAG_IS_ASCII;
        let mut grown =
            StrBlock::alloc_with_capacity(fit.len, fit.is_latin1, grow_capacity(fit.need));
        unsafe {
            grown.header().flags |= kept;
            copy_bytes(
                grown.0.as_ptr().add(STR_DATA_OFF),
                a.add(STR_DATA_OFF),
                fit.used as usize,
            );
            StrBlock::from_raw(a).free_pool_aware();
        }
        cell = grown.into_raw();
    }
    unsafe { put(cell, &fit, bits, meta) };
    cell
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::__torajs_str_free;
    use crate::concat_lit::test_support::*;
    use alloc::vec;
    use torajs_rc::{__torajs_rc_inc, FLAG_STATIC_LITERAL};

    #[test]
    fn a_null_accumulator_spells_null() {
        let (bits, meta) = lit1(b"-");
        let out = unsafe { __torajs_str_append_lit(core::ptr::null_mut(), bits, meta) };
        assert_eq!(read(out), (b"null-".to_vec(), true, 5));
        unsafe { __torajs_str_free(out) };
    }

    #[test]
    fn it_writes_into_the_slack_without_moving_the_cell() {
        let (bits, meta) = lit1(b"x");
        let mut acc = latin1(b"ab");
        let first = acc;
        for _ in 0..14 {
            acc = unsafe { __torajs_str_append_lit(acc, bits, meta) };
        }
        assert_eq!(acc, first, "sixteen bytes fit the smallest class");
        acc = unsafe { __torajs_str_append_lit(acc, bits, meta) };
        assert_ne!(acc, first, "the seventeenth does not");
        let (payload, is_latin1, len) = read(acc);
        assert_eq!(len, 17);
        assert!(is_latin1);
        assert_eq!(&payload[..2], b"ab");
        assert!(payload[2..].iter().all(|b| *b == b'x'));
        assert_eq!(unsafe { StrBlock::from_raw(acc).payload_capacity() }, 32);
        unsafe { __torajs_str_free(acc) };
    }

    #[test]
    fn a_utf16_capacity_cell_takes_a_utf16_literal_in_place() {
        let (bits, meta) = lit2(&[0x4E2D, 0x6587]);
        let acc = unsafe { crate::append_substr::__torajs_str_alloc_capacity_utf16(64) };
        let out = unsafe { __torajs_str_append_lit(acc, bits, meta) };
        assert_eq!(out, acc, "the room is kept");
        assert_eq!(read(out), (vec![0x2D, 0x4E, 0x87, 0x65], false, 2));
        unsafe { __torajs_str_free(out) };
    }

    #[test]
    fn it_grows_by_doubling_and_keeps_every_byte() {
        let (bits, meta) = lit1(b"abc");
        let mut acc = latin1(b"");
        for _ in 0..100 {
            acc = unsafe { __torajs_str_append_lit(acc, bits, meta) };
        }
        let (payload, _, len) = read(acc);
        assert_eq!(len, 300);
        assert!(payload.chunks(3).all(|c| c == b"abc"));
        assert_eq!(unsafe { StrBlock::from_raw(acc).payload_capacity() }, 512);
        unsafe { __torajs_str_free(acc) };
    }

    #[test]
    fn every_literal_length_lands_at_every_fill_of_the_cell() {
        let text = b"12345678";
        for start in 0..20 {
            for n in 1..=8 {
                let acc = latin1(&[b'q'; 20][..start]);
                let (bits, meta) = lit1(&text[..n]);
                let out = unsafe { __torajs_str_append_lit(acc, bits, meta) };
                let mut want = vec![b'q'; start];
                want.extend_from_slice(&text[..n]);
                assert_eq!(read(out).0, want, "start = {start}, n = {n}");
                unsafe { __torajs_str_free(out) };
            }
        }
    }

    #[test]
    fn a_latin1_literal_widens_onto_a_utf16_cell_in_place() {
        let (bits, meta) = lit1(b", ");
        let acc = utf16(&[0x4E2D]);
        let out = unsafe { __torajs_str_append_lit(acc, bits, meta) };
        assert_eq!(out, acc, "six bytes fit the cell it already has");
        assert_eq!(read(out), (vec![0x2D, 0x4E, b',', 0, b' ', 0], false, 3));
        unsafe { __torajs_str_free(out) };
    }

    #[test]
    fn a_utf16_literal_lands_on_a_utf16_cell_in_place() {
        let (bits, meta) = lit2(&[0x6587, 0x5B57]);
        let acc = utf16(&[0x4E2D]);
        let out = unsafe { __torajs_str_append_lit(acc, bits, meta) };
        assert_eq!(out, acc);
        assert_eq!(
            read(out),
            (vec![0x2D, 0x4E, 0x87, 0x65, 0x57, 0x5B], false, 3)
        );
        unsafe { __torajs_str_free(out) };
    }

    #[test]
    fn a_utf16_literal_on_a_latin1_cell_reallocates_wide() {
        let (bits, meta) = lit2(&[0x4E2D]);
        let acc = latin1(b"ab");
        let out = unsafe { __torajs_str_append_lit(acc, bits, meta) };
        assert_eq!(read(out), (vec![b'a', 0, b'b', 0, 0x2D, 0x4E], false, 3));
        unsafe { __torajs_str_free(out) };
    }

    #[test]
    fn a_shared_cell_is_copied_not_mutated() {
        let (bits, meta) = lit1(b"x");
        let acc = latin1(b"ab");
        unsafe { __torajs_rc_inc(acc as *mut core::ffi::c_void) };
        let out = unsafe { __torajs_str_append_lit(acc, bits, meta) };
        assert_ne!(out, acc);
        assert_eq!(read(acc).0, b"ab", "the other holder still sees `ab`");
        assert_eq!(read(out).0, b"abx");
        // the append consumed one of the two stakes
        assert_eq!(unsafe { &*(acc as *const HeapHeader) }.refcount, 1);
        unsafe { __torajs_str_free(acc) };
        unsafe { __torajs_str_free(out) };
    }

    #[test]
    fn a_rodata_literal_is_copied_not_mutated() {
        let (bits, meta) = lit1(b"x");
        let acc = latin1(b"ab");
        unsafe { (*(acc as *mut HeapHeader)).flags |= FLAG_STATIC_LITERAL };
        let out = unsafe { __torajs_str_append_lit(acc, bits, meta) };
        assert_ne!(out, acc);
        assert_eq!(read(acc).0, b"ab");
        assert_eq!(read(out).0, b"abx");
        unsafe { __torajs_str_free(out) };
        unsafe { (*(acc as *mut HeapHeader)).flags &= !FLAG_STATIC_LITERAL };
        unsafe { __torajs_str_free(acc) };
    }

    #[test]
    fn an_in_place_append_keeps_the_ascii_mark_only_for_an_ascii_literal() {
        let (bits, meta) = lit1(b"x");
        let acc = mark_ascii(latin1(b"ab"));
        let out = unsafe { __torajs_str_append_lit(acc, bits, meta) };
        assert_eq!(out, acc);
        assert!(is_marked_ascii(out));
        let (bits, meta) = lit1(&[0xE9]);
        let out = unsafe { __torajs_str_append_lit(out, bits, meta) };
        assert_eq!(out, acc);
        assert!(!is_marked_ascii(out));
        assert_eq!(read(out).0, [b'a', b'b', b'x', 0xE9]);
        unsafe { __torajs_str_free(out) };
    }

    #[test]
    fn a_grown_cell_keeps_the_ascii_mark() {
        let (bits, meta) = lit1(b"x");
        let mut acc = mark_ascii(latin1(b"0123456789abcdef"));
        acc = unsafe { __torajs_str_append_lit(acc, bits, meta) };
        assert_eq!(read(acc).2, 17);
        assert!(is_marked_ascii(acc));
        unsafe { __torajs_str_free(acc) };
    }
}
