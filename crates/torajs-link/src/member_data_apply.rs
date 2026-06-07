//! Member-internal reloc patcher for `__DATA,*` sections — #9 SD-4c
//! swap-2k chunk 2.
//!
//! `member_apply::apply_member_relocs` (S7-C5b) walks each member's
//! `__TEXT,__text` reloc table and patches the text payload in place.
//! That covers code calls and code→data ADRP+ADD pairs, but not the
//! `__DATA,*` sections themselves: rustc vtable rodata lives in
//! `__DATA,__const` and contains UNSIGNED relocs that write 8-byte
//! function-pointer slots. Without a corresponding patcher the slots
//! stay 0, and the first `br x3` through a vtable jumps to PC=0.
//!
//! This module is the counterpart for `__DATA,*` payloads. Per the
//! audit shipped alongside swap-2k chunk 2, every `__DATA,*` reloc
//! emitted by rustc-released staticlibs is one of:
//!
//!   - `__DATA,__const`    `r_type=0 ARM64_RELOC_UNSIGNED` × 63 330
//!   - `__DATA,__data`     `r_type=0 ARM64_RELOC_UNSIGNED` ×      2
//!   - `__DATA,__thread_vars` `r_type=0 ARM64_RELOC_UNSIGNED` ×  642
//!
//! All are 8-byte, `r_extern=1` (named-sym) absolute pointer writes.
//! No SUBTRACTOR / POINTER_TO_GOT / ADDEND ever appears in a `__DATA,*`
//! reloc table, so the patcher only implements the UNSIGNED case and
//! returns `UnknownRelocType` (loud failure) for anything else — a
//! rustc upgrade that starts emitting a new reloc kind here surfaces
//! immediately instead of silently producing a NULL slot.
//!
//! `__DATA,__thread_vars` UNSIGND relocs name the per-TLV
//! `$tlv$init` descriptors that dyld binds at process start, so
//! `archive_emit` deliberately skips wiring TLV descriptors through
//! this patcher (the binding happens through chained fixups, not a
//! static UNSIGNED write). `__LD,__compact_unwind` is a link-only
//! segment that ld64 consumes and never appears in a final binary,
//! so it is also out of scope.

use torajs_obj::{
    ARM64_RELOC_UNSIGNED, LC_SEGMENT_64, MH_MAGIC_64, SECTION_64_SIZE, SEGMENT_COMMAND_64_SIZE,
};

use crate::archive::ArMember;
use crate::archive_link::ArchiveLayoutError;
use crate::archives_merge::MergedArchives;
use crate::layout_types::ArchiveLayout;
use crate::member_apply::MemberRelocApplyError;
use crate::member_reloc::{MemberRelocEntry, MemberRelocError, decode_reloc_table};
use crate::member_sections::is_tlv_descriptor;
use crate::patch::write_unsigned64;

/// Walk a member's load commands and decode every reloc entry on the
/// section identified by 1-based `target_section_index` (matching the
/// ordinal a [`DataSectionLayout`] entry carries). Returns an empty
/// `Vec` when the section has `nreloc == 0` — the common case for
/// zerofill (`__bss`) and read-only payloads with no internal
/// pointers.
///
/// Keying on `section_index` instead of `(segname, sectname)`
/// preserves member layouts where the same name appears twice (rustc
/// occasionally splits `__DATA,__const` across multiple sections in
/// a single object file); the caller already knows which physical
/// section it is integrating.
pub fn parse_member_section_relocs(
    member: &ArMember<'_>,
    target_section_index: u8,
) -> Result<Vec<MemberRelocEntry>, MemberRelocError> {
    let bytes = member.data;
    if bytes.len() < 32 {
        return Err(MemberRelocError::TruncatedHeader);
    }
    let magic = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
    if magic != MH_MAGIC_64 {
        return Err(MemberRelocError::TruncatedHeader);
    }
    let ncmds = u32::from_le_bytes(bytes[16..20].try_into().unwrap()) as usize;

    let mut cursor = 32usize;
    let mut running_section_index: u32 = 0;
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
            running_section_index += 1;
            if running_section_index != u32::from(target_section_index) {
                continue;
            }
            let sec = sections_start + i * SECTION_64_SIZE as usize;
            let reloff = u32::from_le_bytes(bytes[sec + 56..sec + 60].try_into().unwrap());
            let nreloc = u32::from_le_bytes(bytes[sec + 60..sec + 64].try_into().unwrap());
            return decode_reloc_table(bytes, reloff, nreloc);
        }
        cursor += cmdsize;
    }
    // Target index past the last section — caller is asking about a
    // section that doesn't exist in this member. Returning empty
    // here mirrors the leaf-member path: callers iterate
    // `DataSectionLayout` entries the layout pass already validated,
    // so an out-of-range index from outside that pass should be a
    // separate bug rather than a silent UnknownRelocType.
    Ok(Vec::new())
}

/// Patch every UNSIGNED reloc on a single `__DATA,*` payload in
/// place with the chained-fixups encoder's per-slot link value.
/// `bytes` is a fresh copy of the section's member-internal payload
/// (`member.data[member_internal_offset..member_internal_offset +
/// size].to_vec()`).
///
/// `relocs` are the entries decoded from the section's own reloc
/// table (see [`parse_member_section_relocs`]).
/// `slot_link_values` is the matching slice of
/// `ChainedFixupsBlob.data_rebase_link_values` — one 8-byte LE
/// `dyld_chained_ptr_64_rebase` link per reloc, in the same order
/// the collector
/// [`crate::member_data_rebase_layout::compute_member_data_rebase_targets`]
/// emitted them so the dyld loader's chain walker stamps each slot
/// at image load (mirroring the e7b user-vtable path but for the
/// `__DATA` segment chain bucket).
///
/// Returns `UnknownRelocType` on any non-UNSIGNED entry,
/// `InvalidSectionIndex` on `r_extern == 0`,
/// `SlotCountMismatch` when `relocs.len() != slot_link_values.len()`.
/// Per the shipped reloc audit (see module doc-comment), every
/// observed `__DATA,*` reloc is UNSIGNED + `r_extern == 1`; the
/// strict-validation errors surface rustc / staticlib changes
/// loudly before the loader can silently write a NULL slot.
pub fn apply_member_data_const_relocs(
    bytes: &mut [u8],
    relocs: &[MemberRelocEntry],
    slot_link_values: &[u64],
) -> Result<(), MemberRelocApplyError> {
    if relocs.is_empty() {
        return Ok(());
    }
    if relocs.len() != slot_link_values.len() {
        return Err(MemberRelocApplyError::SlotCountMismatch {
            relocs: relocs.len(),
            link_values: slot_link_values.len(),
        });
    }
    let payload_size = bytes.len();

    for (r, &link_value) in relocs.iter().zip(slot_link_values) {
        if r.r_type != ARM64_RELOC_UNSIGNED {
            return Err(MemberRelocApplyError::UnknownRelocType {
                r_type: r.r_type,
                r_address: r.r_address,
            });
        }
        if r.r_extern == 0 {
            return Err(MemberRelocApplyError::InvalidSectionIndex {
                r_symbolnum: r.r_symbolnum,
                nsects: 0,
            });
        }

        let off = r.r_address as usize;
        if off
            .checked_add(8)
            .map(|end| end > payload_size)
            .unwrap_or(true)
        {
            return Err(MemberRelocApplyError::PatchSiteOutOfRange {
                r_address: r.r_address,
                length_bytes: 8,
                text_size: payload_size,
            });
        }

        let slot: &mut [u8; 8] = (&mut bytes[off..off + 8])
            .try_into()
            .expect("PatchSiteOutOfRange guard already enforced 8-byte slot");
        write_unsigned64(slot, link_value);
    }
    Ok(())
}

/// swap-2k chunk 2b-4 emit-pass helper. Walks every integrated
/// member's `__DATA,*` file-storage section in
/// `data_non_text_layouts` order, copies each section's bytes out
/// of the member, runs the patcher for non-TLV sections (TLV
/// descriptor slots flow through the per-thunk-slot chained-fixups
/// bind path, not the REBASE chain), and returns the per-section
/// payload Vec in the order
/// [`crate::archive_emit::emit_binary`] consumes. The link-value
/// cursor walks
/// `data_rebase_link_values` in lockstep with the collector
/// [`crate::member_data_rebase_layout::compute_member_data_rebase_targets`].
pub fn collect_member_data_payloads(
    layout: &ArchiveLayout,
    merged: &MergedArchives<'_>,
    data_rebase_link_values: &[u64],
) -> Result<Vec<Vec<u8>>, ArchiveLayoutError> {
    let mut payloads: Vec<Vec<u8>> = Vec::new();
    let mut lv_cursor: usize = 0;
    for (i, per_member) in layout.data_non_text_layouts.iter().enumerate() {
        let m_key = layout.member_layouts[i].key;
        let member = &merged.per_archive_members[m_key.0][m_key.1];
        for s in per_member {
            if !s.has_file_storage {
                continue;
            }
            let off = s.member_internal_offset as usize;
            let end = off + s.size as usize;
            let mut bytes = member.data[off..end].to_vec();
            if !is_tlv_descriptor(s.flags) {
                let relocs =
                    parse_member_section_relocs(member, s.section_index).map_err(|err| {
                        ArchiveLayoutError::MemberReloc {
                            archive_idx: m_key.0,
                            member_idx: m_key.1,
                            err: MemberRelocApplyError::Decode(err),
                        }
                    })?;
                let n = relocs.len();
                if n > 0 {
                    let lvs = &data_rebase_link_values[lv_cursor..][..n];
                    apply_member_data_const_relocs(&mut bytes, &relocs, lvs).map_err(|err| {
                        ArchiveLayoutError::MemberReloc {
                            archive_idx: m_key.0,
                            member_idx: m_key.1,
                            err,
                        }
                    })?;
                    lv_cursor += n;
                }
            }
            payloads.push(bytes);
        }
    }
    debug_assert_eq!(
        lv_cursor,
        data_rebase_link_values.len(),
        "patcher cursor must consume every link value the collector emitted",
    );
    Ok(payloads)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::{AR_HEADER_SIZE, AR_MAGIC, parse_archive};
    use torajs_codegen::CompiledFunction;
    use torajs_codegen::frame::FrameLayout;
    use torajs_obj::write_object;

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
            bytes: vec![0xC0, 0x03, 0x5F, 0xD6],
            relocs: Vec::new(),
            frame: FrameLayout::leaf_no_spill(),
        }
    }

    /// A `torajs-obj` leaf member only carries a `__TEXT,__text`
    /// section, so probing for a `__DATA,__const` ordinal that
    /// doesn't exist must return an empty `Vec` — the patcher then
    /// short-circuits cleanly without surfacing a spurious error.
    #[test]
    fn leaf_member_section_index_past_end_returns_empty() {
        let foo = fn_leaf("_foo");
        let obj = write_object(std::slice::from_ref(&foo));
        let archive = build_short_name_archive("a.o", &obj);
        let members = parse_archive(&archive).unwrap();
        let entries = parse_member_section_relocs(&members[0], 99).unwrap();
        assert!(entries.is_empty(), "out-of-range index returns empty");
    }

    /// Empty reloc list is a no-op short-circuit, regardless of the
    /// link-value slice length (the chunk 2b-4 cursor walks `relocs.len()`
    /// entries, so the empty case never touches the link-value slice).
    #[test]
    fn empty_relocs_short_circuits() {
        let mut payload = vec![0u8; 16];
        apply_member_data_const_relocs(&mut payload, &[], &[]).expect("empty short-circuit");
        assert_eq!(payload, vec![0u8; 16], "payload untouched");
    }

    /// A single UNSIGNED entry writes its paired link value as
    /// little-endian 8 bytes at `r_address`. This is the chunk 2b
    /// happy path: vtable fn-ptr slot stamped with the
    /// `dyld_chained_ptr_64_rebase` encoding the chained-fixups
    /// encoder produced.
    #[test]
    fn unsigned_extern_writes_link_value_little_endian() {
        let entry = MemberRelocEntry {
            r_address: 8,
            r_symbolnum: 0,
            r_pcrel: 0,
            r_length: 3,
            r_extern: 1,
            r_type: ARM64_RELOC_UNSIGNED,
        };
        // Link value bit pattern doesn't matter for the patcher —
        // we just verify it lands at `r_address` little-endian.
        let link_value = 0xDEAD_BEEF_CAFE_0008u64;

        let mut payload = vec![0u8; 16];
        apply_member_data_const_relocs(&mut payload, &[entry], &[link_value])
            .expect("happy path patches cleanly");

        let expected = link_value.to_le_bytes();
        assert_eq!(&payload[0..8], &[0u8; 8], "leading bytes untouched");
        assert_eq!(&payload[8..16], &expected, "8 bytes written at r_address=8");
    }

    /// A non-UNSIGNED reloc surfaces `UnknownRelocType` instead of
    /// silently leaving the slot zero — the audit-driven guarantee.
    #[test]
    fn non_unsigned_reloc_returns_unknown_kind() {
        // r_type=1 = ARM64_RELOC_SUBTRACTOR (never observed in
        // shipped staticlib `__DATA,*` reloc tables but a hostile
        // input we still reject loudly).
        let entry = MemberRelocEntry {
            r_address: 0,
            r_symbolnum: 0,
            r_pcrel: 0,
            r_length: 3,
            r_extern: 1,
            r_type: 1,
        };

        let mut payload = vec![0u8; 8];
        let err = apply_member_data_const_relocs(&mut payload, &[entry], &[0u64]).unwrap_err();
        match err {
            MemberRelocApplyError::UnknownRelocType { r_type, r_address } => {
                assert_eq!(r_type, 1);
                assert_eq!(r_address, 0);
            }
            other => panic!("expected UnknownRelocType, got {other:?}"),
        }
    }

    /// An UNSIGNED entry whose `r_address` would extend past the
    /// payload surfaces `PatchSiteOutOfRange` instead of corrupting
    /// adjacent payload bytes.
    #[test]
    fn out_of_range_patch_site_surfaces_typed_error() {
        // 4-byte payload, reloc claims an 8-byte write at offset 0 →
        // out of range.
        let entry = MemberRelocEntry {
            r_address: 0,
            r_symbolnum: 0,
            r_pcrel: 0,
            r_length: 3,
            r_extern: 1,
            r_type: ARM64_RELOC_UNSIGNED,
        };

        let mut payload = vec![0u8; 4];
        let err = apply_member_data_const_relocs(&mut payload, &[entry], &[0u64]).unwrap_err();
        match err {
            MemberRelocApplyError::PatchSiteOutOfRange {
                r_address,
                length_bytes,
                text_size,
            } => {
                assert_eq!(r_address, 0);
                assert_eq!(length_bytes, 8);
                assert_eq!(text_size, 4);
            }
            other => panic!("expected PatchSiteOutOfRange, got {other:?}"),
        }
    }

    /// `relocs.len() != slot_link_values.len()` is a programming bug
    /// in the archive_emit wire (cursor walks the two in lockstep);
    /// surface it as `SlotCountMismatch` instead of panicking on slice
    /// bounds.
    #[test]
    fn slot_count_mismatch_surfaces_typed_error() {
        let entry = MemberRelocEntry {
            r_address: 0,
            r_symbolnum: 0,
            r_pcrel: 0,
            r_length: 3,
            r_extern: 1,
            r_type: ARM64_RELOC_UNSIGNED,
        };
        let mut payload = vec![0u8; 8];
        let err = apply_member_data_const_relocs(&mut payload, &[entry], &[]).unwrap_err();
        match err {
            MemberRelocApplyError::SlotCountMismatch {
                relocs,
                link_values,
            } => {
                assert_eq!(relocs, 1);
                assert_eq!(link_values, 0);
            }
            other => panic!("expected SlotCountMismatch, got {other:?}"),
        }
    }
}
