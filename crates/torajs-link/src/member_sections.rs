//! Cross-segment section enumerator for archive members —
//! SD-4c-prereq-b1.
//!
//! Extends `member_text::parse_member_text_section` (which finds the
//! single `__TEXT,__text` slice) to yield **every** section a member
//! exposes, in the 1-based ordinal Mach-O reloc entries reference
//! through `r_symbolnum` when `r_extern = 0`. Lives in its own
//! module so [`crate::member_apply`] stays under the 500-prod-LOC
//! hard limit; the section walker shares no state with the per-fn
//! patcher and can be reused by SD-4c-prereq-c's emit pass without
//! pulling the symtab / reloc machinery along.

use torajs_obj::{LC_SEGMENT_64, MH_MAGIC_64, SECTION_64_SIZE, SEGMENT_COMMAND_64_SIZE};

use crate::archive::ArMember;
use crate::member_reloc::MemberRelocError;

/// Per-section metadata pulled from a member's `LC_SEGMENT_64`
/// chain. `section_index` is the **1-based, cross-segment** ordinal
/// — Mach-O's `nlist.n_sect` and section-keyed `r_symbolnum` both
/// reference sections by this contiguous index counted in
/// load-command order, not by per-segment position. `(sectname,
/// segname)` carry the raw 16-byte slot bytes so callers can match
/// against either spelling without re-parsing the slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberSectionInfo {
    /// 1-based section ordinal, counted in `LC_SEGMENT_64` walk
    /// order. Mach-O reserves 0 for "no section" so this never
    /// equals 0.
    pub section_index: u8,
    /// Raw 16-byte `sectname` slot, NUL/space-padded to fill the
    /// fixed-width field. Callers compare against
    /// `b"__text"` / `b"__cstring"` / `b"__const"` etc. with the
    /// trailing padding trimmed.
    pub sectname: [u8; 16],
    /// Raw 16-byte `segname` slot. Empty in conventional `.o`
    /// objects (everything packed under one anonymous
    /// `LC_SEGMENT_64`); populated in final binaries.
    pub segname: [u8; 16],
    /// Byte offset *inside the member* where the section's payload
    /// starts (= `section_64.offset`). Pair with `size` to slice
    /// `member.data[member_internal_offset..member_internal_offset + size]`.
    pub member_internal_offset: u32,
    /// On-disk size of the section payload (= `section_64.size`).
    pub size: u32,
}

/// Walk a member's `LC_SEGMENT_64` chain and yield every section in
/// 1-based ordinal order across all segments. Includes the primary
/// `__TEXT,__text` entry (callers filter it out when they want only
/// non-text sections); the helper stays generic so SD-4c-prereq-c's
/// emit pass can pull `__DATA,__data` etc. without duplicating the
/// walk.
///
/// `.o` files conventionally pack every section under one anonymous
/// `LC_SEGMENT_64` with empty `segname`; final binaries split
/// sections across named segments. The identifying tuple is
/// per-section `(sectname, segname)` — the function records both
/// raw 16-byte slots so callers can match on either.
///
/// Panics if a member declares > 255 sections; Mach-O `n_sect` is a
/// `u8` so any image exceeding that cap is malformed by spec.
pub fn collect_member_sections(
    member: &ArMember<'_>,
) -> Result<Vec<MemberSectionInfo>, MemberRelocError> {
    let bytes = member.data;
    if bytes.len() < 32 {
        return Err(MemberRelocError::TruncatedHeader);
    }
    let magic = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
    if magic != MH_MAGIC_64 {
        return Err(MemberRelocError::TruncatedHeader);
    }
    let ncmds = u32::from_le_bytes(bytes[16..20].try_into().unwrap()) as usize;

    let mut out: Vec<MemberSectionInfo> = Vec::new();
    let mut next_index: u32 = 1;
    let mut cursor = 32usize;
    for _ in 0..ncmds {
        if bytes.len() < cursor + 8 {
            return Err(MemberRelocError::TruncatedLoadCommand { offset: cursor });
        }
        let cmd = u32::from_le_bytes(bytes[cursor..cursor + 4].try_into().unwrap());
        let cmdsize =
            u32::from_le_bytes(bytes[cursor + 4..cursor + 8].try_into().unwrap()) as usize;
        if cmdsize < 8 || bytes.len() < cursor + cmdsize {
            return Err(MemberRelocError::TruncatedLoadCommand { offset: cursor });
        }
        if cmd != LC_SEGMENT_64 {
            cursor += cmdsize;
            continue;
        }
        if cmdsize < SEGMENT_COMMAND_64_SIZE as usize {
            return Err(MemberRelocError::TruncatedLoadCommand { offset: cursor });
        }
        let nsects =
            u32::from_le_bytes(bytes[cursor + 64..cursor + 68].try_into().unwrap()) as usize;
        let sections_start = cursor + SEGMENT_COMMAND_64_SIZE as usize;
        let need = sections_start + nsects * SECTION_64_SIZE as usize;
        if bytes.len() < need || cmdsize < need - cursor {
            return Err(MemberRelocError::TruncatedSegmentSections { offset: cursor });
        }
        for i in 0..nsects {
            let sec = sections_start + i * SECTION_64_SIZE as usize;
            let mut sectname = [0u8; 16];
            sectname.copy_from_slice(&bytes[sec..sec + 16]);
            let mut segname = [0u8; 16];
            segname.copy_from_slice(&bytes[sec + 16..sec + 32]);
            let size = u64::from_le_bytes(bytes[sec + 40..sec + 48].try_into().unwrap());
            let offset = u32::from_le_bytes(bytes[sec + 48..sec + 52].try_into().unwrap());
            let section_index =
                u8::try_from(next_index).expect("> 255 sections per Mach-O image not supported");
            out.push(MemberSectionInfo {
                section_index,
                sectname,
                segname,
                member_internal_offset: offset,
                size: size as u32,
            });
            next_index += 1;
        }
        cursor += cmdsize;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::{AR_HEADER_SIZE, AR_MAGIC};
    use torajs_codegen::CompiledFunction;
    use torajs_codegen::frame::FrameLayout;

    const RET_BYTES: [u8; 4] = [0xC0, 0x03, 0x5F, 0xD6];

    fn build_short_name_archive(name: &str, data: &[u8]) -> Vec<u8> {
        assert!(name.len() <= 16);
        let mut archive: Vec<u8> = Vec::new();
        archive.extend_from_slice(AR_MAGIC);
        let mut header = [b' '; AR_HEADER_SIZE];
        header[..name.len()].copy_from_slice(name.as_bytes());
        header[16] = b'0';
        let size_str = format!("{}", data.len());
        header[48..48 + size_str.len()].copy_from_slice(size_str.as_bytes());
        header[58] = b'`';
        header[59] = b'\n';
        archive.extend_from_slice(&header);
        archive.extend_from_slice(data);
        if data.len() % 2 != 0 {
            archive.push(0);
        }
        archive
    }

    fn fn_leaf(name: &str) -> CompiledFunction {
        CompiledFunction {
            name: name.into(),
            bytes: RET_BYTES.to_vec(),
            relocs: Vec::new(),
            frame: FrameLayout::leaf_no_spill(),
        }
    }

    fn trim_name(slot: &[u8; 16]) -> &[u8] {
        let mut end = slot.len();
        while end > 0 && (slot[end - 1] == 0 || slot[end - 1] == b' ') {
            end -= 1;
        }
        &slot[..end]
    }

    /// A leaf member's only section is `__TEXT,__text` — the walk
    /// returns exactly one entry at section_index 1 with the same
    /// `(offset, size)` `parse_member_text_section` reports.
    #[test]
    fn returns_text_for_leaf_member() {
        let foo = fn_leaf("_foo");
        let obj = torajs_obj::write_object(std::slice::from_ref(&foo));
        let archive = build_short_name_archive("a.o", &obj);
        let members = crate::archive::parse_archive(&archive).unwrap();
        let sections = collect_member_sections(&members[0]).expect("walk leaf member");
        assert_eq!(sections.len(), 1, "leaf member ships __text only");
        let s = &sections[0];
        assert_eq!(s.section_index, 1);
        assert_eq!(trim_name(&s.sectname), b"__text");
        assert_eq!(trim_name(&s.segname), b"__TEXT");

        let (text_off, text_size) = crate::archive_link::parse_member_text_section(&members[0])
            .expect("parse text section");
        assert_eq!(s.member_internal_offset, text_off);
        assert_eq!(s.size, text_size);
    }

    /// Truncated member (< 32-byte Mach-O header) returns a typed
    /// `MemberRelocError`, matching the surface
    /// `parse_member_text_relocs` already uses.
    #[test]
    fn rejects_truncated_header() {
        let too_small = ArMember {
            name: "tiny.o",
            data: &[0u8; 16],
        };
        let err = collect_member_sections(&too_small).expect_err("too-small header");
        assert_eq!(err, MemberRelocError::TruncatedHeader);
    }

    /// Section index is 1-based, contiguous across `LC_SEGMENT_64`
    /// commands. torajs-obj packs all functions into one
    /// `__TEXT,__text` section, so two user functions still yield a
    /// single section entry — verifies the walker doesn't fragment
    /// per-fn.
    #[test]
    fn indexes_are_contiguous() {
        let a = fn_leaf("_a");
        let b = fn_leaf("_b");
        let obj = torajs_obj::write_object(&[a, b]);
        let archive = build_short_name_archive("ab.o", &obj);
        let members = crate::archive::parse_archive(&archive).unwrap();
        let sections = collect_member_sections(&members[0]).expect("walk");
        assert_eq!(sections.len(), 1, "single __text section per .o");
        assert_eq!(sections[0].section_index, 1);
    }
}
