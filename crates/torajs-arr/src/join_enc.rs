//! Encoding-aware piece machinery for the `arr.join` family (RFC
//! 20260711 follow-up).
//!
//! P11.1-S2 made Str payloads dual-encoding — Latin-1 (one byte per
//! code unit) or UTF-16 LE (two bytes per code unit) with `length` =
//! code-unit count — but the join kernels kept the pre-S2 "len =
//! bytes, payload = byte tape" reads, so joining any UTF-16 element
//! (or through a UTF-16 separator / Substr parent) copied half the
//! payload and re-tagged it Latin-1: `["a", "\u{61C}", "b"].join("")`
//! answered U+001C for the middle symbol. This module gives the
//! kernels the same widest-of-inputs machinery `__torajs_str_concat`
//! uses: pass 1 folds every piece's `is_latin1` into the output
//! encoding, pass 2 emits per piece with Latin-1 → UTF-16 widening
//! when the output is wide.

use crate::str_bridge::str_alloc_pooled_enc;

// Str layout mirrors (Layer-2 cross-tier; see torajs-str layout.rs).
pub(crate) const STR_LEN_OFF: usize = 8;
pub(crate) const STR_DATA_OFF: usize = 16;
const STR_FLAG_IS_LATIN1: u16 = 0x0002;

/// Code-unit count of a Str (u32 field @8; the u32 pad @12 is
/// reserved-zero, so legacy u64 reads worked — this reads the field
/// exactly, mirroring torajs-str's own accessor).
#[inline]
pub(crate) unsafe fn str_units(s: *const u8) -> u64 {
    unsafe { (s.add(STR_LEN_OFF) as *const u32).read() as u64 }
}

#[inline]
pub(crate) unsafe fn str_data(s: *const u8) -> *const u8 {
    unsafe { s.add(STR_DATA_OFF) }
}

#[inline]
pub(crate) unsafe fn str_is_latin1(s: *const u8) -> bool {
    let flags = unsafe { (*(s as *const torajs_rc::HeapHeader)).flags };
    flags & STR_FLAG_IS_LATIN1 != 0
}

/// Allocate the join output: `total_units` code units at the
/// widest-of-inputs encoding the caller folded up in pass 1.
#[inline]
pub(crate) unsafe fn alloc_join_out(total_units: u64, latin1: bool) -> *mut u8 {
    unsafe { str_alloc_pooled_enc(total_units, latin1) }
}

/// Append `units` code units at unit-index `cursor` of the output
/// payload. `out_latin1 == true` implies every piece is Latin-1
/// (widest-of-inputs), so the byte copy is exact; a wide output
/// copies UTF-16 pieces verbatim and widens Latin-1 pieces per unit
/// (low byte + zero high byte, little-endian).
#[inline]
pub(crate) unsafe fn emit_units(
    out_data: *mut u8,
    out_latin1: bool,
    cursor: u64,
    src: *const u8,
    units: u64,
    src_latin1: bool,
) {
    if units == 0 {
        return;
    }
    unsafe {
        if out_latin1 {
            core::ptr::copy_nonoverlapping(src, out_data.add(cursor as usize), units as usize);
        } else if src_latin1 {
            for i in 0..units as usize {
                let d = out_data.add((cursor as usize + i) * 2);
                *d = *src.add(i);
                *d.add(1) = 0;
            }
        } else {
            core::ptr::copy_nonoverlapping(
                src,
                out_data.add(cursor as usize * 2),
                (units * 2) as usize,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emit_units_latin1_out_is_byte_copy() {
        let src = *b"abc";
        let mut out = [0u8; 8];
        unsafe { emit_units(out.as_mut_ptr(), true, 2, src.as_ptr(), 3, true) };
        assert_eq!(&out[2..5], b"abc");
    }

    #[test]
    fn emit_units_widens_latin1_into_utf16_out() {
        let src = *b"ab";
        let mut out = [0xFFu8; 8];
        unsafe { emit_units(out.as_mut_ptr(), false, 1, src.as_ptr(), 2, true) };
        assert_eq!(&out[2..6], &[b'a', 0, b'b', 0]);
    }

    #[test]
    fn emit_units_copies_utf16_verbatim() {
        // "中" U+4E2D LE.
        let src = [0x2Du8, 0x4E];
        let mut out = [0u8; 4];
        unsafe { emit_units(out.as_mut_ptr(), false, 0, src.as_ptr(), 1, false) };
        assert_eq!(&out[0..2], &[0x2D, 0x4E]);
    }
}
