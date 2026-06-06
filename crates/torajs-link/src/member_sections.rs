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

use torajs_obj::{
    LC_SEGMENT_64, MH_MAGIC_64, S_GB_ZEROFILL, S_THREAD_LOCAL_VARIABLES, S_THREAD_LOCAL_ZEROFILL,
    S_ZEROFILL, SECTION_64_SIZE, SECTION_TYPE, SEGMENT_COMMAND_64_SIZE,
};

use crate::archive::ArMember;
use crate::member_reloc::MemberRelocError;

/// Returns `true` when `flags` carries a Mach-O section type that
/// occupies vmsize but **no file storage** — the loader zero-inits
/// the slice at startup / TLS bootstrap. `compute_non_text_layouts`
/// (SD-4c-prereq-c-fix) skips file-payload slicing for these and
/// only reserves `__DATA` vmsize.
///
/// Covers `S_ZEROFILL` (`__DATA,__bss` / `__DATA,__common`),
/// `S_GB_ZEROFILL` (giant BSS), and `S_THREAD_LOCAL_ZEROFILL`
/// (`__DATA,__thread_bss`).
pub fn is_no_file_storage(flags: u32) -> bool {
    let ty = flags & SECTION_TYPE;
    ty == S_ZEROFILL || ty == S_GB_ZEROFILL || ty == S_THREAD_LOCAL_ZEROFILL
}

/// Returns `true` when `flags` marks a TLV descriptor section
/// (`__DATA,__thread_vars`). The descriptor table is regular
/// file-storage but every 24-byte entry's `thunk` slot needs a
/// dyld-bind fixup (`__tlv_bootstrap`) — SD-4c-prereq+c hooks that
/// after fix-c/d settles segment routing.
pub fn is_tlv_descriptor(flags: u32) -> bool {
    (flags & SECTION_TYPE) == S_THREAD_LOCAL_VARIABLES
}

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
    /// starts (= `section_64.offset`). For zerofill-family sections
    /// (`flags & SECTION_TYPE` in {`S_ZEROFILL`, `S_GB_ZEROFILL`,
    /// `S_THREAD_LOCAL_ZEROFILL`}) this is conventionally 0 — no
    /// file storage. Callers must consult [`is_no_file_storage`]
    /// before slicing `member.data[off..off + size]`.
    pub member_internal_offset: u32,
    /// vm size of the section (= `section_64.size`). Equals the
    /// file-storage size for regular sections; for zerofill-family
    /// sections, the loader allocates and zero-inits this many bytes
    /// at startup, with no file payload.
    pub size: u32,
    /// Raw 32-bit `section_64.flags` value. Low 8 bits hold the
    /// section type (compare against `S_REGULAR` / `S_ZEROFILL` /
    /// `S_THREAD_LOCAL_*` via [`SECTION_TYPE`]); high 24 bits hold
    /// attribute flags (`S_ATTR_PURE_INSTRUCTIONS` etc.).
    /// SD-4c-prereq-c-fix routes emit decisions off this; the
    /// helpers [`is_no_file_storage`] and [`is_tlv_descriptor`]
    /// encapsulate the common predicates.
    pub flags: u32,
    /// `section_64.addr` — the section's vmaddr **as declared inside
    /// the `.o`**. Conventionally 0 for `__text` sections (clang +
    /// torajs-obj emit 0), but **non-zero for `__DATA,*` sections in
    /// rustc-emit `.rcgu.o` members** — rustc packs descriptor
    /// tables at cumulative section offsets so `nlist.n_value` for
    /// a thread-local sym equals `member_addr + offset_in_section`.
    /// SD-4c-prereq+b2 reads this to recover the descriptor-relative
    /// offset (`offset = n_value - member_addr`).
    pub member_addr: u64,
    /// `section_64.align` — the section's required alignment as a
    /// power-of-two **log2 exponent** (e.g. `3` ⇒ 8-byte aligned).
    /// rustc emits `align 2^3` for `__DATA,__common`/`__bss` holding
    /// the mmalloc atomic free-list globals; honoring it is mandatory
    /// — an `ldapr` atomic load against an unaligned global SIGBUSes
    /// (SD-4c swap-2i). The layout pass (`data_section_layout`) rounds
    /// each section's `final_vaddr` up to `1 << align`; the emit pass
    /// copies it into the output `section_64.align_log2`.
    pub align: u8,
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
            let addr = u64::from_le_bytes(bytes[sec + 32..sec + 40].try_into().unwrap());
            let size = u64::from_le_bytes(bytes[sec + 40..sec + 48].try_into().unwrap());
            let offset = u32::from_le_bytes(bytes[sec + 48..sec + 52].try_into().unwrap());
            let align_raw = u32::from_le_bytes(bytes[sec + 52..sec + 56].try_into().unwrap());
            let flags = u32::from_le_bytes(bytes[sec + 64..sec + 68].try_into().unwrap());
            // Mach-O section align is a small log2 exponent (≤ 14 in
            // practice); clamp to 31 so `1 << align` never overflows a
            // u64 even on a malformed input.
            debug_assert!(
                align_raw <= 31,
                "section align log2 out of range: {align_raw}"
            );
            let align = align_raw.min(31) as u8;
            let section_index =
                u8::try_from(next_index).expect("> 255 sections per Mach-O image not supported");
            out.push(MemberSectionInfo {
                section_index,
                sectname,
                segname,
                member_internal_offset: offset,
                size: size as u32,
                flags,
                member_addr: addr,
                align,
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

    /// torajs-obj emits `__TEXT,__text` with the canonical clang
    /// `(S_REGULAR | S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS)`
    /// flag combo. fix-b's flags read-out preserves that exactly so
    /// fix-c can keep routing __text into __TEXT (RX) instead of
    /// misclassifying it as __DATA via the flags predicate.
    #[test]
    fn text_section_flags_match_torajs_obj_canonical() {
        use torajs_obj::{S_ATTR_PURE_INSTRUCTIONS, S_ATTR_SOME_INSTRUCTIONS, S_REGULAR};
        let foo = fn_leaf("_foo");
        let obj = torajs_obj::write_object(std::slice::from_ref(&foo));
        let archive = build_short_name_archive("a.o", &obj);
        let members = crate::archive::parse_archive(&archive).unwrap();
        let sections = collect_member_sections(&members[0]).expect("walk");
        let s = &sections[0];
        let expected_flags = S_REGULAR | S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS;
        assert_eq!(s.flags, expected_flags, "__text canonical flags");
        assert_eq!(s.flags & SECTION_TYPE, S_REGULAR);
        assert!(!is_no_file_storage(s.flags));
        assert!(!is_tlv_descriptor(s.flags));
    }

    /// is_no_file_storage covers the three section types whose
    /// payloads the loader supplies at startup. Other types — even
    /// thread-local descriptors / TL regular data — keep file
    /// storage and must NOT be routed through the predicate.
    #[test]
    fn is_no_file_storage_matches_zerofill_family_only() {
        assert!(is_no_file_storage(S_ZEROFILL));
        assert!(is_no_file_storage(S_GB_ZEROFILL));
        assert!(is_no_file_storage(S_THREAD_LOCAL_ZEROFILL));
        // Attributes on top must not change the predicate.
        assert!(is_no_file_storage(
            S_ZEROFILL | torajs_obj::S_ATTR_PURE_INSTRUCTIONS,
        ));

        use torajs_obj::{S_CSTRING_LITERALS, S_REGULAR, S_THREAD_LOCAL_REGULAR};
        assert!(!is_no_file_storage(S_REGULAR));
        assert!(!is_no_file_storage(S_CSTRING_LITERALS));
        assert!(!is_no_file_storage(S_THREAD_LOCAL_REGULAR));
        assert!(!is_no_file_storage(S_THREAD_LOCAL_VARIABLES));
    }

    /// is_tlv_descriptor isolates `__thread_vars` (descriptor
    /// table) from `__thread_data` (regular TL payload) and
    /// `__thread_bss` (TL zerofill).
    #[test]
    fn is_tlv_descriptor_matches_thread_local_variables_only() {
        assert!(is_tlv_descriptor(S_THREAD_LOCAL_VARIABLES));
        assert!(is_tlv_descriptor(
            S_THREAD_LOCAL_VARIABLES | torajs_obj::S_ATTR_PURE_INSTRUCTIONS,
        ));

        use torajs_obj::{S_REGULAR, S_THREAD_LOCAL_REGULAR};
        assert!(!is_tlv_descriptor(S_REGULAR));
        assert!(!is_tlv_descriptor(S_THREAD_LOCAL_REGULAR));
        assert!(!is_tlv_descriptor(S_THREAD_LOCAL_ZEROFILL));
        assert!(!is_tlv_descriptor(S_ZEROFILL));
    }
}
