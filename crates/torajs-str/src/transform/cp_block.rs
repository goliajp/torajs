//! A fresh Str from a run of code points — what the case mappings and
//! `normalize` all end with.

use alloc::vec::Vec;

use super::case::encode_cp_utf16_le;
use crate::block::StrBlock;
use crate::max_len::{__torajs_str_too_long, str_len_of};

/// Encode `out_cps` into a fresh StrBlock. Picks Latin-1 if every
/// cp ≤ 0xFF (canonical encoding invariant: payload bytes match
/// the Latin-1 code-unit values 1:1); otherwise UTF-16 LE. A
/// mapping that grows the string past the longest Str (`ß` uppercases
/// to two units) answers the RangeError placeholder.
pub(crate) fn build_block(out_cps: &[u32], max_cp: u32, out_cu: usize) -> *mut u8 {
    let out_latin1 = max_cp <= 0xFF;
    let Some(out_cu) = str_len_of(out_cu) else {
        return unsafe { __torajs_str_too_long() };
    };
    let mut block = StrBlock::alloc_with_encoding(out_cu, out_latin1);
    if out_cu == 0 {
        return block.into_raw();
    }
    let byte_cnt = if out_latin1 { out_cu } else { out_cu * 2 };
    let dst = unsafe { block.as_bytes_mut(byte_cnt) };
    if out_latin1 {
        for (i, &cp) in out_cps.iter().enumerate() {
            dst[i] = cp as u8;
        }
    } else {
        let mut buf: Vec<u8> = Vec::with_capacity(byte_cnt as usize);
        for &cp in out_cps {
            encode_cp_utf16_le(cp, &mut buf);
        }
        debug_assert_eq!(buf.len(), byte_cnt as usize);
        dst.copy_from_slice(&buf);
    }
    block.into_raw()
}
