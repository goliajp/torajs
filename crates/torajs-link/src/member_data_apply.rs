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
use crate::data_section_layout::DataSectionLayout;
use crate::member_apply::MemberRelocApplyError;
use crate::member_reloc::{MemberRelocEntry, MemberRelocError, decode_reloc_table};
use crate::member_resolve::resolve_local_nsect;
use crate::member_symtab::{parse_member_symtab, read_nlist_name};
use crate::non_text_layout::NonTextSectionLayout;
use crate::patch::write_unsigned64;
use crate::resolve::SymTable;

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

/// Patch every UNSIGNED reloc on a single `__DATA,*` payload in place.
/// `bytes` is a fresh copy of the section's member-internal payload
/// (`member.data[member_internal_offset..member_internal_offset + size]
/// .to_vec()`). `section_final_vaddr` is the vaddr the patched payload
/// will occupy in the linked image — the same `final_vaddr` the
/// section's [`DataSectionLayout`] entry carries.
///
/// `relocs` are the entries decoded from the section's own reloc
/// table (see [`parse_member_section_relocs`]). `sym_table` plus
/// `non_text_sections` / `data_non_text_sections` provide the same
/// final-link target lookup `apply_member_relocs` uses.
///
/// Returns `UnknownRelocType` on any non-UNSIGNED entry. Per the
/// shipped reloc audit (see module doc-comment), every observed
/// `__DATA,*` reloc is UNSIGNED; surfacing a new kind loudly catches
/// future rustc / staticlib changes before they can silently write a
/// NULL slot.
pub fn apply_member_data_const_relocs(
    bytes: &mut [u8],
    member: &ArMember<'_>,
    section_final_vaddr: u64,
    relocs: &[MemberRelocEntry],
    sym_table: &SymTable,
    non_text_sections: &[NonTextSectionLayout],
    data_non_text_sections: &[DataSectionLayout],
) -> Result<(), MemberRelocApplyError> {
    if relocs.is_empty() {
        return Ok(());
    }
    let symtab = parse_member_symtab(member)?;
    let payload_size = bytes.len();

    for r in relocs {
        if r.r_type != ARM64_RELOC_UNSIGNED {
            return Err(MemberRelocApplyError::UnknownRelocType {
                r_type: r.r_type,
                r_address: r.r_address,
            });
        }

        let sym_vaddr = if r.r_extern == 0 {
            // Per audit, no observed `__DATA,*` UNSIGND uses
            // `r_extern=0`; report the strict-validation error so a
            // future encoder change surfaces loudly.
            return Err(MemberRelocApplyError::InvalidSectionIndex {
                r_symbolnum: r.r_symbolnum,
                nsects: non_text_sections.len() + data_non_text_sections.len(),
            });
        } else {
            if r.r_symbolnum >= symtab.nsyms {
                return Err(MemberRelocApplyError::SymbolnumOutOfRange {
                    r_symbolnum: r.r_symbolnum,
                    nsyms: symtab.nsyms,
                });
            }
            let name = read_nlist_name(member, &symtab, r.r_symbolnum)?;
            if let Some(&v) = sym_table.get(&name) {
                v
            } else if let Some(v) = resolve_local_nsect(
                member,
                &symtab,
                r.r_symbolnum,
                section_final_vaddr,
                non_text_sections,
                data_non_text_sections,
            ) {
                v
            } else {
                return Err(MemberRelocApplyError::UnresolvedSymbol { name: name.clone() });
            }
        };

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
        write_unsigned64(slot, sym_vaddr);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::{AR_HEADER_SIZE, AR_MAGIC, ArMember, parse_archive};
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

    /// `apply_member_data_const_relocs` with an empty reloc list is a
    /// no-op, even for a buffer the patcher would otherwise probe.
    #[test]
    fn empty_relocs_skips_symtab_parse() {
        // Member doesn't need an LC_SYMTAB for the empty-list path —
        // a bare 32-byte Mach-O header is enough since we never call
        // `parse_member_symtab` on the empty fast-path.
        let mut bytes = vec![0u8; 32];
        bytes[0..4].copy_from_slice(&MH_MAGIC_64.to_le_bytes());
        let m = ArMember {
            name: "stub.o",
            data: &bytes,
        };
        let mut payload = vec![0u8; 16];
        let sym_table = SymTable::new();
        let r =
            apply_member_data_const_relocs(&mut payload, &m, 0x1_0000, &[], &sym_table, &[], &[]);
        assert!(r.is_ok(), "empty reloc list short-circuits");
        assert_eq!(payload, vec![0u8; 16], "payload untouched");
    }

    /// A single UNSIGNED entry that resolves through `sym_table`
    /// writes the resolved vaddr as little-endian 8 bytes at
    /// `r_address`. This is the chunk-2 happy path: vtable fn-ptr
    /// slot pointing at a code symbol the link driver already knows.
    #[test]
    fn unsigned_extern_writes_sym_vaddr_little_endian() {
        // 16-byte payload, one UNSIGNED reloc at byte 8 → "_target".
        let foo = fn_leaf("_target");
        let obj = write_object(std::slice::from_ref(&foo));
        let archive = build_short_name_archive("a.o", &obj);
        let members = parse_archive(&archive).unwrap();
        let member = &members[0];

        // We don't synthesize a real `__DATA,__const` section in the
        // member (parse_member_section_relocs is exercised separately
        // by `leaf_member_has_no_data_const_section`); instead we
        // construct a `MemberRelocEntry` directly and call the
        // patcher to verify the write semantics in isolation.
        let entry = MemberRelocEntry {
            r_address: 8,
            r_symbolnum: 0, // `_target` is sym index 0 in the leaf object
            r_pcrel: 0,
            r_length: 3,
            r_extern: 1,
            r_type: ARM64_RELOC_UNSIGNED,
        };

        let mut payload = vec![0u8; 16];
        let mut sym_table = SymTable::new();
        sym_table.insert("_target".into(), 0xDEAD_BEEF_CAFE_0008);
        apply_member_data_const_relocs(
            &mut payload,
            member,
            0x1_0000,
            &[entry],
            &sym_table,
            &[],
            &[],
        )
        .expect("happy path patches cleanly");

        let expected = 0xDEAD_BEEF_CAFE_0008u64.to_le_bytes();
        assert_eq!(&payload[0..8], &[0u8; 8], "leading bytes untouched");
        assert_eq!(&payload[8..16], &expected, "8 bytes written at r_address=8");
    }

    /// A non-UNSIGNED reloc surfaces `UnknownRelocType` instead of
    /// silently leaving the slot zero — the audit-driven guarantee.
    #[test]
    fn non_unsigned_reloc_returns_unknown_kind() {
        let foo = fn_leaf("_target");
        let obj = write_object(std::slice::from_ref(&foo));
        let archive = build_short_name_archive("a.o", &obj);
        let members = parse_archive(&archive).unwrap();

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
        let sym_table = SymTable::new();
        let err = apply_member_data_const_relocs(
            &mut payload,
            &members[0],
            0x1_0000,
            &[entry],
            &sym_table,
            &[],
            &[],
        )
        .unwrap_err();
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
        let foo = fn_leaf("_target");
        let obj = write_object(std::slice::from_ref(&foo));
        let archive = build_short_name_archive("a.o", &obj);
        let members = parse_archive(&archive).unwrap();

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
        let mut sym_table = SymTable::new();
        sym_table.insert("_target".into(), 0x1234_5678_9ABC_DEF0);
        let err = apply_member_data_const_relocs(
            &mut payload,
            &members[0],
            0x1_0000,
            &[entry],
            &sym_table,
            &[],
            &[],
        )
        .unwrap_err();
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
}
