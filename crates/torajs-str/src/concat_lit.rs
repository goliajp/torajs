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
//! rather than a `memcpy`. The other operand keeps every runtime rule
//! of [`crate::concat`]: a null pointer is the JS `null` value and
//! reads as the text "null", and the result takes the wider of the two
//! encodings.
//!
//! The compiler picks these entry points for a `StaticStrRef` operand
//! of that size (`torajs-egraph`'s `str_append::literal`); nothing
//! else calls them.

use torajs_rc::{FLAG_STATIC_LITERAL, HeapHeader};

use crate::block::StrBlock;
use crate::layout::{
    STR_DATA_OFF, STR_FLAG_IS_ASCII, STR_FLAG_IS_LATIN1, STR_LEN_OFF, grow_capacity,
};
use crate::str_drop::__torajs_str_drop;
use crate::substr::{FLAG_SUBSTR_INLINE, FLAG_SUBSTR_VIEW};
use crate::substr_methods::{substr_len, substr_offset, substr_parent};

// `bits` is written with plain integer stores
const _: () = assert!(cfg!(target_endian = "little"));

/// Bit position of the literal's code-unit count inside `meta`.
pub const LIT_META_LEN_SHIFT: u32 = 4;

const _: () = assert!((STR_FLAG_IS_LATIN1 | STR_FLAG_IS_ASCII) & (0xF << LIT_META_LEN_SHIFT) == 0);

#[inline(always)]
fn lit_len(meta: u64) -> u32 {
    (meta >> LIT_META_LEN_SHIFT) as u32 & 0xF
}

/// The literal's flag bits; the length bits ride along, and every
/// reader masks with the flag it asks about.
#[inline(always)]
fn lit_flags(meta: u64) -> u16 {
    meta as u16
}

/// Store the low `n` bytes of `bits` at `dst`, `1 <= n <= 8`.
///
/// # Safety
///
/// `dst` is writable for `n` bytes.
#[inline(always)]
unsafe fn write_low_bytes(dst: *mut u8, bits: u64, n: usize) {
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
unsafe fn write_widened(dst: *mut u8, bits: u64, n: usize) {
    for i in 0..n {
        let unit = (bits >> (i * 8)) as u8 as u16;
        unsafe { (dst.add(i * 2) as *mut u16).write_unaligned(unit) };
    }
}

/// Fresh cell holding `payload` (`len` code units, encoding and ASCII
/// mark per `flags`) and the literal, in the order `lit_first` says.
///
/// # Safety
///
/// `payload` is readable for `len` code units in the encoding `flags`
/// names.
#[inline(always)]
unsafe fn join(
    payload: *const u8,
    len: u32,
    flags: u16,
    bits: u64,
    meta: u64,
    lit_first: bool,
) -> *mut u8 {
    let lit_flags = lit_flags(meta);
    if (flags ^ lit_flags) & STR_FLAG_IS_LATIN1 != 0 {
        return unsafe { join_mixed(payload, len, flags, bits, meta, lit_first) };
    }
    let n = lit_len(meta);
    let is_latin1 = flags & STR_FLAG_IS_LATIN1 != 0;
    let shift = u32::from(!is_latin1);
    let side_bytes = (len << shift) as usize;
    let lit_bytes = (n << shift) as usize;
    // only a Latin-1 cell ever carries the ASCII mark
    let ascii = flags & lit_flags & STR_FLAG_IS_ASCII;
    let block = StrBlock::alloc_with_flags(len + n, is_latin1, ascii);
    let dst = unsafe { block.0.as_ptr().add(STR_DATA_OFF) };
    unsafe {
        if lit_first {
            write_low_bytes(dst, bits, lit_bytes);
            core::ptr::copy_nonoverlapping(payload, dst.add(lit_bytes), side_bytes);
        } else {
            core::ptr::copy_nonoverlapping(payload, dst, side_bytes);
            write_low_bytes(dst.add(side_bytes), bits, lit_bytes);
        }
    }
    block.into_raw()
}

/// [`join`] when exactly one side is Latin-1: the result is UTF-16 and
/// that side is widened on the way in.
#[cold]
#[inline(never)]
unsafe fn join_mixed(
    payload: *const u8,
    len: u32,
    flags: u16,
    bits: u64,
    meta: u64,
    lit_first: bool,
) -> *mut u8 {
    let n = lit_len(meta);
    let side_is_latin1 = flags & STR_FLAG_IS_LATIN1 != 0;
    let block = StrBlock::alloc_with_encoding(len + n, false);
    let dst = unsafe { block.0.as_ptr().add(STR_DATA_OFF) };
    let (side_at, lit_at) = if lit_first {
        (n as usize * 2, 0)
    } else {
        (0, len as usize * 2)
    };
    unsafe {
        if side_is_latin1 {
            for i in 0..len as usize {
                (dst.add(side_at + i * 2) as *mut u16)
                    .write_unaligned(payload.add(i).read() as u16);
            }
            write_low_bytes(dst.add(lit_at), bits, n as usize * 2);
        } else {
            core::ptr::copy_nonoverlapping(payload, dst.add(side_at), len as usize * 2);
            write_widened(dst.add(lit_at), bits, n as usize);
        }
    }
    block.into_raw()
}

/// [`join`] for a null Str operand: the JS `null` value, which reads
/// as the text "null".
#[cold]
#[inline(never)]
unsafe fn join_null(bits: u64, meta: u64, lit_first: bool) -> *mut u8 {
    let flags = STR_FLAG_IS_LATIN1 | STR_FLAG_IS_ASCII;
    unsafe { join(b"null".as_ptr(), 4, flags, bits, meta, lit_first) }
}

/// Payload pointer, code-unit count and header flags of a Str cell.
///
/// # Safety
///
/// `s` is a valid, non-null Str heap block.
#[inline(always)]
unsafe fn str_parts(s: *const u8) -> (*const u8, u32, u16) {
    unsafe {
        let flags = (*(s as *const HeapHeader)).flags;
        let len = (s.add(STR_LEN_OFF) as *const u32).read();
        (s.add(STR_DATA_OFF), len, flags)
    }
}

/// Payload pointer, code-unit count and the parent's header flags of
/// a Substr view.
///
/// # Safety
///
/// `v` is a valid Substr whose parent is live.
#[inline(always)]
unsafe fn view_parts(v: *const u8) -> (*const u8, u32, u16) {
    unsafe {
        let parent = substr_parent(v);
        let flags = (*(parent as *const HeapHeader)).flags;
        let shift = u32::from(flags & STR_FLAG_IS_LATIN1 == 0);
        let at = (substr_offset(v) as usize) << shift;
        (parent.add(STR_DATA_OFF + at), substr_len(v) as u32, flags)
    }
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
        return unsafe { join_null(bits, meta, false) };
    }
    let (payload, len, flags) = unsafe { str_parts(a) };
    unsafe { join(payload, len, flags, bits, meta, false) }
}

/// `<literal> + b` — fresh refcount-1 Str; `b` is borrowed.
///
/// # Safety
///
/// Same contract as [`__torajs_str_concat_lit`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_prepend_lit(b: *const u8, bits: u64, meta: u64) -> *mut u8 {
    if b.is_null() {
        return unsafe { join_null(bits, meta, true) };
    }
    let (payload, len, flags) = unsafe { str_parts(b) };
    unsafe { join(payload, len, flags, bits, meta, true) }
}

/// `view + <literal>` — fresh refcount-1 Str; the view is borrowed.
///
/// # Safety
///
/// `v` is a valid Substr whose parent is live; `bits` / `meta`
/// describe a literal as the module doc lays out.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_substr_concat_lit(v: *const u8, bits: u64, meta: u64) -> *mut u8 {
    let (payload, len, flags) = unsafe { view_parts(v) };
    unsafe { join(payload, len, flags, bits, meta, false) }
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
    let (payload, len, flags) = unsafe { view_parts(v) };
    unsafe { join(payload, len, flags, bits, meta, true) }
}

/// `a + <literal>` where **the caller's reference to `a` is
/// consumed**: a cell nobody else holds takes the literal into its
/// slack, the rule [`crate::append`] applies to a Str right side.
/// Answers an owned refcount-1 Str that aliases `a` when the append
/// happened in place.
///
/// # Safety
///
/// `a` is null or a valid Str heap block the caller owns one
/// reference to (released here); `bits` / `meta` describe a literal
/// as the module doc lays out.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_append_lit(a: *mut u8, bits: u64, meta: u64) -> *mut u8 {
    if a.is_null() {
        return unsafe { join_null(bits, meta, false) };
    }
    // SAFETY: caller's contract is a valid Str block at `a`.
    let header = unsafe { &*(a as *const HeapHeader) };
    let flags = header.flags;
    let lit_flags = lit_flags(meta);
    let is_latin1 = flags & STR_FLAG_IS_LATIN1 != 0;
    let plain = flags & (FLAG_STATIC_LITERAL | FLAG_SUBSTR_VIEW | FLAG_SUBSTR_INLINE) == 0;
    // a Latin-1 cell cannot take a UTF-16 literal where it stands
    let fits_width = !is_latin1 || lit_flags & STR_FLAG_IS_LATIN1 != 0;
    if header.refcount != 1 || !plain || !fits_width {
        return unsafe { append_shared(a, bits, meta) };
    }
    let n = lit_len(meta);
    let len = unsafe { (a.add(STR_LEN_OFF) as *const u32).read() };
    let shift = u32::from(!is_latin1);
    let used = len << shift;
    let need = used + (n << shift);
    let mut block = unsafe { StrBlock::from_raw(a) };
    if unsafe { block.payload_capacity() } < need {
        block = unsafe { grow(block, len, used, is_latin1, need) };
    }
    let cell = block.into_raw();
    unsafe {
        let dst = cell.add(STR_DATA_OFF + used as usize);
        if lit_flags & STR_FLAG_IS_LATIN1 != 0 && !is_latin1 {
            write_widened(dst, bits, n as usize);
        } else {
            write_low_bytes(dst, bits, (n << shift) as usize);
        }
        (cell.add(STR_LEN_OFF) as *mut u32).write(len + n);
        // the cell stays known-ASCII only if what landed on it is too
        if lit_flags & STR_FLAG_IS_ASCII == 0 {
            (*(cell as *mut HeapHeader)).flags &= !STR_FLAG_IS_ASCII;
        }
    }
    cell
}

/// The append for a cell this call may not write: copy, then release
/// the reference it was handed.
#[cold]
#[inline(never)]
unsafe fn append_shared(a: *mut u8, bits: u64, meta: u64) -> *mut u8 {
    let r = unsafe { __torajs_str_concat_lit(a, bits, meta) };
    unsafe { __torajs_str_drop(a) };
    r
}

/// Move a solely-held cell's `used` payload bytes into a block with
/// room for `need`, and release the old one. The surplus is what lets
/// the appends after this one land in place.
#[inline(never)]
unsafe fn grow(old: StrBlock, len: u32, used: u32, is_latin1: bool, need: u32) -> StrBlock {
    let kept = unsafe { (*(old.0.as_ptr() as *const HeapHeader)).flags } & STR_FLAG_IS_ASCII;
    let mut grown = StrBlock::alloc_with_capacity(len, is_latin1, grow_capacity(need));
    unsafe {
        grown.header().flags |= kept;
        core::ptr::copy_nonoverlapping(
            old.0.as_ptr().add(STR_DATA_OFF),
            grown.0.as_ptr().add(STR_DATA_OFF),
            used as usize,
        );
    }
    old.free_pool_aware();
    grown
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::__torajs_str_free;
    use crate::layout::byte_capacity;
    use alloc::vec;
    use alloc::vec::Vec;
    use torajs_rc::__torajs_rc_inc;

    fn latin1(payload: &[u8]) -> *mut u8 {
        let mut b = StrBlock::alloc_with_encoding(payload.len() as u32, true);
        unsafe { b.as_bytes_mut(payload.len() as u32) }.copy_from_slice(payload);
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
        let (payload, len, flags) = unsafe { str_parts(p) };
        let is_latin1 = flags & STR_FLAG_IS_LATIN1 != 0;
        let bytes = byte_capacity(len, is_latin1) as usize;
        let payload = unsafe { core::slice::from_raw_parts(payload, bytes) }.to_vec();
        (payload, is_latin1, len)
    }

    fn mark_ascii(p: *mut u8) -> *mut u8 {
        unsafe { (*(p as *mut HeapHeader)).flags |= STR_FLAG_IS_ASCII };
        p
    }

    fn is_marked_ascii(p: *const u8) -> bool {
        unsafe { (*(p as *const HeapHeader)).flags & STR_FLAG_IS_ASCII != 0 }
    }

    /// `(bits, meta)` of a Latin-1 literal.
    fn lit1(payload: &[u8]) -> (u64, u64) {
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
    fn lit2(units: &[u16]) -> (u64, u64) {
        let mut raw = [0u8; 8];
        for (i, u) in units.iter().enumerate() {
            raw[i * 2..i * 2 + 2].copy_from_slice(&u.to_le_bytes());
        }
        (
            u64::from_le_bytes(raw),
            (units.len() as u64) << LIT_META_LEN_SHIFT,
        )
    }

    #[test]
    fn every_latin1_literal_length_lands_byte_exact() {
        let text = b"12345678";
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
                unsafe { __torajs_str_free(p) };
            }
        }
    }

    #[test]
    fn every_utf16_literal_length_lands_byte_exact() {
        let units = [0x4E2D, 0x6587, 0x6D4B, 0x8BD5];
        for n in 1..=4 {
            let a = utf16(&[0x3042]);
            let (bits, meta) = lit2(&units[..n]);
            let r = unsafe { __torajs_str_concat_lit(a, bits, meta) };
            let mut want = vec![0x42, 0x30];
            for u in &units[..n] {
                want.extend_from_slice(&u.to_le_bytes());
            }
            assert_eq!(read(r), (want, false, 1 + n as u32), "n = {n}");
            unsafe { __torajs_str_free(a) };
            unsafe { __torajs_str_free(r) };
        }
    }

    #[test]
    fn a_null_operand_spells_null_on_either_side() {
        let (bits, meta) = lit1(b"-");
        let r = unsafe { __torajs_str_concat_lit(core::ptr::null(), bits, meta) };
        let l = unsafe { __torajs_str_prepend_lit(core::ptr::null(), bits, meta) };
        let o = unsafe { __torajs_str_append_lit(core::ptr::null_mut(), bits, meta) };
        assert_eq!(read(r).0, b"null-");
        assert_eq!(read(l).0, b"-null");
        assert_eq!(read(o).0, b"null-");
        for p in [r, l, o] {
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
            unsafe { __torajs_str_free(p) };
        }
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

    #[test]
    fn append_writes_into_the_slack_without_moving_the_cell() {
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
    fn append_grows_by_doubling_and_keeps_every_byte() {
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
    fn append_widens_a_latin1_literal_onto_a_utf16_cell_in_place() {
        let (bits, meta) = lit1(b", ");
        let acc = utf16(&[0x4E2D]);
        let out = unsafe { __torajs_str_append_lit(acc, bits, meta) };
        assert_eq!(out, acc, "six bytes fit the cell it already has");
        assert_eq!(read(out), (vec![0x2D, 0x4E, b',', 0, b' ', 0], false, 3));
        unsafe { __torajs_str_free(out) };
    }

    #[test]
    fn append_of_a_utf16_literal_onto_a_latin1_cell_reallocates_wide() {
        let (bits, meta) = lit2(&[0x4E2D]);
        let acc = latin1(b"ab");
        let out = unsafe { __torajs_str_append_lit(acc, bits, meta) };
        assert_eq!(read(out), (vec![b'a', 0, b'b', 0, 0x2D, 0x4E], false, 3));
        unsafe { __torajs_str_free(out) };
    }

    #[test]
    fn append_copies_a_shared_cell() {
        let (bits, meta) = lit1(b"x");
        let acc = latin1(b"ab");
        unsafe { __torajs_rc_inc(acc as *mut core::ffi::c_void) };
        let out = unsafe { __torajs_str_append_lit(acc, bits, meta) };
        assert_ne!(out, acc);
        assert_eq!(read(acc).0, b"ab", "the other holder still sees `ab`");
        assert_eq!(read(out).0, b"abx");
        assert_eq!(unsafe { &*(acc as *const HeapHeader) }.refcount, 1);
        unsafe { __torajs_str_free(acc) };
        unsafe { __torajs_str_free(out) };
    }

    #[test]
    fn append_copies_a_rodata_literal() {
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
