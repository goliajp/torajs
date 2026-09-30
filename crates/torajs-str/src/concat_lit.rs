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
use crate::copy::copy_short;
use crate::join::{Side, write_side};
use crate::layout::{STR_DATA_OFF, STR_FLAG_IS_ASCII, STR_FLAG_IS_LATIN1, STR_POOL_PAYLOAD};

// `bits` is written with plain integer stores
const _: () = assert!(cfg!(target_endian = "little"));

/// Bit position of the literal's code-unit count inside `meta`.
pub const LIT_META_LEN_SHIFT: u32 = 4;

const _: () = assert!((STR_FLAG_IS_LATIN1 | STR_FLAG_IS_ASCII) & (0xF << LIT_META_LEN_SHIFT) == 0);

#[inline(always)]
pub(crate) fn lit_len(meta: u64) -> u32 {
    (meta >> LIT_META_LEN_SHIFT) as u32 & 0xF
}

/// The literal's flag bits; the length bits ride along, and every
/// reader masks with the flag it asks about.
#[inline(always)]
pub(crate) fn lit_flags(meta: u64) -> u16 {
    meta as u16
}

/// Store the low `n` bytes of `bits` at `dst`, `1 <= n <= 8`.
///
/// # Safety
///
/// `dst` is writable for `n` bytes.
#[inline(always)]
pub(crate) unsafe fn write_low_bytes(dst: *mut u8, bits: u64, n: usize) {
    unsafe {
        if n == 1 {
            dst.write(bits as u8);
        } else if n == 8 {
            (dst as *mut u64).write_unaligned(bits);
        } else if n >= 4 {
            // two overlapping 4-byte stores cover 4..=7
            (dst as *mut u32).write_unaligned(bits as u32);
            (dst.add(n - 4) as *mut u32).write_unaligned((bits >> ((n - 4) * 8)) as u32);
        } else {
            (dst as *mut u16).write_unaligned(bits as u16);
            (dst.add(n - 2) as *mut u16).write_unaligned((bits >> ((n - 2) * 8)) as u16);
        }
    }
}

/// Store `n` Latin-1 code units held in `bits` at `dst` as UTF-16 LE.
///
/// # Safety
///
/// `dst` is writable for `2 * n` bytes.
#[inline(always)]
pub(crate) unsafe fn write_widened(dst: *mut u8, bits: u64, n: usize) {
    for i in 0..n {
        let unit = (bits >> (i * 8)) as u8 as u16;
        unsafe { (dst.add(i * 2) as *mut u16).write_unaligned(unit) };
    }
}

/// Fresh cell holding `side` and the literal, in the order `lit_first`
/// says.
///
/// # Safety
///
/// `side` is readable for its code units.
#[inline(always)]
unsafe fn join_lit(side: Side, bits: u64, meta: u64, lit_first: bool) -> *mut u8 {
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
    unsafe { join_lit_general(side, bits, meta, lit_first) }
}

/// [`join_lit`] without the short-case shortcut: either width on
/// either side, any length, allocating when the pool has nothing.
///
/// # Safety
///
/// Same contract as [`join_lit`].
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
    let out_latin1 = side.is_latin1() && lit_is_latin1;
    // only a Latin-1 cell ever carries the ASCII mark
    let ascii = side.flags & lit_flags & STR_FLAG_IS_ASCII;
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
    unsafe { join_lit(Side::of_str(a), bits, meta, false) }
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
    unsafe { join_lit(Side::of_str(b), bits, meta, true) }
}

/// `view + <literal>` — fresh refcount-1 Str; the view is borrowed.
///
/// # Safety
///
/// `v` is a valid Substr whose parent is live; `bits` / `meta`
/// describe a literal as the module doc lays out.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_substr_concat_lit(v: *const u8, bits: u64, meta: u64) -> *mut u8 {
    unsafe { join_lit(Side::of_view(v), bits, meta, false) }
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
    unsafe { join_lit(Side::of_view(v), bits, meta, true) }
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
}
