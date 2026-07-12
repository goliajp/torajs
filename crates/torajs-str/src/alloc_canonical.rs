//! Canonical-encoding Str allocation — shared by every kernel that
//! carves a sub-range out of an existing Str (`slice` / `substring`
//! / `substr` / `at` / the trim family).
//!
//! The canonical-encoding invariant (`eq.rs` short-circuit, the
//! inline `=== "literal"` fast path) requires every OWNED Str whose
//! code units all fit Latin-1 to carry the Latin-1 encoding. A
//! sub-range of a UTF-16 source can drop every supra-Latin-1 unit
//! (`"\u{6C49}abc".slice(1)` → `"abc"`), so range-carving kernels
//! must narrow — "the source's encoding is also right for the
//! slice" only holds when the unit set is unchanged.
//! RFC 20260712-string-proto-cluster chunk A2.

use crate::block::StrBlock;

/// Allocate a fresh owned Str holding the `src` payload bytes under
/// `is_latin1`, narrowing a UTF-16 payload to Latin-1 when every
/// unit fits. `src` must be aligned to the encoding's code-unit
/// stride (callers pass whole-unit byte ranges).
pub(crate) fn alloc_units_canonical(src: &[u8], is_latin1: bool) -> *mut u8 {
    if !is_latin1 {
        let all_narrow = src
            .chunks_exact(2)
            .all(|c| u16::from_le_bytes([c[0], c[1]]) <= 0xff);
        if all_narrow {
            let length = (src.len() / 2) as u32;
            let mut block = StrBlock::alloc_with_encoding(length, true);
            if length != 0 {
                let dst = unsafe { block.as_bytes_mut(length) };
                for (i, c) in src.chunks_exact(2).enumerate() {
                    dst[i] = c[0];
                }
            }
            return block.into_raw();
        }
    }
    let stride: u32 = if is_latin1 { 1 } else { 2 };
    let byte_cnt = src.len() as u32;
    let length = byte_cnt / stride;
    let mut block = StrBlock::alloc_with_encoding(length, is_latin1);
    if !src.is_empty() {
        let dst = unsafe { block.as_bytes_mut(byte_cnt) };
        dst.copy_from_slice(src);
    }
    block.into_raw()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{STR_DATA_OFF, STR_FLAG_IS_LATIN1, STR_LEN_OFF};
    use torajs_rc::HeapHeader;

    fn read(p: *const u8) -> (u32, bool, alloc::vec::Vec<u8>) {
        let len = unsafe { (p.add(STR_LEN_OFF) as *const u32).read() };
        let header = unsafe { &*(p as *const HeapHeader) };
        let latin1 = (header.flags & STR_FLAG_IS_LATIN1) != 0;
        let bytes = if latin1 {
            len as usize
        } else {
            len as usize * 2
        };
        let payload = unsafe { core::slice::from_raw_parts(p.add(STR_DATA_OFF), bytes) }.to_vec();
        (len, latin1, payload)
    }

    #[test]
    fn latin1_passthrough() {
        let p = alloc_units_canonical(b"abc", true);
        assert_eq!(read(p), (3, true, b"abc".to_vec()));
        unsafe { crate::block::__torajs_str_free(p) };
    }

    #[test]
    fn utf16_all_narrow_units_narrows() {
        let p = alloc_units_canonical(&[0x61, 0x00, 0x62, 0x00], false);
        assert_eq!(read(p), (2, true, b"ab".to_vec()));
        unsafe { crate::block::__torajs_str_free(p) };
    }

    #[test]
    fn utf16_wide_unit_stays_wide() {
        let src = [0x49, 0x6c, 0x62, 0x00];
        let p = alloc_units_canonical(&src, false);
        assert_eq!(read(p), (2, false, src.to_vec()));
        unsafe { crate::block::__torajs_str_free(p) };
    }

    #[test]
    fn empty_utf16_narrows_to_latin1() {
        let p = alloc_units_canonical(&[], false);
        assert_eq!(read(p), (0, true, alloc::vec::Vec::new()));
        unsafe { crate::block::__torajs_str_free(p) };
    }
}
