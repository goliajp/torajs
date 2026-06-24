//! Archive-aware layout pass (S7-C3). Extends `exec.rs::compute_layout`
//! with `.o`-member integration so each required member's __text
//! concatenates after user fns + its defined externs flatten into
//! LC_SYMTAB. Pipeline: `cfg.archives` → `merge_archive_indexes` →
//! `compute_required_members` → per-member parse → [`ArchiveLayout`].

use torajs_obj::{
    MachHeader64, NLIST_64_SIZE, SECTION_64_SIZE, SEGMENT_COMMAND_64_SIZE, SYMTAB_COMMAND_SIZE,
};

use crate::archive_link_member_scan::{MemberScanLayouts, scan_member_text_and_symbols};
use crate::archive_link_rebase_assembly::{TextRebaseAssembly, assemble_text_rebase_targets};
use crate::archives_merge::{compute_required_members, merge_archive_indexes};
use crate::chained_fixups_call::{ChainedFixupsInputs, compute_chained_fixups_outputs};
use crate::class_name_table_layout::class_name_table_extra_defined_syms;
use crate::data_const_layout::compute_data_const_layout;
use crate::data_section_layout::compute_data_section_layouts;
use crate::exec::LinkConfig;
use crate::fn_addr_syms::fn_addr_extra_defined_syms;
use crate::fn_name_table_layout::fn_name_table_extra_defined_syms;
pub use crate::layout_types::{ArchiveLayout, ArchiveLayoutError, MemberLayout};
use crate::lc::{
    APPLE_SILICON_PAGE_SIZE, BUILD_VERSION_CMDSIZE, DYSYMTAB_CMDSIZE, LINKEDIT_DATA_CMDSIZE,
    LOAD_DYLIB_LIBCURL_CMDSIZE, LOAD_DYLIB_LIBSYSTEM_CMDSIZE, LOAD_DYLINKER_CMDSIZE, MAIN_CMDSIZE,
    TEXT_VMADDR_BASE,
};
pub use crate::member_text::{MemberTextError, parse_member_text_section};
use crate::non_text_layout::{NonTextLayoutError, compute_non_text_layouts};
use crate::sign::adhoc_codesign_blob_size;
use crate::stubs::{LA_PTR_SLOT_SIZE, STUB_SIZE};
use crate::tlv_descriptor_layout::compute_tlv_descriptor_layouts;
use crate::user_class_layouts_layout::user_class_layouts_extra_defined_syms;
use crate::user_data_globals_layout::{
    build_user_data_globals_region, user_data_globals_extra_defined_syms,
};
use crate::user_strings_layout::{build_user_strings_region, user_strings_extra_defined_syms};
use crate::user_vtables_layout::user_vtables_extra_defined_syms;
use std::collections::BTreeMap;

fn round_up_to(value: u64, align: u64) -> u64 {
    debug_assert!(align.is_power_of_two());
    (value + align - 1) & !(align - 1)
}

/// File + virtual layout for an `archives`-populated `LinkConfig`.
/// `cfg.archives.is_empty()` mirrors `exec.rs::compute_layout` with
/// an empty `member_layouts`.
pub fn compute_archive_layout(cfg: &LinkConfig) -> Result<ArchiveLayout, ArchiveLayoutError> {
    let merged = merge_archive_indexes(&cfg.archives).map_err(ArchiveLayoutError::Merge)?;
    let mut extra = user_strings_extra_defined_syms(&cfg.strings);
    extra.extend(fn_addr_extra_defined_syms(&cfg.funcs));
    extra.extend(user_data_globals_extra_defined_syms(&cfg.data_globals));
    extra.extend(user_vtables_extra_defined_syms(&cfg.vtable_globals));
    // e8-2c — `tr build` sets force_emit so the libtorajs_cycle.a
    // extern resolves even on class-free programs (probes stay false).
    extra.extend(user_class_layouts_extra_defined_syms(
        &cfg.class_layouts,
        cfg.force_emit_class_layouts_globals,
    ));
    extra.extend(fn_name_table_extra_defined_syms(
        &cfg.fn_name_globals,
        cfg.force_emit_fn_name_globals,
    ));
    extra.extend(class_name_table_extra_defined_syms(
        &cfg.class_names,
        cfg.force_emit_class_names_globals,
    ));
    let required =
        compute_required_members(&cfg.funcs, &merged, &extra).map_err(ArchiveLayoutError::Link)?;

    // Sort member keys for reproducible layout.
    let mut member_keys: Vec<(usize, usize)> = required.members.iter().copied().collect();
    member_keys.sort();

    let MemberScanLayouts {
        text_sizes: member_text_sizes,
        text_offsets: member_text_offsets,
        defined_syms: mut member_defined_syms,
    } = scan_member_text_and_symbols(&merged, &member_keys)?;

    // Entry must be a user fn — member fns can be called, not entry.
    let entry_idx = cfg
        .funcs
        .iter()
        .position(|f| f.name == cfg.entry)
        .ok_or_else(|| ArchiveLayoutError::EntryNotInUserFuncs {
            entry: cfg.entry.clone(),
        })?;

    // Phase 3: layout. has_dyld → __stubs/__la_symbol_ptr/chain LC
    // (libSystem ord 1, libcurl ord 2). e8: __DATA_CONST hosts vtable
    // + class_layouts; chain LC also turns on for __DATA_CONST rebases.
    let has_dyld = !required.dyld_imports.is_empty();
    let has_data_const_seg = !cfg.vtable_globals.is_empty()
        || !cfg.class_layouts.is_empty()
        || cfg.force_emit_class_layouts_globals
        || !cfg.fn_name_globals.is_empty()
        || cfg.force_emit_fn_name_globals;
    let has_vtable_rebase = cfg
        .vtable_globals
        .iter()
        .any(|v| v.slot_syms.iter().any(|s| s.is_some()));
    let has_class_layouts_rebase = cfg
        .class_layouts
        .iter()
        .any(|e| !e.child_offsets.is_empty());
    // Step 3b.4 — each fn_name_table entry contributes 2 chain-fixup
    // slots (fn_addr + name_ptr) so any non-empty fn_name_globals
    // triggers the chain pipeline even on otherwise vtable-free
    // programs (e.g. plain `function foo() {}` user TS).
    let has_fn_name_table_rebase =
        !cfg.fn_name_globals.is_empty() || cfg.force_emit_fn_name_globals;
    let has_class_names_table_rebase =
        !cfg.class_names.is_empty() || cfg.force_emit_class_names_globals;
    let has_chained_fixups = has_dyld
        || has_vtable_rebase
        || has_class_layouts_rebase
        || has_fn_name_table_rebase
        || has_class_names_table_rebase;
    let has_libcurl_lc = required.dyld_imports.values().any(|&o| o == 2);
    let load_dylib_size = if has_dyld {
        LOAD_DYLIB_LIBSYSTEM_CMDSIZE
            + if has_libcurl_lc {
                LOAD_DYLIB_LIBCURL_CMDSIZE
            } else {
                0
            }
    } else {
        0
    };
    // segment_count = PAGEZERO + __TEXT + (__DATA_CONST?) + (__DATA?) +
    // __LINKEDIT. section_count = __text + (__stubs? when has_dyld);
    // __DATA_CONST = segment-only rodata blob (no section_64 entry).
    let segment_count = 3 + u32::from(has_dyld) + u32::from(has_data_const_seg);
    let section_count = 1 + if has_dyld { 2 } else { 0 };
    let chained_fixups_lc_size = if has_chained_fixups {
        LINKEDIT_DATA_CMDSIZE
    } else {
        0
    };
    let sizeofcmds = (SEGMENT_COMMAND_64_SIZE * segment_count)
        + (SECTION_64_SIZE * section_count)
        + LOAD_DYLINKER_CMDSIZE
        + load_dylib_size
        + BUILD_VERSION_CMDSIZE
        + MAIN_CMDSIZE
        + SYMTAB_COMMAND_SIZE
        + DYSYMTAB_CMDSIZE
        + chained_fixups_lc_size
        + LINKEDIT_DATA_CMDSIZE;
    let header_plus_lc = (MachHeader64::SIZE as u32) + sizeofcmds;
    let text_file_offset = round_up_to(u64::from(header_plus_lc), APPLE_SILICON_PAGE_SIZE) as u32;

    let user_text_size: u32 = cfg.funcs.iter().map(|f| f.bytes.len() as u32).sum();
    let members_text_total: u32 = member_text_sizes.iter().copied().sum();

    // Non-text region (member __cstring/__const) lands between member
    // __texts and `__TEXT,__stubs`; folds into text_size auto-shift.
    let non_text_region_file_offset = text_file_offset + user_text_size + members_text_total;
    let non_text_result =
        compute_non_text_layouts(&merged, &member_keys, non_text_region_file_offset).map_err(
            |NonTextLayoutError {
                 archive_idx,
                 member_idx,
                 err,
             }| {
                ArchiveLayoutError::MemberSections {
                    archive_idx,
                    member_idx,
                    err,
                }
            },
        )?;
    let non_text_region_size = non_text_result.region_size;

    // e1 — user strings in `__TEXT,__cstring` past member non-text.
    let (user_strings_layout, user_strings_payload) = build_user_strings_region(
        &cfg.strings,
        TEXT_VMADDR_BASE,
        non_text_region_file_offset + non_text_region_size,
    );
    let text_size =
        user_text_size + members_text_total + non_text_region_size + user_strings_layout.total_size;

    // __TEXT = __text + __stubs (has_dyld); vmsize page-aligned.
    let dyld_count = required.dyld_imports.len() as u64;
    let stubs_section_size = if has_dyld { dyld_count * STUB_SIZE } else { 0 };
    let la_ptr_section_size = if has_dyld {
        dyld_count * LA_PTR_SLOT_SIZE
    } else {
        0
    };
    // chunk 3 — 4-align `__stubs` (ARM instr) past non-text padding.
    let stubs_file_offset = round_up_to(u64::from(text_file_offset + text_size), 4) as u32;
    let text_segment_file_end = u64::from(stubs_file_offset) + stubs_section_size;
    let text_vmsize = round_up_to(text_segment_file_end, APPLE_SILICON_PAGE_SIZE);

    // e8 + W-J A3c __DATA_CONST — vtable + class_layouts +
    // fn_name_table + class_name_table rodata.
    let data_const_layout = compute_data_const_layout(
        &cfg.vtable_globals,
        &cfg.class_layouts,
        cfg.force_emit_class_layouts_globals,
        &cfg.fn_name_globals,
        cfg.force_emit_fn_name_globals,
        &cfg.class_names,
        cfg.force_emit_class_names_globals,
        &cfg.baked_regex_entries,
        text_vmsize as u32,
        TEXT_VMADDR_BASE + text_vmsize,
    );
    let has_data_const = data_const_layout.has_data_const;
    let data_file_offset = if has_dyld {
        data_const_layout.segment_file_offset + data_const_layout.segment_filesize as u32
    } else {
        0
    };
    let data_vmaddr = if has_dyld {
        data_const_layout.segment_vmaddr + data_const_layout.segment_vmsize
    } else {
        0
    };

    // Section vaddrs follow directly from segment vmaddrs +
    // section file offsets.
    let stubs_section_vaddr = if has_dyld {
        TEXT_VMADDR_BASE + u64::from(stubs_file_offset)
    } else {
        0
    };
    let la_ptr_section_vaddr = data_vmaddr;

    // c-fix-c3/c4 — `__DATA,*` past `__la_symbol_ptr` (chain seg_off=0).
    let (
        data_non_text_layouts,
        data_non_text_file_offset,
        data_non_text_file_size,
        data_non_text_zerofill_vmsize,
    ) = if has_dyld {
        let file_start = data_file_offset + la_ptr_section_size as u32;
        let vaddr_start = data_vmaddr + la_ptr_section_size;
        let res = compute_data_section_layouts(&merged, &member_keys, file_start, vaddr_start)
            .map_err(
                |NonTextLayoutError {
                     archive_idx,
                     member_idx,
                     err,
                 }| {
                    ArchiveLayoutError::MemberSections {
                        archive_idx,
                        member_idx,
                        err,
                    }
                },
            )?;
        (
            res.per_member,
            file_start,
            res.file_region_size,
            res.zerofill_vmsize,
        )
    } else {
        (Vec::new(), 0u32, 0u32, 0u32)
    };

    // b1/c — member __DATA,__thread_vars TLV descriptors for chain
    // per-thunk-slot binds. has_dyld=false → no __DATA → no TLVs.
    let tlv_descriptors = if has_dyld {
        compute_tlv_descriptor_layouts(&member_keys, &data_non_text_layouts)
            .expect("TLV descriptor layout cannot fail when inputs are zipped from compute_data_section_layouts")
            .descriptors
    } else {
        Vec::new()
    };

    // __DATA: data_filesize page-aligns for next-seg fileoff; vmsize
    // extends with zerofill (member zerofill + e4 user-bss).
    let data_total_file_size = if has_dyld {
        la_ptr_section_size + u64::from(data_non_text_file_size)
    } else {
        0
    };
    let data_filesize = if has_dyld {
        round_up_to(data_total_file_size, APPLE_SILICON_PAGE_SIZE)
    } else {
        0
    };
    // e4 — `__DATA,__bss` zerofill past member zerofill.
    let user_data_globals_layout = build_user_data_globals_region(
        &cfg.data_globals,
        has_dyld,
        data_vmaddr + data_total_file_size + u64::from(data_non_text_zerofill_vmsize),
    )
    .map_err(|count| ArchiveLayoutError::DataGlobalsWithoutDyld { count })?;
    let data_vmsize = if has_dyld {
        round_up_to(
            data_total_file_size
                + u64::from(data_non_text_zerofill_vmsize)
                + u64::from(user_data_globals_layout.total_vmsize),
            APPLE_SILICON_PAGE_SIZE,
        )
    } else {
        0
    };

    // e8: __DATA_CONST seg sits between __TEXT and __DATA; account
    // for its file + vm footprint when locating __LINKEDIT.
    let linkedit_file_offset =
        (text_vmsize + data_const_layout.segment_filesize + data_filesize) as u32;
    let linkedit_vmaddr =
        TEXT_VMADDR_BASE + text_vmsize + data_const_layout.segment_vmsize + data_vmsize;

    // Per-import stub vaddr stride is STUB_SIZE. BTreeMap iter is
    // sorted-by-name → stub_vaddrs key order is reproducible.
    let mut stub_vaddrs: BTreeMap<String, u64> = BTreeMap::new();
    if has_dyld {
        for (i, name) in required.dyld_imports.keys().enumerate() {
            stub_vaddrs.insert(name.clone(), stubs_section_vaddr + (i as u64) * STUB_SIZE);
        }
    }

    // Per-user-func vaddrs.
    let mut fn_vaddrs: Vec<u64> = Vec::with_capacity(cfg.funcs.len());
    let mut running: u32 = 0;
    for f in &cfg.funcs {
        debug_assert_eq!(f.bytes.len() % 4, 0, "user fn bytes must be 4-aligned");
        fn_vaddrs.push(TEXT_VMADDR_BASE + u64::from(text_file_offset) + u64::from(running));
        running += f.bytes.len() as u32;
    }
    let entry_file_offset = text_file_offset
        + (fn_vaddrs[entry_idx] - TEXT_VMADDR_BASE - u64::from(text_file_offset)) as u32;

    // Per-member __text pinning, cumulative past user funcs.
    let mut member_layouts: Vec<MemberLayout> = Vec::with_capacity(member_keys.len());
    let mut cumulative: u32 = running;
    for (i, &key) in member_keys.iter().enumerate() {
        let text_size_i = member_text_sizes[i];
        member_layouts.push(MemberLayout {
            key,
            text_size: text_size_i,
            file_offset: text_file_offset + cumulative,
            vaddr: TEXT_VMADDR_BASE + u64::from(text_file_offset) + u64::from(cumulative),
            member_text_offset_in_member: member_text_offsets[i],
            defined_syms: std::mem::take(&mut member_defined_syms[i]),
            non_text_sections: Vec::new(),
        });
        cumulative += text_size_i;
    }

    // SD-4c-prereq-c — populate per-member non_text_sections from the
    // pre-computed layout (the region size + offsets were folded into
    // text_size up top so stubs_file_offset / text_vmsize landed at
    // the right values automatically).
    for (i, layouts) in non_text_result.per_member.into_iter().enumerate() {
        member_layouts[i].non_text_sections = layouts;
    }

    // nsyms = user fn count + sum(member defined syms).
    let user_nsyms = cfg.funcs.len() as u32;
    let member_nsyms: u32 = member_layouts
        .iter()
        .map(|m| m.defined_syms.len() as u32)
        .sum();
    let nsyms = user_nsyms + member_nsyms;

    let nlist_size = nsyms * NLIST_64_SIZE;
    let symoff = linkedit_file_offset;
    let stroff = symoff + nlist_size;

    // strtab: "\0" + user fn names + member defined-sym names.
    let mut strsize: u32 = 1;
    for f in &cfg.funcs {
        strsize += f.name.len() as u32 + 1;
    }
    for m in &member_layouts {
        for (name, _, _) in &m.defined_syms {
            strsize += name.len() as u32 + 1;
        }
    }

    // SD-3 chain blob; +c binds each TLV thunk to `__tlv_bootstrap`.
    let tlv_thunk_offsets: Vec<u64> = tlv_descriptors
        .iter()
        .map(|d| d.thunk_slot_vaddr - data_vmaddr)
        .collect();
    // Phase 3 — concat vtable | class_layouts | fn_name_table |
    // class_name_table rebase in walk order so dyld + archive_emit
    // can per-region split via the recorded counts.
    let TextRebaseAssembly {
        combined: combined_text_rebase_targets,
        vtable_rebase_target_count,
        fn_name_rebase_target_count,
        class_name_rebase_target_count,
        baked_regex_rebase_target_count,
    } = assemble_text_rebase_targets(&data_const_layout, &fn_vaddrs, &user_strings_layout);

    // e8: __DATA_CONST idx=2; __DATA shifts to idx 3 when has_data_const.
    let data_seg_idx: u32 = if has_data_const { 3 } else { 2 };
    let (
        chained_fixups_blob,
        la_ptr_slot_values,
        tlv_thunk_link_values,
        text_rebase_link_values,
        _data_rebase_link_values,
    ) = compute_chained_fixups_outputs(ChainedFixupsInputs {
        dyld_imports: &required.dyld_imports,
        data_seg_vmaddr_offset: data_vmaddr.saturating_sub(TEXT_VMADDR_BASE),
        data_seg_vmsize: data_vmsize,
        tlv_thunk_offsets: &tlv_thunk_offsets,
        segment_count,
        data_seg_idx,
        data_const_layout: &data_const_layout,
        vtable_rebase_targets: &combined_text_rebase_targets,
        data_seg_rebase_targets: &[],
    });
    // +c: chain + CodeDir both need 8-aligned LINKEDIT offsets.
    let chained_fixups_dataoff = if has_chained_fixups {
        round_up_to(u64::from(stroff + strsize), 8) as u32
    } else {
        0
    };
    let chained_fixups_datasize = chained_fixups_blob.len() as u32;
    let codesign_dataoff = if has_chained_fixups {
        round_up_to(
            u64::from(chained_fixups_dataoff + chained_fixups_datasize),
            8,
        ) as u32
    } else {
        stroff + strsize
    };
    let codesign_datasize = adhoc_codesign_blob_size(codesign_dataoff, &cfg.codesign_ident);
    // __LINKEDIT filesize = codesign end - segment start (covers
    // the alignment pads above).
    let linkedit_data_size =
        (codesign_dataoff + codesign_datasize).saturating_sub(linkedit_file_offset);
    let linkedit_vmsize = round_up_to(u64::from(linkedit_data_size), APPLE_SILICON_PAGE_SIZE);
    let total_size = linkedit_file_offset + linkedit_data_size;

    Ok(ArchiveLayout {
        text_file_offset,
        text_size,
        text_vmaddr: TEXT_VMADDR_BASE,
        text_vmsize,
        linkedit_file_offset,
        linkedit_vmaddr,
        linkedit_vmsize,
        symoff,
        stroff,
        total_size,
        nsyms,
        strsize,
        entry_file_offset,
        fn_vaddrs,
        member_layouts,
        codesign_dataoff,
        codesign_datasize,
        dyld_imports: required.dyld_imports,
        stubs_section_vaddr,
        stubs_section_size,
        stubs_file_offset,
        la_ptr_section_vaddr,
        la_ptr_section_size,
        la_ptr_file_offset: data_file_offset,
        data_vmsize,
        data_filesize,
        data_vmaddr,
        stub_vaddrs,
        chained_fixups_blob,
        chained_fixups_dataoff,
        chained_fixups_datasize,
        la_ptr_slot_values,
        non_text_region_file_offset,
        non_text_region_size,
        data_non_text_layouts,
        data_non_text_file_offset,
        data_non_text_file_size,
        data_non_text_zerofill_vmsize,
        tlv_descriptors,
        tlv_thunk_link_values,
        user_strings_layout,
        user_strings_payload,
        user_data_globals_layout,
        user_vtables_layout: data_const_layout.vtable_layout.clone(),
        fn_name_table_layout: data_const_layout.fn_name_table_layout.clone(),
        class_name_table_layout: data_const_layout.class_name_table_layout.clone(),
        data_const_layout,
        text_rebase_link_values,
        vtable_rebase_target_count,
        fn_name_rebase_target_count,
        class_name_rebase_target_count,
        baked_regex_rebase_target_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::{AR_HEADER_SIZE, AR_MAGIC};
    use crate::archives_merge::ArchiveLinkError;
    use crate::resolve::SymTable;
    use torajs_codegen::CompiledFunction;
    use torajs_codegen::frame::FrameLayout;
    use torajs_codegen::reloc::{CallTarget, Reloc, RelocKind};

    const BL_PLACEHOLDER: [u8; 4] = [0x00, 0x00, 0x00, 0x94];
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

    fn fn_with_extern_calls(name: &str, extern_names: &[&str]) -> CompiledFunction {
        let mut bytes: Vec<u8> = Vec::new();
        let mut relocs: Vec<Reloc> = Vec::new();
        for ext in extern_names {
            let byte_offset = bytes.len() as u32;
            bytes.extend_from_slice(&BL_PLACEHOLDER);
            relocs.push(Reloc {
                byte_offset,
                kind: RelocKind::CallSite {
                    target: CallTarget::Extern((*ext).into()),
                },
            });
        }
        bytes.extend_from_slice(&RET_BYTES);
        CompiledFunction {
            name: name.into(),
            bytes,
            relocs,
            frame: FrameLayout::leaf_no_spill(),
        }
    }

    fn fn_leaf(name: &str) -> CompiledFunction {
        CompiledFunction {
            name: name.into(),
            bytes: RET_BYTES.to_vec(),
            relocs: Vec::new(),
            frame: FrameLayout::leaf_no_spill(),
        }
    }

    /// Parsing a synthetic `.o` should surface the `__TEXT,__text`
    /// size as the sum of the wrapped fn's byte length.
    #[test]
    fn parse_member_text_section_round_trips() {
        let foo = fn_leaf("_foo");
        let obj = torajs_obj::write_object(std::slice::from_ref(&foo));
        let archive = build_short_name_archive("a.o", &obj);
        let members = crate::archive::parse_archive(&archive).unwrap();
        let (offset, size) = parse_member_text_section(&members[0]).unwrap();
        assert!(offset >= 32, "text offset must be past Mach-O header");
        assert_eq!(size as usize, foo.bytes.len());
    }

    /// Pure-user-funcs layout (empty `cfg.archives`) must produce
    /// numbers byte-identical to `exec.rs::compute_layout` for the
    /// shared fields. Validates the wrapper doesn't perturb the
    /// baseline.
    #[test]
    fn empty_archives_layout_matches_exec_baseline() {
        let main = fn_leaf("_main");
        let cfg = LinkConfig {
            funcs: vec![main],
            entry: "_main".into(),
            sym_table: SymTable::new(),
            codesign_ident: "tora".into(),
            archives: Vec::new(),
            strings: Vec::new(),
            data_globals: Vec::new(),
            vtable_globals: Vec::new(),
            class_layouts: Vec::new(),
            force_emit_class_layouts_globals: false,
            fn_name_globals: Vec::new(),
            force_emit_fn_name_globals: false,
            class_names: Vec::new(),
            force_emit_class_names_globals: false,
            baked_regex_entries: Vec::new(),
        };
        let layout = compute_archive_layout(&cfg).unwrap();
        let base = crate::exec::compute_layout(&cfg);
        assert_eq!(layout.text_file_offset, base.text_file_offset);
        assert_eq!(layout.text_size, base.text_size);
        assert_eq!(layout.text_vmsize, base.text_vmsize);
        assert_eq!(layout.linkedit_file_offset, base.linkedit_file_offset);
        assert_eq!(layout.nsyms, base.nsyms);
        assert_eq!(layout.strsize, base.strsize);
        assert_eq!(layout.total_size, base.total_size);
        assert_eq!(layout.fn_vaddrs, base.fn_vaddrs);
        assert!(layout.member_layouts.is_empty());
        // SD-2a — empty dyld_imports leaves the new stubs/la_ptr
        // fields zero/empty so the rest of the emit path stays
        // byte-identical to SD-1.
        assert!(layout.dyld_imports.is_empty());
        assert_eq!(layout.stubs_section_vaddr, 0);
        assert_eq!(layout.stubs_section_size, 0);
        assert_eq!(layout.la_ptr_section_vaddr, 0);
        assert_eq!(layout.la_ptr_section_size, 0);
        assert_eq!(layout.data_vmsize, 0);
        assert_eq!(layout.data_vmaddr, 0);
        assert!(layout.stub_vaddrs.is_empty());
    }

    /// SD-2a — a user fn calling a libSystem-resolved extern
    /// (`_malloc`) must show up in `dyld_imports`, populate the
    /// `__TEXT,__stubs` + `__DATA,__la_symbol_ptr` section fields,
    /// and grow `text_vmsize` / `linkedit_file_offset` to make
    /// room for the new sections + segment.
    #[test]
    fn layout_populates_dyld_stubs_when_libsystem_referenced() {
        let main = fn_with_extern_calls("_main", &["_malloc"]);
        let cfg = LinkConfig {
            funcs: vec![main.clone()],
            entry: "_main".into(),
            sym_table: SymTable::new(),
            codesign_ident: "tora".into(),
            archives: Vec::new(),
            strings: Vec::new(),
            data_globals: Vec::new(),
            vtable_globals: Vec::new(),
            class_layouts: Vec::new(),
            force_emit_class_layouts_globals: false,
            fn_name_globals: Vec::new(),
            force_emit_fn_name_globals: false,
            class_names: Vec::new(),
            force_emit_class_names_globals: false,
            baked_regex_entries: Vec::new(),
        };
        let layout = compute_archive_layout(&cfg).unwrap();
        assert_eq!(layout.dyld_imports.len(), 1);
        assert!(layout.dyld_imports.contains_key("_malloc"));

        // Stubs section + slot section non-zero, sized to the
        // single import.
        assert_eq!(layout.stubs_section_size, super::STUB_SIZE);
        assert_eq!(layout.la_ptr_section_size, super::LA_PTR_SLOT_SIZE);

        // Stubs land right after the user __text.
        assert_eq!(
            u64::from(layout.stubs_file_offset),
            u64::from(layout.text_file_offset) + u64::from(layout.text_size),
        );
        assert_eq!(
            layout.stubs_section_vaddr,
            TEXT_VMADDR_BASE + u64::from(layout.stubs_file_offset),
        );

        // __DATA segment follows the page-aligned end of __TEXT.
        assert_eq!(layout.data_vmaddr, TEXT_VMADDR_BASE + layout.text_vmsize);
        assert_eq!(layout.la_ptr_section_vaddr, layout.data_vmaddr);
        assert!(layout.data_vmsize >= APPLE_SILICON_PAGE_SIZE);
        assert!(layout.data_vmsize % APPLE_SILICON_PAGE_SIZE == 0);

        // __LINKEDIT follows __DATA, not __TEXT.
        assert_eq!(
            u64::from(layout.linkedit_file_offset),
            layout.text_vmsize + layout.data_vmsize,
        );

        // stub_vaddrs map populated for SD-2b's effective sym
        // table extension.
        assert_eq!(layout.stub_vaddrs.len(), 1);
        assert_eq!(
            layout.stub_vaddrs.get("_malloc").copied(),
            Some(layout.stubs_section_vaddr),
        );
    }

    /// SD-2a — multiple imports stride correctly through both the
    /// `__stubs` section and the `__la_symbol_ptr` slot table.
    /// Both sections are emitted in `BTreeSet` (sorted-by-name)
    /// order so byte output is reproducible across runs.
    #[test]
    fn layout_strides_stub_vaddrs_per_import() {
        let main = fn_with_extern_calls("_main", &["_malloc", "_free", "_pthread_create"]);
        let cfg = LinkConfig {
            funcs: vec![main],
            entry: "_main".into(),
            sym_table: SymTable::new(),
            codesign_ident: "tora".into(),
            archives: Vec::new(),
            strings: Vec::new(),
            data_globals: Vec::new(),
            vtable_globals: Vec::new(),
            class_layouts: Vec::new(),
            force_emit_class_layouts_globals: false,
            fn_name_globals: Vec::new(),
            force_emit_fn_name_globals: false,
            class_names: Vec::new(),
            force_emit_class_names_globals: false,
            baked_regex_entries: Vec::new(),
        };
        let layout = compute_archive_layout(&cfg).unwrap();
        assert_eq!(layout.dyld_imports.len(), 3);
        assert_eq!(layout.stubs_section_size, 3 * super::STUB_SIZE);
        assert_eq!(layout.la_ptr_section_size, 3 * super::LA_PTR_SLOT_SIZE);
        // BTreeSet iter is sorted-by-name → _free < _malloc < _pthread_create.
        let names: Vec<&String> = layout.stub_vaddrs.keys().collect();
        assert_eq!(names, vec!["_free", "_malloc", "_pthread_create"]);
        let stride_zero = layout.stub_vaddrs["_free"];
        let stride_one = layout.stub_vaddrs["_malloc"];
        let stride_two = layout.stub_vaddrs["_pthread_create"];
        assert_eq!(stride_zero, layout.stubs_section_vaddr);
        assert_eq!(stride_one, stride_zero + super::STUB_SIZE);
        assert_eq!(stride_two, stride_zero + 2 * super::STUB_SIZE);
    }

    /// `_main` calls `_foo` extern; archive contains `_foo`.
    /// Layout must:
    ///   - place `_main` at TEXT_VMADDR_BASE + text_file_offset
    ///   - place the integrated member directly after `_main`
    ///   - report a `text_size` covering both user fn + member
    ///   - count the member's defined-extern in `nsyms`
    ///   - include `_foo` in the strtab size budget
    #[test]
    fn layout_includes_integrated_member() {
        let foo = fn_leaf("_foo");
        let archive =
            build_short_name_archive("a.o", &torajs_obj::write_object(std::slice::from_ref(&foo)));
        let main = fn_with_extern_calls("_main", &["_foo"]);
        let cfg = LinkConfig {
            funcs: vec![main.clone()],
            entry: "_main".into(),
            sym_table: SymTable::new(),
            codesign_ident: "tora".into(),
            archives: vec![archive],
            strings: Vec::new(),
            data_globals: Vec::new(),
            vtable_globals: Vec::new(),
            class_layouts: Vec::new(),
            force_emit_class_layouts_globals: false,
            fn_name_globals: Vec::new(),
            force_emit_fn_name_globals: false,
            class_names: Vec::new(),
            force_emit_class_names_globals: false,
            baked_regex_entries: Vec::new(),
        };
        let layout = compute_archive_layout(&cfg).unwrap();

        assert_eq!(layout.member_layouts.len(), 1);
        let m = &layout.member_layouts[0];
        assert_eq!(m.key, (0, 0));
        assert_eq!(m.text_size as usize, foo.bytes.len());

        // _main lands at the page boundary; the member's __text
        // lands right after _main's bytes.
        assert_eq!(
            layout.fn_vaddrs[0],
            TEXT_VMADDR_BASE + u64::from(layout.text_file_offset),
        );
        assert_eq!(m.vaddr, layout.fn_vaddrs[0] + main.bytes.len() as u64);
        assert_eq!(
            m.file_offset,
            layout.text_file_offset + main.bytes.len() as u32,
        );

        // text_size = user fn + member fn.
        assert_eq!(
            layout.text_size as usize,
            main.bytes.len() + foo.bytes.len(),
        );

        // nsyms = 1 (_main) + 1 (_foo) = 2.
        assert_eq!(layout.nsyms, 2);
        // strsize covers "\0" + "_main\0" + "_foo\0" = 1 + 6 + 5 = 12.
        assert_eq!(layout.strsize, 1 + (main.name.len() as u32 + 1) + 5);

        // member_defined_syms contains _foo at n_value 0 (foo is
        // the only fn in its .o so it lands at section-offset 0).
        assert_eq!(m.defined_syms.len(), 1);
        assert_eq!(m.defined_syms[0].0, "_foo");
        assert_eq!(m.defined_syms[0].1, 0);
    }

    /// Transitive integration: `_main → _foo → _bar`, each in
    /// its own archive member. Layout must list both members in
    /// `member_layouts`, in sorted (archive_idx, member_idx)
    /// order, with cumulative vaddrs.
    #[test]
    fn layout_integrates_transitive_chain() {
        let foo = fn_with_extern_calls("_foo", &["_bar"]);
        let bar = fn_leaf("_bar");
        let archive_a =
            build_short_name_archive("a.o", &torajs_obj::write_object(std::slice::from_ref(&foo)));
        let archive_b =
            build_short_name_archive("b.o", &torajs_obj::write_object(std::slice::from_ref(&bar)));
        let main = fn_with_extern_calls("_main", &["_foo"]);
        let cfg = LinkConfig {
            funcs: vec![main.clone()],
            entry: "_main".into(),
            sym_table: SymTable::new(),
            codesign_ident: "tora".into(),
            archives: vec![archive_a, archive_b],
            strings: Vec::new(),
            data_globals: Vec::new(),
            vtable_globals: Vec::new(),
            class_layouts: Vec::new(),
            force_emit_class_layouts_globals: false,
            fn_name_globals: Vec::new(),
            force_emit_fn_name_globals: false,
            class_names: Vec::new(),
            force_emit_class_names_globals: false,
            baked_regex_entries: Vec::new(),
        };
        let layout = compute_archive_layout(&cfg).unwrap();

        assert_eq!(layout.member_layouts.len(), 2);
        // Members sorted by (archive_idx, member_idx).
        assert_eq!(layout.member_layouts[0].key, (0, 0));
        assert_eq!(layout.member_layouts[1].key, (1, 0));

        // Cumulative vaddrs: main → foo → bar.
        let main_start = layout.fn_vaddrs[0];
        assert_eq!(
            layout.member_layouts[0].vaddr,
            main_start + main.bytes.len() as u64,
        );
        assert_eq!(
            layout.member_layouts[1].vaddr,
            layout.member_layouts[0].vaddr + layout.member_layouts[0].text_size as u64,
        );
    }

    /// Unresolved extern bubbles up as `ArchiveLayoutError::Link`
    /// (typed) rather than panicking — production callers can
    /// surface the missing-library message.
    #[test]
    fn unresolved_extern_returns_typed_error() {
        let main = fn_with_extern_calls("_main", &["_missing"]);
        let cfg = LinkConfig {
            funcs: vec![main],
            entry: "_main".into(),
            sym_table: SymTable::new(),
            codesign_ident: "tora".into(),
            archives: Vec::new(),
            strings: Vec::new(),
            data_globals: Vec::new(),
            vtable_globals: Vec::new(),
            class_layouts: Vec::new(),
            force_emit_class_layouts_globals: false,
            fn_name_globals: Vec::new(),
            force_emit_fn_name_globals: false,
            class_names: Vec::new(),
            force_emit_class_names_globals: false,
            baked_regex_entries: Vec::new(),
        };
        let err = compute_archive_layout(&cfg).unwrap_err();
        match err {
            ArchiveLayoutError::Link(ArchiveLinkError::UnresolvedExterns { names }) => {
                assert_eq!(names, vec!["_missing".to_string()]);
            }
            other => panic!("expected Link::UnresolvedExterns, got {other:?}"),
        }
    }

    /// Entry symbol must be a user function. Pointing it at a
    /// symbol that only exists in an archive surfaces a typed
    /// error.
    #[test]
    fn entry_must_be_user_func() {
        let foo = fn_leaf("_foo");
        let archive =
            build_short_name_archive("a.o", &torajs_obj::write_object(std::slice::from_ref(&foo)));
        let main = fn_with_extern_calls("_main", &["_foo"]);
        let cfg = LinkConfig {
            funcs: vec![main],
            entry: "_foo".into(), // wrong — _foo isn't a user fn
            sym_table: SymTable::new(),
            codesign_ident: "tora".into(),
            archives: vec![archive],
            strings: Vec::new(),
            data_globals: Vec::new(),
            vtable_globals: Vec::new(),
            class_layouts: Vec::new(),
            force_emit_class_layouts_globals: false,
            fn_name_globals: Vec::new(),
            force_emit_fn_name_globals: false,
            class_names: Vec::new(),
            force_emit_class_names_globals: false,
            baked_regex_entries: Vec::new(),
        };
        let err = compute_archive_layout(&cfg).unwrap_err();
        assert!(matches!(
            err,
            ArchiveLayoutError::EntryNotInUserFuncs { entry } if entry == "_foo"
        ));
    }
}
