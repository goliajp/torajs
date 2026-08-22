//! `__torajs_str_append` — `a ++ b` where the caller hands over its
//! reference to `a`.
//!
//! `__torajs_str_concat` only borrows both operands, so it must mint
//! a fresh cell and copy the whole left side into it every time. In
//! the string-builder shape — `acc = acc + piece`, the single most
//! common way JS grows a string — the caller drops `acc` on the very
//! next instruction, so that copy is pure waste and the loop pays
//! O(n^2) bytes plus one malloc/free round-trip per round. The
//! `multibyte-concat` bench spends 62% of its samples in exactly
//! those three symbols (`memcpy` / `__torajs_malloc` / `__torajs_free`).
//!
//! This kernel takes ownership of `a` instead, which lets it ask the
//! question `concat` cannot: **is anyone else holding this cell?**
//!
//! - refcount 1, room to spare → write `b` into the slack and bump
//!   the length. No allocation, no copy of `a`.
//! - refcount 1, out of room → take a new cell at
//!   [`grow_capacity`] bytes, move both sides in, release `a`. The
//!   surplus means the next N appends take the branch above, so the
//!   reallocations along a growing string are logarithmic in its
//!   length rather than one per append.
//! - shared, or a `.rodata` literal, or a Substr view → fall back to
//!   `concat` verbatim and then release our stake, which is the
//!   sequence the caller used to emit inline.
//!
//! That is copy-on-write keyed on unique ownership — Swift's
//! `isKnownUniquelyReferenced` mutation gate, and the same reason
//! Rust's `String + &str` takes `self` by value. The ownership
//! transfer at the call site is what makes it legal, so the rewrite
//! from `concat` to `append` is a peephole on the emitted stream
//! (`torajs-egraph`'s `str_append` pass), not something `concat`
//! could ever decide for itself.

use torajs_rc::{FLAG_STATIC_LITERAL, HeapHeader};

use crate::block::StrBlock;
use crate::concat::__torajs_str_concat;
use crate::layout::{STR_DATA_OFF, STR_FLAG_IS_LATIN1, STR_LEN_OFF, byte_capacity, grow_capacity};
use crate::str_drop::__torajs_str_drop;
use crate::substr::{FLAG_SUBSTR_INLINE, FLAG_SUBSTR_VIEW};

/// Cells this kernel may rewrite in place: nobody else is looking,
/// the bytes are ours to move, and the header means what the Str
/// layout says it means.
///
/// The Substr bits matter because views share `Tag::Str` — reading
/// one through the Str layout is already wrong, and writing one
/// would corrupt a parent's payload. `__torajs_str_drop` dispatches
/// on the same two bits for the same reason.
#[inline]
unsafe fn is_solely_ours(header: &HeapHeader) -> bool {
    header.refcount == 1
        && header.flags & (FLAG_STATIC_LITERAL | FLAG_SUBSTR_VIEW | FLAG_SUBSTR_INLINE) == 0
}

/// Write `src` at `dst[..]`, widening Latin-1 to UTF-16 LE when the
/// destination encoding is wider than the source's.
#[inline]
fn write_side(src: &[u8], dst: &mut [u8], src_is_latin1: bool, dst_is_latin1: bool) {
    if src_is_latin1 == dst_is_latin1 {
        dst.copy_from_slice(src);
        return;
    }
    debug_assert!(src_is_latin1 && !dst_is_latin1);
    for (i, &b) in src.iter().enumerate() {
        dst[i * 2] = b;
        dst[i * 2 + 1] = 0;
    }
}

/// `a + b` for Str operands where **the caller's reference to `a` is
/// consumed**. Answers an owned refcount-1 Str; the answer aliases
/// `a` when the append happened in place.
///
/// # Safety
///
/// `a` and `b` must each be null or a valid Str heap block, and the
/// caller must own the reference to `a` it is passing (this fn
/// releases it). `b` is borrowed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_append(a: *mut u8, b: *const u8) -> *mut u8 {
    // A null operand is the `undefined` spelling (RC-4 F1b-2);
    // `concat` already owns that path. Cold either way.
    if a.is_null() || b.is_null() {
        let r = unsafe { __torajs_str_concat(a, b) };
        unsafe { __torajs_str_drop(a) };
        return r;
    }
    // SAFETY: caller's contract is a valid Str block at `a`.
    let header = unsafe { &*(a as *const HeapHeader) };
    // `s + s` hands the same cell in twice: appending it to itself
    // would read the source through a slice it is also writing, and
    // the grow path would read `b` out of the block it just freed.
    // The peephole cannot rule this out — two distinct SSA values
    // can carry one pointer — so the kernel does.
    if core::ptr::eq(a as *const u8, b) || !unsafe { is_solely_ours(header) } {
        let r = unsafe { __torajs_str_concat(a, b) };
        unsafe { __torajs_str_drop(a) };
        return r;
    }
    let a_len = unsafe { (a.add(STR_LEN_OFF) as *const u32).read() };
    let b_len = unsafe { (b.add(STR_LEN_OFF) as *const u32).read() };
    if b_len == 0 {
        return a;
    }
    let a_is_latin1 = header.flags & STR_FLAG_IS_LATIN1 != 0;
    // SAFETY: same contract for `b`.
    let b_header = unsafe { &*(b as *const HeapHeader) };
    let b_is_latin1 = b_header.flags & STR_FLAG_IS_LATIN1 != 0;
    let out_is_latin1 = a_is_latin1 && b_is_latin1;
    let total_len = a_len + b_len;
    let need = byte_capacity(total_len, out_is_latin1);
    let a_bytes = byte_capacity(a_len, a_is_latin1) as usize;
    let b_bytes = byte_capacity(b_len, b_is_latin1) as usize;
    // SAFETY: `b`'s payload is `b_bytes` long by its own length and
    // encoding fields.
    let b_payload = unsafe { core::slice::from_raw_parts(b.add(STR_DATA_OFF), b_bytes) };
    let mut block = unsafe { StrBlock::from_raw(a) };

    if a_is_latin1 == out_is_latin1 && unsafe { block.payload_capacity() } >= need {
        let b_out_bytes = byte_capacity(b_len, out_is_latin1) as usize;
        // SAFETY: the capacity check above says the block owns
        // `need` payload bytes, and `a_bytes + b_out_bytes == need`.
        let dst = unsafe { block.as_bytes_mut(need) };
        write_side(
            b_payload,
            &mut dst[a_bytes..a_bytes + b_out_bytes],
            b_is_latin1,
            out_is_latin1,
        );
        // SAFETY: refcount 1, plain Str cell, payload now written
        // through `total_len` code units.
        unsafe { block.set_length(total_len) };
        return a;
    }

    // Out of room (or `a` needs widening): take a cell with slack so
    // the appends after this one land in the branch above.
    let mut grown = StrBlock::alloc_with_capacity(total_len, out_is_latin1, grow_capacity(need));
    // SAFETY: freshly allocated with at least `need` payload bytes.
    let dst = unsafe { grown.as_bytes_mut(need) };
    let a_out_bytes = byte_capacity(a_len, out_is_latin1) as usize;
    if a_bytes > 0 {
        // SAFETY: `a`'s payload is `a_bytes` long by its own header.
        let a_payload = unsafe { core::slice::from_raw_parts(a.add(STR_DATA_OFF), a_bytes) };
        write_side(
            a_payload,
            &mut dst[..a_out_bytes],
            a_is_latin1,
            out_is_latin1,
        );
    }
    let b_out_bytes = byte_capacity(b_len, out_is_latin1) as usize;
    write_side(
        b_payload,
        &mut dst[a_out_bytes..a_out_bytes + b_out_bytes],
        b_is_latin1,
        out_is_latin1,
    );
    // The reference we were handed was the only one; release it.
    block.free_pool_aware();
    grown.into_raw()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::__torajs_str_free;
    use crate::layout::STR_POOL_PAYLOAD;
    use alloc::vec::Vec;
    use torajs_rc::__torajs_rc_inc;

    fn latin1(payload: &[u8]) -> *mut u8 {
        let mut b = StrBlock::alloc_with_encoding(payload.len() as u32, true);
        let dst = unsafe { b.as_bytes_mut(payload.len() as u32) };
        dst.copy_from_slice(payload);
        b.into_raw()
    }

    fn utf16(units: &[u16]) -> *mut u8 {
        let length = units.len() as u32;
        let mut b = StrBlock::alloc_with_encoding(length, false);
        let dst = unsafe { b.as_bytes_mut(length * 2) };
        for (i, &u) in units.iter().enumerate() {
            dst[i * 2..i * 2 + 2].copy_from_slice(&u.to_le_bytes());
        }
        b.into_raw()
    }

    fn read(p: *const u8) -> (Vec<u8>, bool, u32) {
        let length = unsafe { (p.add(STR_LEN_OFF) as *const u32).read() };
        let header = unsafe { &*(p as *const HeapHeader) };
        let is_latin1 = header.flags & STR_FLAG_IS_LATIN1 != 0;
        let bytes = byte_capacity(length, is_latin1) as usize;
        let payload = unsafe { core::slice::from_raw_parts(p.add(STR_DATA_OFF), bytes) }.to_vec();
        (payload, is_latin1, length)
    }

    #[test]
    fn appends_into_the_slack_without_moving_the_cell() {
        // "ab" takes the 16-byte class, so the first appends fit
        // without reallocating and the cell address must not move.
        let mut acc = latin1(b"ab");
        let first = acc;
        for _ in 0..7 {
            acc = unsafe { __torajs_str_append(acc, piece()) };
        }
        assert_eq!(acc, first, "stayed in the same cell");
        let (payload, is_latin1, length) = read(acc);
        assert_eq!(payload, b"abxxxxxxx");
        assert!(is_latin1);
        assert_eq!(length, 9);
        unsafe { __torajs_str_free(acc) };
    }

    /// One-byte Latin-1 `"x"` to append. Minted fresh per call and
    /// deliberately leaked — these tests assert on the accumulator,
    /// and threading a free through every loop would only be noise.
    fn piece() -> *mut u8 {
        latin1(b"x")
    }

    #[test]
    fn growing_past_the_capacity_reallocates_with_slack() {
        let mut acc = latin1(b"");
        for _ in 0..100 {
            acc = unsafe { __torajs_str_append(acc, piece()) };
        }
        let (payload, _, length) = read(acc);
        assert_eq!(length, 100);
        assert_eq!(payload, [b'x'; 100]);
        // 100 bytes rounds to 128, so the cell holds slack — that is
        // what makes the next appends free.
        let cap = unsafe { StrBlock::from_raw(acc).payload_capacity() };
        assert_eq!(cap, 128);
        unsafe { __torajs_str_free(acc) };
    }

    #[test]
    fn a_shared_cell_is_copied_not_mutated() {
        let acc = latin1(b"ab");
        // A second holder — an alias binding, an array slot, a
        // Substr view: all of them take a refcount.
        unsafe { __torajs_rc_inc(acc as *mut core::ffi::c_void) };
        let out = unsafe { __torajs_str_append(acc, piece()) };
        assert_ne!(out, acc, "shared cells must not be appended in place");
        assert_eq!(read(acc).0, b"ab", "the other holder still sees `ab`");
        assert_eq!(read(out).0, b"abx");
        // The append consumed one of the two stakes; the survivor is
        // the alias we minted above.
        assert_eq!(unsafe { &*(acc as *const HeapHeader) }.refcount, 1);
        unsafe { __torajs_str_free(acc) };
        unsafe { __torajs_str_free(out) };
    }

    #[test]
    fn appending_a_cell_to_itself_copies() {
        let acc = latin1(b"ab");
        let out = unsafe { __torajs_str_append(acc, acc) };
        assert_eq!(read(out).0, b"abab");
        unsafe { __torajs_str_free(out) };
    }

    #[test]
    fn a_utf16_right_side_widens_the_whole_result() {
        let acc = latin1(b"ab");
        let piece = utf16(&[0x4E2D]);
        let out = unsafe { __torajs_str_append(acc, piece) };
        let (payload, is_latin1, length) = read(out);
        assert!(!is_latin1);
        assert_eq!(length, 3);
        assert_eq!(payload, [0x61, 0x00, 0x62, 0x00, 0x2D, 0x4E]);
        unsafe { __torajs_str_free(piece) };
        unsafe { __torajs_str_free(out) };
    }

    #[test]
    fn a_latin1_right_side_widens_into_a_utf16_accumulator() {
        let mut acc = utf16(&[0x4E2D]);
        acc = unsafe { __torajs_str_append(acc, piece()) };
        let (payload, is_latin1, length) = read(acc);
        assert!(!is_latin1);
        assert_eq!(length, 2);
        assert_eq!(payload, [0x2D, 0x4E, 0x78, 0x00]);
        unsafe { __torajs_str_free(acc) };
    }

    #[test]
    fn an_empty_right_side_answers_the_accumulator_untouched() {
        let acc = latin1(b"ab");
        let empty = latin1(b"");
        let out = unsafe { __torajs_str_append(acc, empty) };
        assert_eq!(out, acc);
        assert_eq!(read(out).0, b"ab");
        unsafe { __torajs_str_free(empty) };
        unsafe { __torajs_str_free(acc) };
    }

    #[test]
    fn a_rodata_literal_is_copied_not_mutated() {
        let acc = latin1(b"ab");
        unsafe { (&mut *(acc as *mut HeapHeader)).flags |= FLAG_STATIC_LITERAL };
        let out = unsafe { __torajs_str_append(acc, piece()) };
        assert_ne!(out, acc);
        assert_eq!(read(acc).0, b"ab");
        assert_eq!(read(out).0, b"abx");
        unsafe { __torajs_str_free(out) };
        // `acc` still carries the literal flag; free it the way the
        // block tests do, by hand at its alloc-time size.
        unsafe { (&mut *(acc as *mut HeapHeader)).flags &= !FLAG_STATIC_LITERAL };
        unsafe { __torajs_str_free(acc) };
    }

    #[test]
    fn a_null_operand_still_spells_undefined() {
        let acc = latin1(b"ab");
        let out = unsafe { __torajs_str_append(acc, core::ptr::null()) };
        assert_eq!(read(out).0, b"abundefined");
        unsafe { __torajs_str_free(out) };
    }

    #[test]
    fn a_grown_cell_frees_at_the_size_it_was_taken_at() {
        // The capacity slot is what tells `free` how big the block
        // is; a grown cell past the pool cutoff must not be handed
        // back at its length-derived size.
        let mut acc = latin1(b"");
        for _ in 0..(STR_POOL_PAYLOAD as usize + 5) {
            acc = unsafe { __torajs_str_append(acc, piece()) };
        }
        let cap = unsafe { StrBlock::from_raw(acc).payload_capacity() };
        assert_eq!(cap, 128, "69 bytes rounds to 128");
        assert!(cap > STR_POOL_PAYLOAD, "past the pool cutoff");
        unsafe { __torajs_str_free(acc) };
    }
}
