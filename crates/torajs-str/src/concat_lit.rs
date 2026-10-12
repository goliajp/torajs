//! String `+` where one operand is a short literal handed over by
//! value.
//!
//! A literal whose payload fits in eight bytes — one to eight Latin-1
//! code units, or one to four UTF-16 ones — travels as two immediates
//! instead of a pointer to its `.rodata` cell:
//!
//! - `bits`: the payload bytes, first byte lowest;
//! - `meta`: the header flag bits the literal's cell would carry
//!   ([`STR_FLAG_IS_LATIN1`], [`STR_FLAG_IS_ASCII`]) at their header
//!   positions, and the code-unit count in the four bits at
//!   [`LIT_META_LEN_SHIFT`], which neither flag occupies.
//!
//! Everything the general kernels have to read off a right-hand cell
//! is then a constant of the call site: it cannot be null, its length
//! and width need no load, and its bytes land with one or two stores
//! rather than a copy. The other operand keeps every runtime rule of
//! [`crate::concat`]: a null pointer is the JS `null` value and reads
//! as the text "null", and the result takes the wider of the two
//! encodings.
//!
//! The borrowing forms live here; the consuming one is
//! [`crate::append_lit`]. The compiler picks them for a
//! `StaticStrRef` operand of that size (`torajs-egraph`'s
//! `str_append::literal`); nothing else calls them. Like
//! [`crate::join`], the short same-width case is straight-line code
//! that ends in a pooled block, and the rest is one tail call away.

use crate::block::StrBlock;
use crate::concat_num::stack_cell;
use crate::copy::copy_short;
use crate::join::{Side, write_side};
use crate::layout::{STR_DATA_OFF, STR_FLAG_IS_ASCII, STR_FLAG_IS_LATIN1, STR_POOL_PAYLOAD};
pub use crate::lit_meta::{LIT_META_LEN_MASK, LIT_META_LEN_SHIFT};
pub(crate) use crate::lit_meta::{lit_flags, lit_len, write_low_bytes, write_widened};
use crate::max_len::{raise_too_long, str_len_of};

/// Fresh cell holding `side` and the literal, in the order `lit_first`
/// says. `shared` says `side` is a whole Str heap block
/// ([`Side::of_str`]), which a result past the longest Str answers
/// with one more reference instead of a copy.
///
/// # Safety
///
/// `side` is readable for its code units.
#[inline(always)]
unsafe fn join_lit(side: Side, bits: u64, meta: u64, lit_first: bool, shared: bool) -> *mut u8 {
    let lit_flags = lit_flags(meta);
    // a length within the largest pooled payload: neither the sum
    // below nor its byte count can wrap
    if (side.flags ^ lit_flags) & STR_FLAG_IS_LATIN1 == 0 && side.len <= STR_POOL_PAYLOAD {
        let shift = u32::from(!side.is_latin1());
        let n = lit_len(meta);
        let side_bytes = side.len << shift;
        let lit_bytes = n << shift;
        // the width bit is the same on both sides; the ASCII mark
        // needs both
        let flags = side.flags & lit_flags & (STR_FLAG_IS_LATIN1 | STR_FLAG_IS_ASCII);
        if let Some(block) = StrBlock::pop_pooled(side.len + n, side_bytes + lit_bytes, flags) {
            let (side_bytes, lit_bytes) = (side_bytes as usize, lit_bytes as usize);
            unsafe {
                let dst = block.0.as_ptr().add(STR_DATA_OFF);
                if lit_first {
                    write_low_bytes(dst, bits, lit_bytes);
                    copy_short(dst.add(lit_bytes), side.ptr, side_bytes);
                } else {
                    copy_short(dst, side.ptr, side_bytes);
                    write_low_bytes(dst.add(side_bytes), bits, lit_bytes);
                }
            }
            return block.into_raw();
        }
    }
    if !fits_with_lit(side, meta) {
        return unsafe {
            if lit_first {
                lit_first_too_long(bits, meta)
            } else if shared {
                crate::concat::too_long_sharing(side.ptr)
            } else {
                crate::join::too_long_keeping(side.ptr, side.len, side.flags)
            }
        };
    }
    unsafe { join_lit_general(side, bits, meta, lit_first) }
}

/// Raise the RangeError for `<literal> + side` past the longest Str
/// and answer a fresh cell holding the literal, its left operand.
///
/// # Safety
///
/// `bits` / `meta` describe a literal as the module doc lays out.
#[cold]
#[inline(never)]
unsafe fn lit_first_too_long(bits: u64, meta: u64) -> *mut u8 {
    unsafe {
        raise_too_long();
        join_lit_general(Side::EMPTY, bits, meta, true)
    }
}

/// Whether `side` with the literal `meta` describes fits the longest
/// Str. The side is at most [`crate::max_len::STR_MAX_LEN`], so the
/// u32 sum cannot wrap.
#[inline(always)]
pub(crate) fn fits_with_lit(side: Side, meta: u64) -> bool {
    str_len_of((side.len + lit_len(meta)) as usize).is_some()
}

/// [`join_lit`] without the short-case shortcut: either width on
/// either side, any length, allocating when the pool has nothing.
///
/// # Safety
///
/// Same contract as [`join_lit`], and the side with the literal
/// [`fits_with_lit`]: the callers ask.
#[inline(never)]
pub(crate) unsafe extern "C" fn join_lit_general(
    side: Side,
    bits: u64,
    meta: u64,
    lit_first: bool,
) -> *mut u8 {
    let lit_flags = lit_flags(meta);
    let n = lit_len(meta);
    let lit_is_latin1 = lit_flags & STR_FLAG_IS_LATIN1 != 0;
    let out_latin1 = side.fits_latin1() && lit_is_latin1;
    // only a Latin-1 cell ever carries the ASCII mark
    let ascii = side.ascii_mark() & lit_flags;
    let block = StrBlock::alloc_with_flags(side.len + n, out_latin1, ascii);
    let shift = u32::from(!out_latin1);
    let lit_bytes = (n << shift) as usize;
    let (side_at, lit_at) = if lit_first {
        (lit_bytes, 0)
    } else {
        (0, (side.len as usize) << shift)
    };
    unsafe {
        let dst = block.0.as_ptr().add(STR_DATA_OFF);
        write_side(dst.add(side_at), side, out_latin1);
        if lit_is_latin1 == out_latin1 {
            write_low_bytes(dst.add(lit_at), bits, lit_bytes);
        } else {
            write_widened(dst.add(lit_at), bits, n as usize);
        }
    }
    block.into_raw()
}

/// `a + <literal>` — fresh refcount-1 Str; `a` is borrowed.
///
/// # Safety
///
/// `a` is null or a valid Str heap block; `bits` / `meta` describe a
/// literal as the module doc lays out.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_concat_lit(a: *const u8, bits: u64, meta: u64) -> *mut u8 {
    if a.is_null() {
        return unsafe { join_lit_general(Side::NULL_TEXT, bits, meta, false) };
    }
    unsafe { join_lit(Side::of_str(a), bits, meta, false, true) }
}

/// `<literal> + b` — fresh refcount-1 Str; `b` is borrowed.
///
/// # Safety
///
/// Same contract as [`__torajs_str_concat_lit`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_prepend_lit(b: *const u8, bits: u64, meta: u64) -> *mut u8 {
    if b.is_null() {
        return unsafe { join_lit_general(Side::NULL_TEXT, bits, meta, true) };
    }
    unsafe { join_lit(Side::of_str(b), bits, meta, true, true) }
}

/// `view + <literal>` — fresh refcount-1 Str; the view is borrowed.
///
/// # Safety
///
/// `v` is a valid Substr whose parent is live; `bits` / `meta`
/// describe a literal as the module doc lays out.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_substr_concat_lit(v: *const u8, bits: u64, meta: u64) -> *mut u8 {
    unsafe { join_lit(Side::of_view(v), bits, meta, false, false) }
}

/// `<literal> + view` — fresh refcount-1 Str; the view is borrowed.
///
/// # Safety
///
/// Same contract as [`__torajs_substr_concat_lit`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_substr_prepend_lit(
    v: *const u8,
    bits: u64,
    meta: u64,
) -> *mut u8 {
    unsafe { join_lit(Side::of_view(v), bits, meta, true, false) }
}

/// `view + <literal>` written into the caller's frame cell `buf`
/// when the result is a pooled-size payload of one width; answers
/// `buf`, whose static header makes every release of it a no-op.
/// Any other result is the fresh refcount-1 Str
/// [`__torajs_substr_concat_lit`] answers, which the caller releases
/// as before.
///
/// # Safety
///
/// Same contract as [`__torajs_substr_concat_lit`]; `buf` is 8-aligned
/// with room for the header and [`STR_POOL_PAYLOAD`] bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_substr_concat_lit_into(
    v: *const u8,
    bits: u64,
    meta: u64,
    buf: *mut u8,
) -> *mut u8 {
    let side = unsafe { Side::of_view(v) };
    let lit_flags = lit_flags(meta);
    // a length within the payload first, so the byte sum cannot wrap
    if (side.flags ^ lit_flags) & STR_FLAG_IS_LATIN1 == 0 && side.len <= STR_POOL_PAYLOAD {
        let shift = u32::from(!side.is_latin1());
        let n = lit_len(meta);
        let side_bytes = side.len << shift;
        let lit_bytes = n << shift;
        if side_bytes + lit_bytes <= STR_POOL_PAYLOAD {
            let ascii = side.flags & lit_flags & STR_FLAG_IS_ASCII;
            let (side_bytes, lit_bytes) = (side_bytes as usize, lit_bytes as usize);
            unsafe {
                stack_cell(buf, side.len + n, side.is_latin1(), ascii);
                let dst = buf.add(STR_DATA_OFF);
                copy_short(dst, side.ptr, side_bytes);
                write_low_bytes(dst.add(side_bytes), bits, lit_bytes);
            }
            return buf;
        }
    }
    unsafe { join_lit(side, bits, meta, false, false) }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use crate::layout::{STR_LEN_OFF, byte_capacity};
    use alloc::vec::Vec;
    use torajs_rc::HeapHeader;

    pub fn latin1(payload: &[u8]) -> *mut u8 {
        let mut b = StrBlock::alloc_with_encoding(payload.len() as u32, true);
        unsafe { b.as_bytes_mut(payload.len() as u32) }.copy_from_slice(payload);
        b.into_raw()
    }

    pub fn utf16(units: &[u16]) -> *mut u8 {
        let length = units.len() as u32;
        let mut b = StrBlock::alloc_with_encoding(length, false);
        let dst = unsafe { b.as_bytes_mut(length * 2) };
        for (i, &u) in units.iter().enumerate() {
            dst[i * 2..i * 2 + 2].copy_from_slice(&u.to_le_bytes());
        }
        b.into_raw()
    }

    /// `(payload bytes, is_latin1, code units)` of a Str cell.
    pub fn read(p: *const u8) -> (Vec<u8>, bool, u32) {
        let flags = unsafe { (*(p as *const HeapHeader)).flags };
        let len = unsafe { (p.add(STR_LEN_OFF) as *const u32).read() };
        let is_latin1 = flags & STR_FLAG_IS_LATIN1 != 0;
        let bytes = byte_capacity(len, is_latin1) as usize;
        let payload = unsafe { core::slice::from_raw_parts(p.add(STR_DATA_OFF), bytes) }.to_vec();
        (payload, is_latin1, len)
    }

    pub fn mark_ascii(p: *mut u8) -> *mut u8 {
        unsafe { (*(p as *mut HeapHeader)).flags |= STR_FLAG_IS_ASCII };
        p
    }

    pub fn is_marked_ascii(p: *const u8) -> bool {
        unsafe { (*(p as *const HeapHeader)).flags & STR_FLAG_IS_ASCII != 0 }
    }

    /// Take every block the pool holds; the caller frees them.
    pub fn drain_pool() -> Vec<*mut u8> {
        use crate::layout::{STR_POOL_PAYLOADS, STR_POOL_SLOTS};
        let mut taken = Vec::new();
        for payload in STR_POOL_PAYLOADS {
            for _ in 0..STR_POOL_SLOTS {
                taken.push(StrBlock::alloc_with_encoding(payload, true).into_raw());
            }
        }
        taken
    }

    /// `(bits, meta)` of a Latin-1 literal.
    pub fn lit1(payload: &[u8]) -> (u64, u64) {
        let mut raw = [0u8; 8];
        raw[..payload.len()].copy_from_slice(payload);
        let mut flags = STR_FLAG_IS_LATIN1;
        if payload.iter().all(|b| *b < 0x80) {
            flags |= STR_FLAG_IS_ASCII;
        }
        (
            u64::from_le_bytes(raw),
            (payload.len() as u64) << LIT_META_LEN_SHIFT | flags as u64,
        )
    }

    /// `(bits, meta)` of a UTF-16 literal.
    pub fn lit2(units: &[u16]) -> (u64, u64) {
        let mut raw = [0u8; 8];
        for (i, u) in units.iter().enumerate() {
            raw[i * 2..i * 2 + 2].copy_from_slice(&u.to_le_bytes());
        }
        (
            u64::from_le_bytes(raw),
            (units.len() as u64) << LIT_META_LEN_SHIFT,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;
    use crate::block::__torajs_str_free;
    use alloc::vec;
    use alloc::vec::Vec;
    use torajs_rc::FLAG_STATIC_LITERAL;

    /// Run `body` twice. First it releases each cell as soon as it is
    /// done with it, so the next allocation pops that block; then the
    /// pool is drained and every cell is held to the end, so every
    /// allocation is a fresh one.
    fn on_both_paths(body: impl Fn(&mut dyn FnMut(*mut u8))) {
        body(&mut |p| unsafe { __torajs_str_free(p) });
        let mut held = drain_pool();
        body(&mut |p| held.push(p));
        for p in held {
            unsafe { __torajs_str_free(p) };
        }
    }

    #[test]
    fn every_latin1_literal_length_lands_byte_exact() {
        let text = b"12345678";
        on_both_paths(|release| {
            for n in 1..=8 {
                let a = latin1(b"ab");
                let (bits, meta) = lit1(&text[..n]);
                let r = unsafe { __torajs_str_concat_lit(a, bits, meta) };
                let mut want = b"ab".to_vec();
                want.extend_from_slice(&text[..n]);
                assert_eq!(read(r), (want, true, 2 + n as u32), "n = {n}");
                let l = unsafe { __torajs_str_prepend_lit(a, bits, meta) };
                let mut want = text[..n].to_vec();
                want.extend_from_slice(b"ab");
                assert_eq!(read(l), (want, true, 2 + n as u32), "n = {n}");
                for p in [a, r, l] {
                    release(p);
                }
            }
        });
    }

    #[test]
    fn an_empty_utf16_side_takes_the_latin1_literal_width() {
        let empty = utf16(&[]);
        let (bits, meta) = lit1(b"b");
        let r = unsafe { __torajs_str_concat_lit(empty, bits, meta) };
        let l = unsafe { __torajs_str_prepend_lit(empty, bits, meta) };
        for p in [r, l] {
            assert_eq!(read(p), (b"b".to_vec(), true, 1));
            unsafe { __torajs_str_free(p) };
        }
        unsafe { __torajs_str_free(empty) };
    }

    #[test]
    fn every_utf16_literal_length_lands_byte_exact() {
        let units = [0x4E2D, 0x6587, 0x6D4B, 0x8BD5];
        on_both_paths(|release| {
            for n in 1..=4 {
                let a = utf16(&[0x3042]);
                let (bits, meta) = lit2(&units[..n]);
                let r = unsafe { __torajs_str_concat_lit(a, bits, meta) };
                let mut want = vec![0x42, 0x30];
                for u in &units[..n] {
                    want.extend_from_slice(&u.to_le_bytes());
                }
                assert_eq!(read(r), (want, false, 1 + n as u32), "n = {n}");
                release(a);
                release(r);
            }
        });
    }

    #[test]
    fn every_operand_length_through_the_pool_classes_lands_byte_exact() {
        let text: Vec<u8> = (0..80u8).map(|i| b'a' + i % 26).collect();
        let (bits, meta) = lit1(b", ");
        on_both_paths(|release| {
            for len in 0..text.len() {
                let a = latin1(&text[..len]);
                let r = unsafe { __torajs_str_concat_lit(a, bits, meta) };
                let mut want = text[..len].to_vec();
                want.extend_from_slice(b", ");
                assert_eq!(read(r).0, want, "len = {len}");
                let l = unsafe { __torajs_str_prepend_lit(a, bits, meta) };
                let mut want = b", ".to_vec();
                want.extend_from_slice(&text[..len]);
                assert_eq!(read(l).0, want, "len = {len}");
                for p in [a, r, l] {
                    release(p);
                }
            }
        });
    }

    #[test]
    fn a_null_operand_spells_null_on_either_side() {
        let (bits, meta) = lit1(b"-");
        let r = unsafe { __torajs_str_concat_lit(core::ptr::null(), bits, meta) };
        let l = unsafe { __torajs_str_prepend_lit(core::ptr::null(), bits, meta) };
        assert_eq!(read(r), (b"null-".to_vec(), true, 5));
        assert_eq!(read(l), (b"-null".to_vec(), true, 5));
        let (bits, meta) = lit2(&[0x4E2D]);
        let w = unsafe { __torajs_str_concat_lit(core::ptr::null(), bits, meta) };
        assert_eq!(
            read(w),
            (
                vec![b'n', 0, b'u', 0, b'l', 0, b'l', 0, 0x2D, 0x4E],
                false,
                5
            )
        );
        for p in [r, l, w] {
            unsafe { __torajs_str_free(p) };
        }
    }

    #[test]
    fn a_latin1_literal_widens_onto_a_utf16_operand() {
        let a = utf16(&[0x4E2D]);
        let (bits, meta) = lit1(b", ");
        let r = unsafe { __torajs_str_concat_lit(a, bits, meta) };
        assert_eq!(
            read(r),
            (vec![0x2D, 0x4E, b',', 0, b' ', 0], false, 3),
            "literal last"
        );
        let l = unsafe { __torajs_str_prepend_lit(a, bits, meta) };
        assert_eq!(
            read(l),
            (vec![b',', 0, b' ', 0, 0x2D, 0x4E], false, 3),
            "literal first"
        );
        for p in [a, r, l] {
            unsafe { __torajs_str_free(p) };
        }
    }

    #[test]
    fn a_utf16_literal_widens_a_latin1_operand() {
        let a = latin1(b"ab");
        let (bits, meta) = lit2(&[0x4E2D]);
        let r = unsafe { __torajs_str_concat_lit(a, bits, meta) };
        assert_eq!(
            read(r),
            (vec![b'a', 0, b'b', 0, 0x2D, 0x4E], false, 3),
            "literal last"
        );
        let l = unsafe { __torajs_str_prepend_lit(a, bits, meta) };
        assert_eq!(
            read(l),
            (vec![0x2D, 0x4E, b'a', 0, b'b', 0], false, 3),
            "literal first"
        );
        for p in [a, r, l] {
            unsafe { __torajs_str_free(p) };
        }
    }

    #[test]
    fn the_ascii_mark_needs_both_sides() {
        let (ascii_bits, ascii_meta) = lit1(b"x");
        let (high_bits, high_meta) = lit1(&[0xE9]);
        on_both_paths(|release| {
            let marked = mark_ascii(latin1(b"ab"));
            let unknown = latin1(b"ab");
            let both = unsafe { __torajs_str_concat_lit(marked, ascii_bits, ascii_meta) };
            let high = unsafe { __torajs_str_concat_lit(marked, high_bits, high_meta) };
            let one = unsafe { __torajs_str_concat_lit(unknown, ascii_bits, ascii_meta) };
            assert!(is_marked_ascii(both));
            assert!(!is_marked_ascii(high));
            assert!(!is_marked_ascii(one));
            assert_eq!(read(high).0, [b'a', b'b', 0xE9]);
            for p in [marked, unknown, both, high, one] {
                release(p);
            }
        });
    }

    /// A stack view over `parent`: `[header][len][parent][offset]`.
    fn view(parent: *mut u8, offset: u64, len: u64) -> [u64; 4] {
        [
            (FLAG_STATIC_LITERAL as u64) << 48,
            len,
            parent as u64,
            offset,
        ]
    }

    #[test]
    fn a_view_reads_its_window_of_the_parent() {
        let parent = latin1(b"alpha,beta");
        let v = view(parent, 6, 4);
        let (bits, meta) = lit1(b"|");
        let r = unsafe { __torajs_substr_concat_lit(v.as_ptr() as *const u8, bits, meta) };
        let l = unsafe { __torajs_substr_prepend_lit(v.as_ptr() as *const u8, bits, meta) };
        assert_eq!(read(r), (b"beta|".to_vec(), true, 5));
        assert_eq!(read(l), (b"|beta".to_vec(), true, 5));
        let wide = utf16(&[0x3042, 0x4E2D, 0x6587]);
        let v = view(wide, 1, 2);
        let r2 = unsafe { __torajs_substr_concat_lit(v.as_ptr() as *const u8, bits, meta) };
        assert_eq!(
            read(r2),
            (vec![0x2D, 0x4E, 0x87, 0x65, b'|', 0], false, 3),
            "offset and length are code units"
        );
        for p in [parent, wide, r, l, r2] {
            unsafe { __torajs_str_free(p) };
        }
    }

    /// 8-aligned room for a frame cell: the header and the largest
    /// pooled payload.
    #[repr(align(8))]
    struct Frame([u8; STR_DATA_OFF + STR_POOL_PAYLOAD as usize]);

    #[test]
    fn a_pooled_size_view_and_literal_land_in_the_frame() {
        let parent = latin1(b"alpha,beta");
        let v = view(parent, 6, 4);
        let (bits, meta) = lit1(b"|");
        let mut frame = Frame([0xAA; STR_DATA_OFF + STR_POOL_PAYLOAD as usize]);
        let buf = frame.0.as_mut_ptr();
        let r =
            unsafe { __torajs_substr_concat_lit_into(v.as_ptr() as *const u8, bits, meta, buf) };
        assert_eq!(r, buf);
        assert_eq!(read(r), (b"beta|".to_vec(), true, 5));
        let flags = unsafe { (*(r as *const torajs_rc::HeapHeader)).flags };
        assert_ne!(flags & FLAG_STATIC_LITERAL, 0);
        assert!(!is_marked_ascii(r), "the parent carries no ASCII mark");
        unsafe { crate::str_drop::__torajs_str_drop(r) };
        assert_eq!(read(r).0, b"beta|", "a release passes over the frame cell");
        let text = [b'q'; STR_POOL_PAYLOAD as usize];
        let full = mark_ascii(latin1(&text));
        let v = view(full, 1, 63);
        let r =
            unsafe { __torajs_substr_concat_lit_into(v.as_ptr() as *const u8, bits, meta, buf) };
        assert_eq!(r, buf, "exactly the payload bound");
        let mut want = text[1..].to_vec();
        want.push(b'|');
        assert_eq!(read(r), (want, true, 64));
        assert!(is_marked_ascii(r));
        let wide = utf16(&[0x3042, 0x4E2D, 0x6587]);
        let v = view(wide, 1, 2);
        let (wbits, wmeta) = lit2(&[0x7C]);
        let r =
            unsafe { __torajs_substr_concat_lit_into(v.as_ptr() as *const u8, wbits, wmeta, buf) };
        assert_eq!(r, buf);
        assert_eq!(read(r), (vec![0x2D, 0x4E, 0x87, 0x65, b'|', 0], false, 3));
        for p in [parent, full, wide] {
            unsafe { __torajs_str_free(p) };
        }
    }

    #[test]
    fn a_long_or_mixed_width_view_falls_back_to_a_heap_cell() {
        let mut frame = Frame([0; STR_DATA_OFF + STR_POOL_PAYLOAD as usize]);
        let buf = frame.0.as_mut_ptr();
        let (bits, meta) = lit1(b"|");
        let text = [b'q'; 80];
        let long = latin1(&text);
        let v = view(long, 0, 64);
        let r =
            unsafe { __torajs_substr_concat_lit_into(v.as_ptr() as *const u8, bits, meta, buf) };
        assert_ne!(r, buf, "65 bytes are past the payload");
        let mut want = text[..64].to_vec();
        want.push(b'|');
        assert_eq!(read(r), (want, true, 65));
        let wide = utf16(&[0x3042, 0x4E2D]);
        let v = view(wide, 0, 2);
        let w =
            unsafe { __torajs_substr_concat_lit_into(v.as_ptr() as *const u8, bits, meta, buf) };
        assert_ne!(
            w, buf,
            "a Latin-1 literal on a UTF-16 view widens on the heap"
        );
        assert_eq!(read(w), (vec![0x42, 0x30, 0x2D, 0x4E, b'|', 0], false, 3));
        for p in [long, wide, r, w] {
            unsafe { __torajs_str_free(p) };
        }
    }

    #[test]
    fn the_frame_cell_size_is_the_one_the_compiler_reserves() {
        // torajs-egraph's concat_num_fuse::stack mirrors this as its
        // view-literal slot size
        assert_eq!(STR_DATA_OFF + STR_POOL_PAYLOAD as usize, 80);
    }
}
