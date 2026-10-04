//! `__torajs_str_append_substr` — `a ++ view` where the caller hands
//! over its reference to `a` and the right side is a Substr view —
//! and `__torajs_str_alloc_capacity`, the empty cell a builder loop
//! starts from.
//!
//! The in-place rule is [`crate::append`]'s: a cell nobody else holds
//! takes the bytes into its slack, everything else goes through
//! `concat`. The right side's bytes are read off the view's parent, so
//! a view of `a` itself is the aliasing case `append` guards against
//! by pointer, and is guarded the same way here through the parent.

use torajs_rc::HeapHeader;

use crate::append::{append_in_place, append_payload, is_solely_ours};
use crate::block::StrBlock;
use crate::join::Side;
use crate::layout::{STR_FLAG_IS_ASCII, grow_capacity};
use crate::max_len::str_max_bytes;
use crate::str_drop::__torajs_str_drop;
use crate::substr_concat::__torajs_substr_concat_str_substr;
use crate::substr_methods::{substr_parent, substr_view};

/// `a + v` for a Str `a` whose reference the caller hands over and a
/// borrowed Substr `v`. Answers an owned refcount-1 Str; the answer
/// aliases `a` when the append happened in place.
///
/// # Safety
///
/// `a` is a valid Str heap block the caller owns one reference to
/// (released here); `v` is a valid Substr whose parent is live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_append_substr(a: *mut u8, v: *const u8) -> *mut u8 {
    // a view of `a` itself reads the payload the append would write;
    // the general path owns that case
    if !core::ptr::eq(a as *const u8, unsafe { substr_parent(v) })
        && unsafe { append_in_place(a, Side::of_view(v)) }
    {
        return a;
    }
    unsafe { append_substr_general(a, v) }
}

/// [`__torajs_str_append_substr`] past its in-place shortcut.
#[inline(never)]
unsafe fn append_substr_general(a: *mut u8, v: *const u8) -> *mut u8 {
    // SAFETY: caller's contract is a valid Str block at `a` and a
    // valid view at `v`.
    let header = unsafe { &*(a as *const HeapHeader) };
    let parent = unsafe { substr_parent(v) };
    if core::ptr::eq(a as *const u8, parent) || !unsafe { is_solely_ours(header) } {
        let r = unsafe { __torajs_substr_concat_str_substr(a, v) } as *mut u8;
        unsafe { __torajs_str_drop(a) };
        return r;
    }
    let (v_payload, v_len, v_is_latin1) = unsafe { substr_view(v) };
    let parent_flags = unsafe { &*(parent as *const HeapHeader) }.flags;
    unsafe {
        append_payload(
            a,
            v_payload,
            v_len as u32,
            v_is_latin1,
            parent_flags & STR_FLAG_IS_ASCII != 0,
        )
    }
}

/// An empty Latin-1 Str owning at least `cap` payload bytes of slack,
/// marked ASCII: the cell a loop of appends grows into, so the first
/// appends land in place instead of reallocating.
///
/// # Safety
///
/// The answer is an owned refcount-1 Str the caller releases.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_alloc_capacity(cap: u64) -> *mut u8 {
    let cap = reserve(cap, true);
    let mut block = StrBlock::alloc_with_capacity(0, true, cap);
    // SAFETY: freshly allocated; the header is ours to mark.
    unsafe { block.header().flags |= STR_FLAG_IS_ASCII };
    block.into_raw()
}

/// [`__torajs_str_alloc_capacity`] for a cell every append writes
/// UTF-16 onto: an empty UTF-16 Str, so the first append does not
/// reallocate to widen nothing. Not marked ASCII.
///
/// # Safety
///
/// The answer is an owned refcount-1 Str the caller releases.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_alloc_capacity_utf16(cap: u64) -> *mut u8 {
    let cap = reserve(cap, false);
    StrBlock::alloc_with_capacity(0, false, cap).into_raw()
}

/// The slack for a hint of `cap` bytes: grown like an append's, and no
/// more than a Str in that encoding holds.
fn reserve(cap: u64, is_latin1: bool) -> u32 {
    // a hint past what a Str holds takes all it holds
    grow_capacity(cap, is_latin1).unwrap_or(str_max_bytes(is_latin1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::__torajs_str_free;
    use crate::layout::{STR_DATA_OFF, STR_FLAG_IS_LATIN1, STR_LEN_OFF, byte_capacity};
    use alloc::vec;
    use alloc::vec::Vec;
    use torajs_rc::FLAG_STATIC_LITERAL;

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

    /// A stack view with the static header the iterator lanes write:
    /// `[header][len][parent][offset]`.
    fn view(parent: *mut u8, offset: u64, len: u64) -> [u64; 4] {
        [
            (FLAG_STATIC_LITERAL as u64) << 48,
            len,
            parent as u64,
            offset,
        ]
    }

    #[test]
    fn a_view_lands_in_the_slack_in_place() {
        let parent = latin1(b"hello world");
        let v = view(parent, 6, 5);
        let acc = latin1(b"x");
        let out = unsafe { __torajs_str_append_substr(acc, v.as_ptr() as *const u8) };
        assert_eq!(out, acc, "the append happened in place");
        assert_eq!(read(out).0, b"xworld");
        unsafe { __torajs_str_free(out) };
        unsafe { __torajs_str_free(parent) };
    }

    #[test]
    fn a_view_of_the_accumulator_itself_copies() {
        let acc = latin1(b"abc");
        let v = view(acc, 1, 2);
        let out = unsafe { __torajs_str_append_substr(acc, v.as_ptr() as *const u8) };
        assert_ne!(out, acc);
        assert_eq!(read(out).0, b"abcbc");
        unsafe { __torajs_str_free(out) };
    }

    #[test]
    fn a_utf16_parent_widens_the_accumulator() {
        let parent = utf16(&[0x61, 0x4E2D, 0x62]);
        let v = view(parent, 1, 2);
        let acc = latin1(b"x");
        let out = unsafe { __torajs_str_append_substr(acc, v.as_ptr() as *const u8) };
        let (payload, is_latin1, length) = read(out);
        assert!(!is_latin1);
        assert_eq!(length, 3);
        assert_eq!(payload, [0x78, 0x00, 0x2D, 0x4E, 0x62, 0x00]);
        unsafe { __torajs_str_free(out) };
        unsafe { __torajs_str_free(parent) };
    }

    #[test]
    fn an_unmarked_parent_clears_the_ascii_mark() {
        let parent = latin1(b"abc");
        let v = view(parent, 0, 3);
        let acc = unsafe { __torajs_str_alloc_capacity(8) };
        assert!(unsafe { &*(acc as *const HeapHeader) }.flags & STR_FLAG_IS_ASCII != 0);
        let out = unsafe { __torajs_str_append_substr(acc, v.as_ptr() as *const u8) };
        assert_eq!(out, acc);
        assert!(unsafe { &*(out as *const HeapHeader) }.flags & STR_FLAG_IS_ASCII == 0);
        assert_eq!(read(out).0, b"abc");
        unsafe { __torajs_str_free(out) };
        unsafe { __torajs_str_free(parent) };
    }

    #[test]
    fn a_utf16_capacity_cell_takes_a_utf16_view_in_place() {
        let parent = utf16(&[0x4E2D, 0x6587]);
        let v = view(parent, 0, 2);
        let acc = unsafe { __torajs_str_alloc_capacity_utf16(64) };
        assert_eq!(read(acc), (Vec::new(), false, 0));
        assert!(unsafe { &*(acc as *const HeapHeader) }.flags & STR_FLAG_IS_ASCII == 0);
        assert_eq!(unsafe { StrBlock::from_raw(acc).payload_capacity() }, 64);
        let out = unsafe { __torajs_str_append_substr(acc, v.as_ptr() as *const u8) };
        assert_eq!(out, acc, "the room is kept");
        assert_eq!(read(out), (vec![0x2D, 0x4E, 0x87, 0x65], false, 2));
        unsafe { __torajs_str_free(out) };
        unsafe { __torajs_str_free(parent) };
    }

    #[test]
    fn a_capacity_cell_is_empty_and_owns_the_room_asked_for() {
        let acc = unsafe { __torajs_str_alloc_capacity(100) };
        let (payload, is_latin1, length) = read(acc);
        assert!(payload.is_empty());
        assert!(is_latin1);
        assert_eq!(length, 0);
        let cap = unsafe { StrBlock::from_raw(acc).payload_capacity() };
        assert_eq!(cap, 128);
        unsafe { __torajs_str_free(acc) };
    }
}
