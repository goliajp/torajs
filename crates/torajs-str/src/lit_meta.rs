//! How a short literal travels by value: `bits` holds its payload
//! bytes, `meta` its header flag bits and code-unit count (see
//! [`crate::concat_lit`]), and the stores that land it in a cell.

use crate::layout::{STR_FLAG_IS_ASCII, STR_FLAG_IS_LATIN1};

// `bits` is written with plain integer stores
const _: () = assert!(cfg!(target_endian = "little"));

/// Bit position of the literal's code-unit count inside `meta`.
pub const LIT_META_LEN_SHIFT: u32 = 4;

/// The code-unit count's bits once shifted down by
/// [`LIT_META_LEN_SHIFT`].
pub const LIT_META_LEN_MASK: u32 = 0xF;

const _: () = assert!(
    (STR_FLAG_IS_LATIN1 | STR_FLAG_IS_ASCII) & ((LIT_META_LEN_MASK as u16) << LIT_META_LEN_SHIFT)
        == 0
);

#[inline(always)]
pub(crate) fn lit_len(meta: u64) -> u32 {
    (meta >> LIT_META_LEN_SHIFT) as u32 & LIT_META_LEN_MASK
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
