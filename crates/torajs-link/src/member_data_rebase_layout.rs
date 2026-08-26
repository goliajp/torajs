//! Per-member `__DATA,*` rebase-target collection — #9 SD-4c swap-2k
//! chunk 2b-1.
//!
//! Mirror of [`crate::user_vtables_layout::compute_vtable_rebase_targets`]
//! for staticlib member `__DATA,*` UNSIGNED relocs. Walks every
//! integrated member's `__DATA,*` sections, parses their reloc tables
//! (via [`crate::member_data_apply::parse_member_section_relocs`]),
//! resolves each entry's target vaddr through the same sym-table +
//! `resolve_local_nsect` lookup the patcher uses, and returns one
//! `(slot_off_in_seg, target_off_in_image)` per UNSIGNED reloc.
//!
//! The output feeds `compute_chained_fixups_outputs` (chunk 2b-2) so
//! the `__DATA` segment's chain bucket includes a REBASE link for
//! every staticlib `__DATA,__const` vtable slot. Once wired, dyld
//! ASLR-slides every slot at image load, replacing the current
//! "write absolute vaddr → loader doesn't rebase → SIGSEGV under PIE"
//! failure observed in chunk 2a.
//!
//! Chunk 2b-1 ships the pure collector + unit tests with no caller —
//! 2b-2 extends `ChainedFixupsInputs`, 2b-3 widens
//! `build_chained_fixups`'s `__DATA` chain, 2b-4 swaps the patcher to
//! consume the encoder's link-value table.
//!
//! Strict-validation policy mirrors the patcher: any reloc kind
//! other than `ARM64_RELOC_UNSIGNED` surfaces `UnknownRelocType`, any
//! `r_extern == 0` entry surfaces `InvalidSectionIndex`, and an
//! unresolved name surfaces `UnresolvedSymbol`. Future rustc / .a
//! changes that introduce a new reloc shape fail loudly here before
//! the encoder ever sees the data.

use torajs_obj::ARM64_RELOC_UNSIGNED;

use crate::archive::ArMember;
use crate::archives_merge::MergedArchives;
use crate::chained_fixups_starts::RebaseTarget;
use crate::data_section_layout::DataSectionLayout;
use crate::layout_types::ArchiveLayout;
use crate::lc::TEXT_VMADDR_BASE;
use crate::member_apply::MemberRelocApplyError;
use crate::member_resolve::resolve_local_nsect;
use crate::member_sections::is_tlv_descriptor;
use crate::member_symtab::{parse_member_symtab, read_nlist_name};
use crate::non_text_layout::NonTextSectionLayout;
use crate::resolve::SymTable;

/// Walk every integrated member's `__DATA,*` sections and produce one
/// `RebaseTarget` per UNSIGNED reloc. Skips zerofill sections
/// (`!has_file_storage` — no slot bytes exist to rebase) and skips
/// TLV descriptor sections (`is_tlv_descriptor(flags)` — those flow
/// through the per-thunk-slot chained-fixups bind path, not the
/// REBASE chain).
///
/// `data_seg_vmaddr` is the final `__DATA` segment vmaddr;
/// `slot_off_in_seg = section.final_vaddr + reloc.r_address -
/// data_seg_vmaddr` lets the chained-fixups encoder bucket each slot
/// into its 16 KB page. `target_off_in_image = target_vaddr -
/// TEXT_VMADDR_BASE` matches the format-6 `target` field the REBASE
/// link encoder consumes.
///
/// Returns the targets in member-walk order — outer `member_layouts`
/// iteration order, inner `data_non_text_layouts[mi]` order, inner
/// reloc table order. 2b-3 / 2b-4 walk the result in the same order
/// so the patcher's index lines up with the encoder's link-value
/// output vec.
pub(crate) fn compute_member_data_rebase_targets(
    layout: &ArchiveLayout,
    merged: &MergedArchives<'_>,
    sym_table: &SymTable,
    data_seg_vmaddr: u64,
) -> Result<Vec<RebaseTarget>, MemberRelocApplyError> {
    let mut targets = Vec::new();
    for (mi, sections) in layout.data_non_text_layouts.iter().enumerate() {
        let member_layout = &layout.member_layouts[mi];
        let (archive_idx, member_idx) = member_layout.key;
        let member = &merged.per_archive_members[archive_idx][member_idx];
        collect_member_targets(
            member,
            sections,
            &member_layout.non_text_sections,
            sym_table,
            data_seg_vmaddr,
            member_layout.vaddr,
            &mut targets,
        )?;
    }
    Ok(targets)
}

/// Per-member collector — exposed at module scope so unit tests can
/// drive it with a hand-built `ArMember` + section layout pair
/// without having to materialize an `ArchiveLayout` / `MergedArchives`.
/// Pushes onto `targets` so the caller controls the output buffer
/// across the whole member walk.
///
/// `member_text_vaddr` is the final virtual address where the member's
/// `__TEXT,__text` payload lands — fed to [`resolve_local_nsect`] for
/// the `n_sect == 1` fallback (member-local sym in `__text`). The
/// previous swap-2k chunk 2b-1 wire incorrectly passed the per-section
/// `__DATA` vaddr here, so any vtable slot referencing a member-local
/// `__text` symbol (e.g. `<I64Writer as fmt::Write>::write_char`)
/// resolved to `data_seg_vmaddr + n_value`, producing the
/// `0x10011b920` (= `CORE_ALLOC + 4080`) SIGBUS observed under
/// `console.log(String(-1))` / `String(1000000)`.
fn collect_member_targets(
    member: &ArMember<'_>,
    data_sections: &[DataSectionLayout],
    non_text_sections: &[NonTextSectionLayout],
    sym_table: &SymTable,
    data_seg_vmaddr: u64,
    member_text_vaddr: u64,
    targets: &mut Vec<RebaseTarget>,
) -> Result<(), MemberRelocApplyError> {
    let mut symtab_cache = None;
    for s in data_sections {
        if !s.has_file_storage {
            continue;
        }
        if is_tlv_descriptor(s.flags) {
            continue;
        }
        let relocs = crate::member_data_apply::parse_member_section_relocs(member, s.section_index)
            .map_err(MemberRelocApplyError::Decode)?;
        if relocs.is_empty() {
            continue;
        }
        let symtab = match symtab_cache {
            Some(ref st) => st,
            None => {
                symtab_cache = Some(parse_member_symtab(member)?);
                symtab_cache.as_ref().unwrap()
            }
        };
        for r in &relocs {
            if r.r_type != ARM64_RELOC_UNSIGNED {
                return Err(MemberRelocApplyError::UnknownRelocType {
                    r_type: r.r_type,
                    r_address: r.r_address,
                });
            }
            if r.r_extern == 0 {
                return Err(MemberRelocApplyError::InvalidSectionIndex {
                    r_symbolnum: r.r_symbolnum,
                    nsects: non_text_sections.len() + data_sections.len(),
                });
            }
            if r.r_symbolnum >= symtab.nsyms {
                return Err(MemberRelocApplyError::SymbolnumOutOfRange {
                    r_symbolnum: r.r_symbolnum,
                    nsyms: symtab.nsyms,
                });
            }
            let name = read_nlist_name(member, symtab, r.r_symbolnum)?;
            let section_final_vaddr = s.final_vaddr;
            let target_vaddr = if let Some(&v) = sym_table.get(&name) {
                v
            } else if let Some(v) = resolve_local_nsect(
                member,
                symtab,
                r.r_symbolnum,
                member_text_vaddr,
                non_text_sections,
                data_sections,
            ) {
                v
            } else {
                return Err(MemberRelocApplyError::UnresolvedSymbol { name: name.clone() });
            };
            let slot_vaddr = section_final_vaddr + u64::from(r.r_address);
            debug_assert!(
                slot_vaddr >= data_seg_vmaddr,
                "data slot vaddr {slot_vaddr:#x} cannot precede __DATA segment base {data_seg_vmaddr:#x}",
            );
            debug_assert!(
                target_vaddr >= TEXT_VMADDR_BASE,
                "data slot {name:?} target vaddr {target_vaddr:#x} cannot precede image base {TEXT_VMADDR_BASE:#x}",
            );
            let slot_off_in_seg = slot_vaddr - data_seg_vmaddr;
            let target_off_in_image = target_vaddr - TEXT_VMADDR_BASE;
            targets.push((slot_off_in_seg, target_off_in_image));
        }
    }
    Ok(())
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

    fn data_section(
        section_index: u8,
        final_vaddr: u64,
        size: u32,
        flags: u32,
        has_file_storage: bool,
    ) -> DataSectionLayout {
        DataSectionLayout {
            section_index,
            sectname: [0; 16],
            segname: [0; 16],
            member_internal_offset: 0,
            size,
            flags,
            has_file_storage,
            final_file_offset: 0,
            final_vaddr,
            member_addr: 0,
            align: 0,
        }
    }

    /// A member that has no `__DATA,*` reloc table (single `__text`
    /// only) contributes zero targets — the collector short-circuits
    /// out of the file-storage section probe because the layout vec
    /// is empty.
    #[test]
    fn leaf_member_no_data_sections_yields_empty() {
        let foo = fn_leaf("_foo");
        let obj = write_object(std::slice::from_ref(&foo));
        let archive = build_short_name_archive("a.o", &obj);
        let members = parse_archive(&archive).unwrap();

        let mut targets = Vec::new();
        let sym_table = SymTable::new();
        collect_member_targets(
            &members[0],
            &[],
            &[],
            &sym_table,
            0x2_0000_0000,
            0xDEAD_BEEF_DEAD_BEEFu64,
            &mut targets,
        )
        .expect("empty data sections short-circuits");
        assert!(targets.is_empty());
    }

    /// Zerofill sections (`!has_file_storage`) carry no slot bytes
    /// for dyld to rebase — the collector skips them even if the
    /// member happens to expose a stub layout entry.
    #[test]
    fn zerofill_section_is_skipped() {
        let foo = fn_leaf("_foo");
        let obj = write_object(std::slice::from_ref(&foo));
        let archive = build_short_name_archive("a.o", &obj);
        let members = parse_archive(&archive).unwrap();

        let zerofill = data_section(99, 0x2_0000_0000, 16, 0, /*has_file_storage*/ false);
        let mut targets = Vec::new();
        let sym_table = SymTable::new();
        collect_member_targets(
            &members[0],
            &[zerofill],
            &[],
            &sym_table,
            0x2_0000_0000,
            0xDEAD_BEEF_DEAD_BEEFu64,
            &mut targets,
        )
        .expect("zerofill skip is a no-op");
        assert!(targets.is_empty());
    }

    /// `__DATA,__thread_vars` sections (`S_THREAD_LOCAL_VARIABLES`
    /// flag low 8 bits = 0x13) flow through the per-thunk-slot bind
    /// path, not the REBASE chain — the collector filters them
    /// before probing the reloc table.
    #[test]
    fn tlv_descriptor_section_is_skipped() {
        let foo = fn_leaf("_foo");
        let obj = write_object(std::slice::from_ref(&foo));
        let archive = build_short_name_archive("a.o", &obj);
        let members = parse_archive(&archive).unwrap();

        // S_THREAD_LOCAL_VARIABLES = 0x13 in the type field (low 8 bits).
        let tlv = data_section(99, 0x2_0000_0000, 24, 0x13, /*has_file_storage*/ true);
        let mut targets = Vec::new();
        let sym_table = SymTable::new();
        collect_member_targets(
            &members[0],
            &[tlv],
            &[],
            &sym_table,
            0x2_0000_0000,
            0xDEAD_BEEF_DEAD_BEEFu64,
            &mut targets,
        )
        .expect("tlv descriptor skip is a no-op");
        assert!(targets.is_empty());
    }

    /// A section ordinal past the member's last LC_SEGMENT_64 section
    /// returns an empty reloc list from `parse_member_section_relocs`
    /// — the collector treats that as zero rebase contribution rather
    /// than surfacing an error (mirrors the patcher's empty short-
    /// circuit at `member_data_apply.rs::apply_member_data_const_relocs`).
    #[test]
    fn out_of_range_section_index_yields_empty_relocs() {
        let foo = fn_leaf("_foo");
        let obj = write_object(std::slice::from_ref(&foo));
        let archive = build_short_name_archive("a.o", &obj);
        let members = parse_archive(&archive).unwrap();

        // section_index past the leaf's only `__text` ordinal.
        let stub = data_section(99, 0x2_0000_0000, 16, 0, /*has_file_storage*/ true);
        let mut targets = Vec::new();
        let sym_table = SymTable::new();
        collect_member_targets(
            &members[0],
            &[stub],
            &[],
            &sym_table,
            0x2_0000_0000,
            0xDEAD_BEEF_DEAD_BEEFu64,
            &mut targets,
        )
        .expect("out-of-range section index → empty reloc table → no targets");
        assert!(targets.is_empty());
    }

    /// `compute_member_data_rebase_targets` over an empty layout
    /// (no integrated members) returns an empty vec without touching
    /// the merged archives' per-member vectors — guards against
    /// pre-flight regressions when the worklist closure picks zero
    /// `__DATA,*`-carrying members.
    #[test]
    fn empty_layout_yields_empty() {
        use crate::archives_merge::MergedArchives;
        let merged: MergedArchives<'_> = MergedArchives {
            per_archive_members: Vec::new(),
            index: std::collections::BTreeMap::new(),
        };
        let layout = empty_archive_layout();
        let sym_table = SymTable::new();
        let targets =
            compute_member_data_rebase_targets(&layout, &merged, &sym_table, 0x2_0000_0000)
                .expect("empty layout cannot fail");
        assert!(targets.is_empty());
    }

    /// Build a minimal `ArchiveLayout` with no integrated members so
    /// the empty-layout test path doesn't have to spell out every
    /// SD-4c-prereq+e8 field. Only the fields the collector reads
    /// (`member_layouts`, `data_non_text_layouts`) matter.
    fn empty_archive_layout() -> ArchiveLayout {
        ArchiveLayout {
            text_file_offset: 0,
            text_size: 0,
            text_vmaddr: 0,
            text_vmsize: 0,
            linkedit_file_offset: 0,
            linkedit_vmaddr: 0,
            linkedit_vmsize: 0,
            symoff: 0,
            stroff: 0,
            total_size: 0,
            nsyms: 0,
            strsize: 0,
            entry_file_offset: 0,
            fn_vaddrs: Vec::new(),
            member_layouts: Vec::new(),
            codesign_dataoff: 0,
            codesign_datasize: 0,
            dyld_imports: std::collections::BTreeMap::new(),
            has_dyld: false,
            stubs_section_vaddr: 0,
            stubs_section_size: 0,
            stubs_file_offset: 0,
            la_ptr_section_vaddr: 0,
            la_ptr_section_size: 0,
            la_ptr_file_offset: 0,
            data_vmsize: 0,
            data_filesize: 0,
            data_vmaddr: 0,
            stub_vaddrs: std::collections::BTreeMap::new(),
            chained_fixups_blob: Vec::new(),
            chained_fixups_dataoff: 0,
            chained_fixups_datasize: 0,
            la_ptr_slot_values: Vec::new(),
            non_text_region_file_offset: 0,
            non_text_region_size: 0,
            data_non_text_layouts: Vec::new(),
            data_non_text_file_offset: 0,
            data_non_text_file_size: 0,
            data_non_text_zerofill_vmsize: 0,
            data_merged_sections: Vec::new(),
            tlv_descriptors: Vec::new(),
            tlv_thunk_link_values: Vec::new(),
            user_strings_layout: crate::user_strings_layout::UserStringsLayout::default(),
            user_strings_payload: Vec::new(),
            user_data_globals_layout:
                crate::user_data_globals_layout::UserDataGlobalsLayout::default(),
            user_vtables_layout: crate::user_vtables_layout::UserVtablesLayout::default(),
            fn_name_table_layout: crate::fn_name_table_layout::FnNameTableLayout::default(),
            class_name_table_layout: crate::class_name_table_layout::ClassNameTableLayout::default(
            ),
            data_const_layout: crate::data_const_layout::DataConstLayout::default(),
            text_rebase_link_values: Vec::new(),
            vtable_rebase_target_count: 0,
            fn_name_rebase_target_count: 0,
            class_name_rebase_target_count: 0,
            baked_regex_rebase_target_count: 0,
        }
    }
}
