//! Archive-aware layout pass (S7-C3). Extends `exec.rs::compute_layout`
//! with `.o`-member integration so each required member's __text
//! concatenates after user fns + its defined externs flatten into
//! LC_SYMTAB. Pipeline: `cfg.archives` → `merge_archive_indexes` →
//! `compute_required_members` → per-member parse → [`ArchiveLayout`].
//!
//! 2026-07-03 fn-debt decomp: the 428-LOC `compute_archive_layout`
//! body split into three phase fns — [`compute_text_region_plan`] /
//! [`compute_data_region_plan`] / [`compute_linkedit_plan`] — with
//! carrier structs; this fn keeps input scanning, per-fn/member
//! vaddr pinning, symtab sizing, and the final assembly.

use torajs_obj::NLIST_64_SIZE;

use crate::archive_layout_data::compute_data_region_plan;
use crate::archive_layout_linkedit::compute_linkedit_plan;
use crate::archive_layout_text::compute_text_region_plan;
use crate::archive_link_extra_syms::collect_extra_defined_syms;
use crate::archive_link_member_scan::{MemberScanLayouts, scan_member_text_and_symbols};
use crate::archives_merge::{compute_required_members, merge_archive_indexes};
use crate::exec::LinkConfig;
pub use crate::layout_types::{ArchiveLayout, ArchiveLayoutError, MemberLayout};
use crate::lc::TEXT_VMADDR_BASE;
pub use crate::member_text::{MemberTextError, parse_member_text_section};

pub(crate) fn round_up_to(value: u64, align: u64) -> u64 {
    debug_assert!(align.is_power_of_two());
    (value + align - 1) & !(align - 1)
}

/// File + virtual layout for an `archives`-populated `LinkConfig`.
/// `cfg.archives.is_empty()` mirrors `exec.rs::compute_layout` with
/// an empty `member_layouts`.
pub fn compute_archive_layout(cfg: &LinkConfig) -> Result<ArchiveLayout, ArchiveLayoutError> {
    let merged = merge_archive_indexes(&cfg.archives).map_err(ArchiveLayoutError::Merge)?;
    compute_archive_layout_with_merged(cfg, &merged)
}

/// Layout against an already-merged archive index. The emit driver
/// merges once and shares the result between layout and emit — the
/// double `merge_archive_indexes` (full archive + per-member symtab
/// re-parse) was ~23ms/case, ~1/3 of the link wall.
pub fn compute_archive_layout_with_merged(
    cfg: &LinkConfig,
    merged: &crate::archives_merge::MergedArchives<'_>,
) -> Result<ArchiveLayout, ArchiveLayoutError> {
    let extra = collect_extra_defined_syms(cfg);
    let required =
        compute_required_members(&cfg.funcs, merged, &extra).map_err(ArchiveLayoutError::Link)?;

    // S2 dead-strip blade 1 — accounting only, env-gated, no effect
    // on the emitted binary (RFC 20260824-s2-dead-strip).
    if std::env::var_os("TORAJS_LINK_DEADSTRIP_DIAG").is_some() {
        crate::dead_strip_diag::report(cfg, merged, &required, &extra);
    }

    // Sort member keys for reproducible layout.
    let mut member_keys: Vec<(usize, usize)> = required.members.iter().copied().collect();
    member_keys.sort();

    let MemberScanLayouts {
        text_sizes: member_text_sizes,
        text_offsets: member_text_offsets,
        defined_syms: mut member_defined_syms,
    } = scan_member_text_and_symbols(merged, &member_keys)?;

    // Entry must be a user fn — member fns can be called, not entry.
    let entry_idx = cfg
        .funcs
        .iter()
        .position(|f| f.name == cfg.entry)
        .ok_or_else(|| ArchiveLayoutError::EntryNotInUserFuncs {
            entry: cfg.entry.clone(),
        })?;

    let mut tp =
        compute_text_region_plan(cfg, &required, merged, &member_keys, &member_text_sizes)?;
    let dp = compute_data_region_plan(cfg, &required, merged, &member_keys, &tp)?;

    // Per-user-func vaddrs.
    let mut fn_vaddrs: Vec<u64> = Vec::with_capacity(cfg.funcs.len());
    let mut running: u32 = 0;
    for f in &cfg.funcs {
        debug_assert_eq!(f.bytes.len() % 4, 0, "user fn bytes must be 4-aligned");
        fn_vaddrs.push(TEXT_VMADDR_BASE + u64::from(tp.text_file_offset) + u64::from(running));
        running += f.bytes.len() as u32;
    }
    let entry_file_offset = tp.text_file_offset
        + (fn_vaddrs[entry_idx] - TEXT_VMADDR_BASE - u64::from(tp.text_file_offset)) as u32;

    // Per-member __text pinning, cumulative past user funcs.
    let mut member_layouts: Vec<MemberLayout> = Vec::with_capacity(member_keys.len());
    let mut cumulative: u32 = running;
    for (i, &key) in member_keys.iter().enumerate() {
        let text_size_i = member_text_sizes[i];
        member_layouts.push(MemberLayout {
            key,
            text_size: text_size_i,
            file_offset: tp.text_file_offset + cumulative,
            vaddr: TEXT_VMADDR_BASE + u64::from(tp.text_file_offset) + u64::from(cumulative),
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
    for (i, layouts) in std::mem::take(&mut tp.non_text_per_member)
        .into_iter()
        .enumerate()
    {
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
    let symoff = dp.linkedit_file_offset;
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

    let lp = compute_linkedit_plan(cfg, &required, &tp, &dp, &fn_vaddrs, stroff, strsize);

    Ok(ArchiveLayout {
        text_file_offset: tp.text_file_offset,
        text_size: tp.text_size,
        text_vmaddr: TEXT_VMADDR_BASE,
        text_vmsize: tp.text_vmsize,
        linkedit_file_offset: dp.linkedit_file_offset,
        linkedit_vmaddr: dp.linkedit_vmaddr,
        linkedit_vmsize: lp.linkedit_vmsize,
        symoff,
        stroff,
        total_size: lp.total_size,
        nsyms,
        strsize,
        entry_file_offset,
        fn_vaddrs,
        member_layouts,
        codesign_dataoff: lp.codesign_dataoff,
        codesign_datasize: lp.codesign_datasize,
        dyld_imports: required.dyld_imports,
        has_dyld: tp.has_dyld,
        stubs_section_vaddr: dp.stubs_section_vaddr,
        stubs_section_size: tp.stubs_section_size,
        stubs_file_offset: tp.stubs_file_offset,
        la_ptr_section_vaddr: dp.la_ptr_section_vaddr,
        la_ptr_section_size: tp.la_ptr_section_size,
        la_ptr_file_offset: dp.data_file_offset,
        data_vmsize: dp.data_vmsize,
        data_filesize: dp.data_filesize,
        data_vmaddr: dp.data_vmaddr,
        stub_vaddrs: dp.stub_vaddrs,
        chained_fixups_blob: lp.chained_fixups_blob,
        chained_fixups_dataoff: lp.chained_fixups_dataoff,
        chained_fixups_datasize: lp.chained_fixups_datasize,
        la_ptr_slot_values: lp.la_ptr_slot_values,
        non_text_region_file_offset: tp.non_text_region_file_offset,
        non_text_region_size: tp.non_text_region_size,
        data_non_text_layouts: dp.data_non_text_layouts,
        data_non_text_file_offset: dp.data_non_text_file_offset,
        data_non_text_file_size: dp.data_non_text_file_size,
        data_non_text_zerofill_vmsize: dp.data_non_text_zerofill_vmsize,
        tlv_descriptors: dp.tlv_descriptors,
        tlv_thunk_link_values: lp.tlv_thunk_link_values,
        user_strings_layout: tp.user_strings_layout,
        user_strings_payload: tp.user_strings_payload,
        user_data_globals_layout: dp.user_data_globals_layout,
        user_vtables_layout: dp.data_const_layout.vtable_layout.clone(),
        fn_name_table_layout: dp.data_const_layout.fn_name_table_layout.clone(),
        class_name_table_layout: dp.data_const_layout.class_name_table_layout.clone(),
        data_const_layout: dp.data_const_layout,
        text_rebase_link_values: lp.text_rebase_link_values,
        vtable_rebase_target_count: lp.vtable_rebase_target_count,
        fn_name_rebase_target_count: lp.fn_name_rebase_target_count,
        class_name_rebase_target_count: lp.class_name_rebase_target_count,
        baked_regex_rebase_target_count: lp.baked_regex_rebase_target_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::{AR_HEADER_SIZE, AR_MAGIC};
    use crate::archives_merge::ArchiveLinkError;
    use crate::lc::APPLE_SILICON_PAGE_SIZE;
    use crate::resolve::SymTable;
    use crate::stubs::{LA_PTR_SLOT_SIZE, STUB_SIZE};
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
            dead_strip: false,
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
            dead_strip: false,
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
        assert_eq!(layout.stubs_section_size, STUB_SIZE);
        assert_eq!(layout.la_ptr_section_size, LA_PTR_SLOT_SIZE);

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
            dead_strip: false,
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
        assert_eq!(layout.stubs_section_size, 3 * STUB_SIZE);
        assert_eq!(layout.la_ptr_section_size, 3 * LA_PTR_SLOT_SIZE);
        // BTreeSet iter is sorted-by-name → _free < _malloc < _pthread_create.
        let names: Vec<&String> = layout.stub_vaddrs.keys().collect();
        assert_eq!(names, vec!["_free", "_malloc", "_pthread_create"]);
        let stride_zero = layout.stub_vaddrs["_free"];
        let stride_one = layout.stub_vaddrs["_malloc"];
        let stride_two = layout.stub_vaddrs["_pthread_create"];
        assert_eq!(stride_zero, layout.stubs_section_vaddr);
        assert_eq!(stride_one, stride_zero + STUB_SIZE);
        assert_eq!(stride_two, stride_zero + 2 * STUB_SIZE);
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
            dead_strip: false,
            archives: vec![archive.into()],
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
            dead_strip: false,
            archives: vec![archive_a.into(), archive_b.into()],
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
            dead_strip: false,
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
            dead_strip: false,
            archives: vec![archive.into()],
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
