//! Archive-aware emit pass (S7-C4): emits a complete `MH_EXECUTE`
//! Mach-O byte stream including user fns + member text/data
//! payloads + chained-fixups + ad-hoc codesign blob.

use torajs_obj::{
    CPU_SUBTYPE_ARM64_ALL, CPU_TYPE_ARM64, MH_MAGIC_64, MachHeader64, Nlist64,
    S_ATTR_PURE_INSTRUCTIONS, S_ATTR_SOME_INSTRUCTIONS, SECTION_64_SIZE, SEGMENT_COMMAND_64_SIZE,
    SYMTAB_COMMAND_SIZE, Section64, SegmentCommand64, StringTable, SymtabCommand, VM_PROT_EXECUTE,
    VM_PROT_READ,
};

use crate::archive_emit_lc_meta::{EmitLcMeta, compute_emit_lc_meta};
use crate::archive_link::{ArchiveLayout, ArchiveLayoutError, compute_archive_layout};
use crate::archives_merge::merge_archive_indexes;
use crate::chained_fixups_call::recompute_chained_fixups_with_data_rebase;
use crate::class_name_table_layout::{
    apply_class_name_table_overrides, build_class_name_table_payload,
};
use crate::data_section_emit::{
    build_data_non_text_section_64_entries, write_data_non_text_file_payloads,
};
use crate::data_section_layout::DataSectionLayout;
use crate::defined_extern_resolve::section_vaddr_for_sym;
use crate::dyld_emit::{
    build_data_segment, build_stubs_section, write_la_ptr_section, write_stubs_section,
};
use crate::exec::LinkConfig;
use crate::fn_addr_syms::register_fn_addr_syms;
use crate::fn_name_table_layout::{apply_fn_name_table_overrides, build_fn_name_table_payload};
use crate::lc::{
    BUILD_VERSION_CMDSIZE, DYSYMTAB_CMDSIZE, LINKEDIT_DATA_CMDSIZE, LOAD_DYLINKER_CMDSIZE,
    MAIN_CMDSIZE, MH_EXECUTE, PAGEZERO_VMSIZE, write_build_version, write_code_signature,
    write_dyld_chained_fixups, write_dysymtab, write_lc_main, write_load_dylib_libcurl,
    write_load_dylib_libsystem, write_load_dylinker,
};
use crate::member_apply::apply_member_relocs;
use crate::member_data_apply::collect_member_data_payloads;
use crate::member_data_rebase_layout::compute_member_data_rebase_targets;
use crate::resolve::apply_relocs;
use crate::sign::build_adhoc_codesign_blob;
use crate::tlv_descriptor_layout::apply_tlv_overrides;
use crate::user_class_layouts_layout::{
    apply_user_class_layouts_overrides, build_user_class_layouts_payload,
};
use crate::user_data_globals_layout::apply_user_data_global_overrides;
use crate::user_strings_emit::apply_user_string_overrides;
use crate::user_vtables_layout::{apply_user_vtable_overrides, build_user_vtables_payload};

/// Link an `archives`-populated `LinkConfig` into a complete
/// ad-hoc-codesigned `MH_EXECUTE` byte stream. User-fn + member
/// relocs both resolve against one effective sym table (caller
/// externs + every member defined-external + dyld stub vaddrs).
pub fn link_to_exec_with_archives(cfg: &LinkConfig) -> Result<Vec<u8>, ArchiveLayoutError> {
    let mut layout = compute_archive_layout(cfg)?;
    let merged = merge_archive_indexes(&cfg.archives).map_err(ArchiveLayoutError::Merge)?;

    // Effective sym table = caller externs + member defined-externs
    // (vaddr dispatched by home section) + SD-2b dyld-stub vaddrs.
    let mut effective_sym_table = cfg.sym_table.clone();
    for (m_idx, m) in layout.member_layouts.iter().enumerate() {
        let data_layouts = member_data_layouts(&layout, m_idx);
        for (name, n_value, n_sect) in &m.defined_syms {
            let vaddr = section_vaddr_for_sym(
                *n_sect,
                *n_value,
                m.vaddr,
                &m.non_text_sections,
                data_layouts,
            );
            effective_sym_table.insert(name.clone(), vaddr);
        }
    }
    for (name, stub_vaddr) in &layout.stub_vaddrs {
        effective_sym_table.insert(name.clone(), *stub_vaddr);
    }

    // SD-4c-prereq overrides for ADRP+ADD targets.
    apply_tlv_overrides(&layout, &merged, &mut effective_sym_table)?;
    apply_user_string_overrides(&layout.user_strings_layout, &mut effective_sym_table);
    apply_user_data_global_overrides(&layout.user_data_globals_layout, &mut effective_sym_table);
    register_fn_addr_syms(&cfg.funcs, &layout.fn_vaddrs, &mut effective_sym_table);
    apply_user_vtable_overrides(&layout.user_vtables_layout, &mut effective_sym_table);
    apply_user_class_layouts_overrides(
        &layout.data_const_layout.class_layouts_layout,
        &mut effective_sym_table,
    );
    apply_fn_name_table_overrides(&layout.fn_name_table_layout, &mut effective_sym_table);
    apply_class_name_table_overrides(&layout.class_name_table_layout, &mut effective_sym_table);

    // chunk 2b-4: dyld-rebase member `__DATA,*` slots under ASLR.
    let data_rebase_targets = compute_member_data_rebase_targets(
        &layout,
        &merged,
        &effective_sym_table,
        layout.data_vmaddr,
    )
    .map_err(|err| ArchiveLayoutError::MemberReloc {
        archive_idx: 0,
        member_idx: 0,
        err,
    })?;
    let data_rebase_link_values =
        recompute_chained_fixups_with_data_rebase(&mut layout, &data_rebase_targets);

    // e7b-4/e8-2b + Step 3b.4-5 + W-J A3c: 4-way split
    // text_rebase_link_values = vtable | class_layouts |
    // fn_name_table | class_name_table. class_layouts count is the
    // middle slice computed by subtraction so the split arithmetic
    // stays single-source-of-truth even if other counts drift.
    let total_text_rebase = layout.text_rebase_link_values.len();
    let vtable_count = layout.vtable_rebase_target_count;
    let fn_name_count = layout.fn_name_rebase_target_count;
    let class_name_count = layout.class_name_rebase_target_count;
    let class_count = total_text_rebase
        .checked_sub(vtable_count + fn_name_count + class_name_count)
        .expect("text_rebase_link_values has fewer entries than the 3 fixed regions combined");
    let (vtable_lv, rest) = layout.text_rebase_link_values.split_at(vtable_count);
    let (class_lv, rest) = rest.split_at(class_count);
    let (fn_name_lv, class_name_lv) = rest.split_at(fn_name_count);
    debug_assert_eq!(class_name_lv.len(), class_name_count);
    let user_vtables_payload = build_user_vtables_payload(&layout.user_vtables_layout, vtable_lv);
    let user_class_layouts_payload =
        build_user_class_layouts_payload(&layout.data_const_layout.class_layouts_layout, class_lv);
    let fn_name_table_payload =
        build_fn_name_table_payload(&layout.fn_name_table_layout, fn_name_lv);
    let class_name_table_payload =
        build_class_name_table_payload(&layout.class_name_table_layout, class_name_lv);
    let resolved = apply_relocs(&cfg.funcs, &layout.fn_vaddrs, &effective_sym_table);

    // S7-C5 — patch each member's __text in place against effective sym table.
    let mut member_text_payloads: Vec<Vec<u8>> = Vec::with_capacity(layout.member_layouts.len());
    let mut non_text_payloads: Vec<Vec<u8>> = Vec::new();
    for (mi, m) in layout.member_layouts.iter().enumerate() {
        let member = &merged.per_archive_members[m.key.0][m.key.1];
        let off = m.member_text_offset_in_member as usize;
        let end = off + m.text_size as usize;
        let mut bytes = member.data[off..end].to_vec();
        apply_member_relocs(
            &mut bytes,
            member,
            m.vaddr,
            &effective_sym_table,
            &m.non_text_sections,
            member_data_layouts(&layout, mi),
        )
        .map_err(|err| ArchiveLayoutError::MemberReloc {
            archive_idx: m.key.0,
            member_idx: m.key.1,
            err,
        })?;
        member_text_payloads.push(bytes);
        // SD-4c-prereq-c — slice each `__TEXT,*` non-text section's
        // bytes out of the member, in compute_non_text_layouts order.
        for s in &m.non_text_sections {
            let off = s.member_internal_offset as usize;
            let end = off + s.size as usize;
            non_text_payloads.push(member.data[off..end].to_vec());
        }
    }

    let data_non_text_payloads =
        collect_member_data_payloads(&layout, &merged, &data_rebase_link_values)?;

    let bytes = emit_binary(
        cfg,
        &layout,
        &member_text_payloads,
        &non_text_payloads,
        &data_non_text_payloads,
        &resolved,
        &user_vtables_payload,
        &user_class_layouts_payload,
        &fn_name_table_payload,
        &class_name_table_payload,
    );
    Ok(bytes)
}

/// Concatenate header + LCs + __text + __LINKEDIT (nlist, strtab,
/// codesign blob). Pure byte assembly — layout from `ArchiveLayout`.
fn emit_binary(
    cfg: &LinkConfig,
    layout: &ArchiveLayout,
    member_text_payloads: &[Vec<u8>],
    non_text_payloads: &[Vec<u8>],
    data_non_text_payloads: &[Vec<u8>],
    resolved: &[crate::resolve::ResolvedFunction],
    user_vtables_payload: &[u8],
    user_class_layouts_payload: &[u8],
    fn_name_table_payload: &[u8],
    class_name_table_payload: &[u8],
) -> Vec<u8> {
    let EmitLcMeta {
        has_dyld,
        has_chained_fixups,
        has_libcurl_lc,
        has_libsystem_lc,
        extra_lc_count,
        extra_lc_size,
        mh_flags,
    } = compute_emit_lc_meta(layout);

    let header = MachHeader64 {
        magic: MH_MAGIC_64,
        cputype: CPU_TYPE_ARM64,
        cpusubtype: CPU_SUBTYPE_ARM64_ALL,
        filetype: MH_EXECUTE,
        ncmds: 9 + extra_lc_count,
        sizeofcmds: SEGMENT_COMMAND_64_SIZE * 3
            + SECTION_64_SIZE
            + LOAD_DYLINKER_CMDSIZE
            + extra_lc_size
            + BUILD_VERSION_CMDSIZE
            + MAIN_CMDSIZE
            + SYMTAB_COMMAND_SIZE
            + DYSYMTAB_CMDSIZE
            + LINKEDIT_DATA_CMDSIZE,
        flags: mh_flags,
        reserved: 0,
    };

    let pagezero = SegmentCommand64 {
        segname: "__PAGEZERO".into(),
        vmaddr: 0,
        vmsize: PAGEZERO_VMSIZE,
        fileoff: 0,
        filesize: 0,
        maxprot: 0,
        initprot: 0,
        flags: 0,
        sections: Vec::new(),
    };

    let text_section = Section64 {
        sectname: "__text".into(),
        segname: "__TEXT".into(),
        addr: layout.text_vmaddr + u64::from(layout.text_file_offset),
        size: u64::from(layout.text_size),
        offset: layout.text_file_offset,
        align_log2: 2,
        reloff: 0,
        nreloc: 0,
        flags: S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS,
        reserved1: 0,
        reserved2: 0,
        reserved3: 0,
    };
    // SD-2a: add __stubs section to __TEXT when has_libsystem.
    let text_sections = if has_dyld {
        vec![text_section, build_stubs_section(layout)]
    } else {
        vec![text_section]
    };
    let text_segment = SegmentCommand64 {
        segname: "__TEXT".into(),
        vmaddr: layout.text_vmaddr,
        vmsize: layout.text_vmsize,
        fileoff: 0,
        filesize: layout.text_vmsize,
        maxprot: VM_PROT_READ | VM_PROT_EXECUTE,
        initprot: VM_PROT_READ | VM_PROT_EXECUTE,
        flags: 0,
        sections: text_sections,
    };

    // __DATA (has_dyld): `__la_symbol_ptr` first, then member `__DATA,*`.
    let data_segment_opt = has_dyld.then(|| {
        let mut extra_sections = build_data_non_text_section_64_entries(layout);
        if layout.user_data_globals_layout.total_vmsize > 0 {
            extra_sections.push(crate::dyld_emit::build_user_bss_section(layout));
        }
        build_data_segment(layout, extra_sections)
    });

    // SD-4c-prereq+e8: __DATA_CONST seg between __TEXT and __DATA.
    let data_const_segment_opt =
        crate::data_const_layout::build_data_const_segment(&layout.data_const_layout);

    // __LINKEDIT covers chain blob + 8-byte pad past codesign end.
    let linkedit_data_size = u64::from(layout.codesign_dataoff + layout.codesign_datasize)
        - u64::from(layout.linkedit_file_offset);
    let linkedit_segment = SegmentCommand64 {
        segname: "__LINKEDIT".into(),
        vmaddr: layout.linkedit_vmaddr,
        vmsize: layout.linkedit_vmsize,
        fileoff: u64::from(layout.linkedit_file_offset),
        filesize: linkedit_data_size,
        maxprot: VM_PROT_READ,
        initprot: VM_PROT_READ,
        flags: 0,
        sections: Vec::new(),
    };

    let symtab_cmd = SymtabCommand {
        symoff: layout.symoff,
        nsyms: layout.nsyms,
        stroff: layout.stroff,
        strsize: layout.strsize,
    };

    let mut buf: Vec<u8> = Vec::with_capacity(layout.total_size as usize);

    header.write_to(&mut buf);
    pagezero.write_to(&mut buf);
    text_segment.write_to(&mut buf);
    // e8: __DATA_CONST → __DATA → __LINKEDIT in segment-vmaddr order.
    if let Some(ref s) = data_const_segment_opt {
        s.write_to(&mut buf);
    }
    if let Some(ref data_segment) = data_segment_opt {
        data_segment.write_to(&mut buf);
    }
    linkedit_segment.write_to(&mut buf);
    write_load_dylinker(&mut buf);
    // LC_LOAD_DYLIB: libSystem ord 1, libcurl ord 2 (matches chain enc).
    if has_libsystem_lc {
        write_load_dylib_libsystem(&mut buf);
    }
    if has_libcurl_lc {
        write_load_dylib_libcurl(&mut buf);
    }
    write_build_version(&mut buf);
    write_lc_main(&mut buf, u64::from(layout.entry_file_offset));
    symtab_cmd.write_to(&mut buf);
    write_dysymtab(&mut buf, layout.nsyms);
    if has_chained_fixups {
        write_dyld_chained_fixups(
            &mut buf,
            layout.chained_fixups_dataoff,
            layout.chained_fixups_datasize,
        );
    }
    write_code_signature(&mut buf, layout.codesign_dataoff, layout.codesign_datasize);

    debug_assert_eq!(
        (buf.len() as u32) - (MachHeader64::SIZE as u32),
        header.sizeofcmds,
        "load command size mismatch",
    );

    pad_to(&mut buf, layout.text_file_offset as usize);

    // __text payload: user funcs (resolved) followed by each
    // integrated member's patched __text slice (S7-C5).
    for r in resolved {
        buf.extend_from_slice(&r.bytes);
    }
    debug_assert_eq!(
        member_text_payloads.len(),
        layout.member_layouts.len(),
        "patched payload count must match member_layouts",
    );
    for (i, m) in layout.member_layouts.iter().enumerate() {
        debug_assert_eq!(
            member_text_payloads[i].len() as u32,
            m.text_size,
            "patched member __text size must match layout",
        );
        buf.extend_from_slice(&member_text_payloads[i]);
    }

    // Non-text section payloads + user-strings region splice between
    // member __texts and `__TEXT,__stubs` (SD-4c-prereq-c / +e1).
    crate::non_text_layout::write_non_text_payloads(&mut buf, layout, non_text_payloads);
    buf.extend_from_slice(&layout.user_strings_payload);

    // e8: __TEXT __stubs → __DATA_CONST (vtable) → __DATA la_ptr.
    if has_dyld {
        // chunk 3 — pad to `stubs_file_offset` past per-section align.
        pad_to(&mut buf, layout.stubs_file_offset as usize);
        write_stubs_section(&mut buf, layout);
    }
    crate::data_const_layout::write_data_const_payload(
        &mut buf,
        &layout.data_const_layout,
        user_vtables_payload,
        user_class_layouts_payload,
        fn_name_table_payload,
        class_name_table_payload,
    );
    if has_dyld {
        write_la_ptr_section(&mut buf, layout);
        write_data_non_text_file_payloads(&mut buf, layout, data_non_text_payloads);
        crate::tlv_thunk_emit::patch_tlv_thunk_slots(&mut buf, layout);
    }

    pad_to(&mut buf, layout.linkedit_file_offset as usize);

    // nlist + strtab: user fns first, then every member's
    // defined externs in member_layouts order.
    let mut strtab = StringTable::new();
    let mut user_strx: Vec<u32> = Vec::with_capacity(cfg.funcs.len());
    for f in &cfg.funcs {
        user_strx.push(strtab.add(&f.name));
    }
    let mut member_strx: Vec<Vec<u32>> = Vec::with_capacity(layout.member_layouts.len());
    for m in &layout.member_layouts {
        let mut row: Vec<u32> = Vec::with_capacity(m.defined_syms.len());
        for (name, _, _) in &m.defined_syms {
            row.push(strtab.add(name));
        }
        member_strx.push(row);
    }
    // Emit nlist entries in the same order strings were added.
    for (i, _f) in cfg.funcs.iter().enumerate() {
        let nlist = Nlist64::defined_extern(user_strx[i], 1, layout.fn_vaddrs[i]);
        nlist.write_to(&mut buf);
    }
    for (mi, m) in layout.member_layouts.iter().enumerate() {
        let data_layouts = member_data_layouts(&layout, mi);
        for (si, (_name, n_value, n_sect)) in m.defined_syms.iter().enumerate() {
            let vaddr = section_vaddr_for_sym(
                *n_sect,
                *n_value,
                m.vaddr,
                &m.non_text_sections,
                data_layouts,
            );
            let nlist = Nlist64::defined_extern(member_strx[mi][si], 1, vaddr);
            nlist.write_to(&mut buf);
        }
    }
    buf.extend_from_slice(strtab.as_bytes());

    // SD-3 + e8: chained-fixups blob lands after strtab (8-byte
    // aligned — dyld rejects unaligned chains) and before codesign.
    if has_chained_fixups {
        pad_to(&mut buf, layout.chained_fixups_dataoff as usize);
        debug_assert_eq!(buf.len() as u32, layout.chained_fixups_dataoff);
        buf.extend_from_slice(&layout.chained_fixups_blob);
        debug_assert_eq!(
            (buf.len() as u32) - layout.chained_fixups_dataoff,
            layout.chained_fixups_datasize,
        );
    }

    // SD-4c-prereq+c: codesign blob also 8-byte aligned — pad any
    // gap left by chained_fixups_datasize not being a multiple of 8.
    pad_to(&mut buf, layout.codesign_dataoff as usize);
    debug_assert_eq!(buf.len() as u32, layout.codesign_dataoff);
    let blob = build_adhoc_codesign_blob(&buf, &cfg.codesign_ident);
    debug_assert_eq!(blob.len() as u32, layout.codesign_datasize);
    buf.extend_from_slice(&blob);

    debug_assert_eq!(
        buf.len() as u32,
        layout.total_size,
        "emitted file size must match layout total"
    );

    buf
}

/// Per-member `__DATA,*` placement slice (empty when the member has
/// no data sections — pre-2e behaviour).
fn member_data_layouts(layout: &ArchiveLayout, m_idx: usize) -> &[DataSectionLayout] {
    layout
        .data_non_text_layouts
        .get(m_idx)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn pad_to(buf: &mut Vec<u8>, target_len: usize) {
    if buf.len() < target_len {
        buf.resize(target_len, 0);
    }
    debug_assert_eq!(buf.len(), target_len, "buf grew past target_len");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::{AR_HEADER_SIZE, AR_MAGIC};
    use crate::resolve::SymTable;
    use torajs_codegen::CompiledFunction;
    use torajs_codegen::compile_function;
    use torajs_codegen::frame::FrameLayout;
    use torajs_codegen::reloc::{CallTarget, Reloc, RelocKind};
    use torajs_core::ssa::{
        BinOp, Block, BlockId, Function, Inst, InstKind, Operand, Terminator, Type, ValueId,
        ValueInfo,
    };

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

    /// Build a SSA `Function` named `name` that returns the
    /// constant `value` (i64), via codegen → real aarch64
    /// instruction bytes. The compiled function ends with a
    /// proper `ret` so when integrated via archive its leaf
    /// position is honoured.
    fn make_ret_const_fn(name: &str, value: i64) -> Function {
        let v0 = ValueId(0);
        Function {
            name: name.into(),
            params: Vec::new(),
            ret: Type::I64,
            values: vec![ValueInfo {
                ty: Type::I64,
                name: Some("v0".into()),
            }],
            blocks: vec![Block {
                id: BlockId(0),
                insts: vec![Inst {
                    result: Some(v0),
                    kind: InstKind::BinOp(
                        BinOp::Add,
                        Operand::ConstI64(value),
                        Operand::ConstI64(0),
                    ),
                    origin: None,
                }],
                term: Terminator::Ret(Some(Operand::Value(v0))),
            }],
            current_origin: None,
        }
    }

    /// Hand-build a `CompiledFunction` whose body is the canonical
    /// AAPCS64 non-leaf prologue + `bl <extern>` + epilogue. The
    /// prologue saves LR before the call so the `ret` at the end
    /// returns to the caller (kernel-side dyld trampoline for the
    /// entry function) instead of looping back into the prologue.
    ///
    /// Instructions (LE 4-byte each):
    ///   stp x29, x30, [sp, #-16]!   ; FD 7B BF A9
    ///   bl  <extern>                ; 00 00 00 94 — placeholder
    ///   ldp x29, x30, [sp], #16     ; FD 7B C1 A8
    ///   ret                         ; C0 03 5F D6
    fn fn_calls_extern_then_returns(name: &str, extern_name: &str) -> CompiledFunction {
        let stp_x29_x30_pre: [u8; 4] = [0xFD, 0x7B, 0xBF, 0xA9];
        let bl_placeholder: [u8; 4] = [0x00, 0x00, 0x00, 0x94];
        let ldp_x29_x30_post: [u8; 4] = [0xFD, 0x7B, 0xC1, 0xA8];
        let ret_bytes: [u8; 4] = [0xC0, 0x03, 0x5F, 0xD6];
        let mut bytes = Vec::with_capacity(16);
        bytes.extend_from_slice(&stp_x29_x30_pre);
        bytes.extend_from_slice(&bl_placeholder);
        bytes.extend_from_slice(&ldp_x29_x30_post);
        bytes.extend_from_slice(&ret_bytes);
        CompiledFunction {
            name: name.into(),
            bytes,
            relocs: vec![Reloc {
                byte_offset: 4,
                kind: RelocKind::CallSite {
                    target: CallTarget::Extern(extern_name.into()),
                },
            }],
            frame: FrameLayout::leaf_no_spill(),
        }
    }

    /// Empty `cfg.archives` → identical binary to `link_to_exec`.
    /// The archive-aware path must be a strict superset of the
    /// baseline.
    #[test]
    fn empty_archives_byte_equal_to_baseline() {
        use crate::exec::link_to_exec;
        let main_fn = compile_function(&make_ret_const_fn("_main", 42));
        let cfg = LinkConfig {
            funcs: vec![main_fn],
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
        };
        let archive_bytes = link_to_exec_with_archives(&cfg).unwrap();
        let baseline_bytes = link_to_exec(&cfg);
        assert_eq!(
            archive_bytes, baseline_bytes,
            "empty-archives output must match baseline byte-for-byte"
        );
    }

    /// End-to-end: user `_main` BL's an extern `_foo` defined by
    /// a single-member archive (`_foo` returns 7). After link +
    /// exec the process must exit with code 7. macOS arm64 only.
    #[test]
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    fn end_to_end_user_calls_archive_member_exits_7() {
        use std::os::unix::fs::PermissionsExt;
        use std::process::Command;

        // _foo: `fn _foo() -> i64 { 7 }` via codegen.
        let foo_cf = compile_function(&make_ret_const_fn("_foo", 7));
        let archive = build_short_name_archive(
            "a.o",
            &torajs_obj::write_object(std::slice::from_ref(&foo_cf)),
        );

        // _main: `bl _foo; ret` — the BL site picks up _foo's
        // return value in X0 by the AAPCS64 calling convention,
        // and _main's `ret` propagates X0 to the kernel as the
        // process exit code.
        let main_cf = fn_calls_extern_then_returns("_main", "_foo");

        let cfg = LinkConfig {
            funcs: vec![main_cf],
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
        };
        let bytes = link_to_exec_with_archives(&cfg).expect("link_to_exec_with_archives");

        let path = format!("/private/tmp/torajs_link_s7c4_{}", std::process::id());
        std::fs::write(&path, &bytes).expect("write binary");
        let mut perms = std::fs::metadata(&path).expect("stat").permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).expect("chmod +x");

        let run = Command::new(&path).output().expect("exec emitted binary");
        let exit = run.status.code();
        let _ = std::fs::remove_file(&path);
        assert_eq!(
            exit,
            Some(7),
            "binary did not exit 7 — stdout={:?} stderr={:?}",
            String::from_utf8_lossy(&run.stdout),
            String::from_utf8_lossy(&run.stderr)
        );
    }

    /// End-to-end transitive chain — `_main` BL's archive_a's
    /// `_foo`, which BL's archive_b's `_bar` (the leaf returning 7).
    /// _foo's `bl _bar` site is patched by S7-C5's
    /// `apply_member_relocs` against the effective sym table
    /// (which now resolves _bar to archive_b's final vaddr). Exec
    /// the result and the kernel-side X0 propagation must surface
    /// `_bar`'s return value as the process exit code. macOS arm64
    /// only.
    #[test]
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    fn end_to_end_transitive_member_chain_exits_7() {
        use std::os::unix::fs::PermissionsExt;
        use std::process::Command;

        // archive_b: _bar leaf returning 7 (via codegen).
        let bar_cf = compile_function(&make_ret_const_fn("_bar", 7));
        let archive_b = build_short_name_archive(
            "b.o",
            &torajs_obj::write_object(std::slice::from_ref(&bar_cf)),
        );

        // archive_a: _foo's body = canonical AAPCS64 non-leaf
        // prologue + `bl _bar` + epilogue + ret. _foo is not a
        // leaf — it must save LR before the BL so its own RET
        // returns up the chain rather than looping back into the
        // prologue. The internal BL placeholder + reloc against
        // extern _bar is what S7-C5 patches.
        let foo_cf = fn_calls_extern_then_returns("_foo", "_bar");
        let archive_a = build_short_name_archive(
            "a.o",
            &torajs_obj::write_object(std::slice::from_ref(&foo_cf)),
        );

        // _main: canonical AAPCS64 non-leaf wrapping `bl _foo`.
        let main_cf = fn_calls_extern_then_returns("_main", "_foo");

        let cfg = LinkConfig {
            funcs: vec![main_cf],
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
        };
        let bytes = link_to_exec_with_archives(&cfg).expect("link_to_exec_with_archives");

        let path = format!("/private/tmp/torajs_link_s7c5_{}", std::process::id());
        std::fs::write(&path, &bytes).expect("write binary");
        let mut perms = std::fs::metadata(&path).expect("stat").permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).expect("chmod +x");

        let run = Command::new(&path).output().expect("exec emitted binary");
        let exit = run.status.code();
        let _ = std::fs::remove_file(&path);
        assert_eq!(
            exit,
            Some(7),
            "binary did not exit 7 — stdout={:?} stderr={:?}",
            String::from_utf8_lossy(&run.stdout),
            String::from_utf8_lossy(&run.stderr),
        );
    }

    /// SD-3 — when a user fn references a libSystem-resolved
    /// extern (`_malloc`), the emitted binary must:
    ///   - grow by exactly the `__stubs` payload (12 B per import)
    ///     plus a page-aligned `__DATA` segment carrying the
    ///     `__la_symbol_ptr` slot
    ///   - place the `__stubs` payload right after the user
    ///     `__text` inside the `__TEXT` segment
    ///   - encode the `__la_symbol_ptr` slot as a
    ///     `dyld_chained_ptr_64_bind` link (no longer zero-init —
    ///     SD-3 wires LC_DYLD_CHAINED_FIXUPS to bind it)
    ///   - report `ncmds = 12` (PAGEZERO + TEXT + DATA + LINKEDIT
    ///     segments + DYLINKER + LOAD_DYLIB + BUILD_VERSION +
    ///     LC_MAIN + SYMTAB + DYSYMTAB + DYLD_CHAINED_FIXUPS +
    ///     CODE_SIGNATURE)
    ///
    /// The test populates `cfg.sym_table` with a within-page
    /// `_malloc` address so `apply_relocs` can patch the BL
    /// displacement without panicking. SD-2b already routed
    /// `_malloc` through `layout.stub_vaddrs` automatically, but
    /// the hand-supplied entry stays for backwards-compat
    /// coverage of the explicit-sym_table code path.
    #[test]
    fn libsystem_extern_emits_stubs_section_and_la_ptr_slot() {
        // user `_main` does `stp; bl _malloc; ldp; ret`.
        let main_cf = fn_calls_extern_then_returns("_main", "_malloc");
        let mut sym_table = SymTable::new();
        // _main lands at TEXT_VMADDR_BASE + text_file_offset =
        // 0x100000000 + 0x4000 = 0x100004000. Point `_malloc` at
        // the same fn's epilogue so patch_branch26's debug-assert
        // is happy; the runtime never executes this binary in
        // this test.
        let fake_malloc_vaddr = 0x1_0000_4000u64 + (main_cf.bytes.len() as u64 - 4);
        sym_table.insert("_malloc".into(), fake_malloc_vaddr);

        let cfg = LinkConfig {
            funcs: vec![main_cf.clone()],
            entry: "_main".into(),
            sym_table,
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
        };
        let layout = compute_archive_layout(&cfg).expect("layout");
        assert!(!layout.dyld_imports.is_empty(), "dyld_imports populated");
        assert_eq!(layout.stubs_section_size, 12);
        assert_eq!(layout.la_ptr_section_size, 8);

        // Drive emit_binary directly — apply_relocs is happy
        // because cfg.sym_table covers `_malloc`.
        let resolved = apply_relocs(&cfg.funcs, &layout.fn_vaddrs, &cfg.sym_table);
        let bytes = emit_binary(&cfg, &layout, &[], &[], &[], &resolved, &[], &[], &[], &[]);

        assert_eq!(
            bytes.len() as u32,
            layout.total_size,
            "emitted size must match layout total"
        );

        // __stubs payload sits at stubs_file_offset, 12 bytes.
        let stubs_off = layout.stubs_file_offset as usize;
        let stubs = &bytes[stubs_off..stubs_off + 12];
        let stub_bytes_match = crate::stubs::build_stubs(
            &layout.dyld_imports,
            layout.stubs_section_vaddr,
            layout.la_ptr_section_vaddr,
        );
        assert_eq!(stub_bytes_match.len(), 1);
        assert_eq!(stubs, &stub_bytes_match[0].bytes);

        // SD-3 — __la_symbol_ptr slot now carries the
        // dyld_chained_ptr_64_bind encoding (bind=1, ordinal=0,
        // next=0 for single-import chain end). The byte pattern
        // matches `0x8000_0000_0000_0000` LE.
        let slot_off = layout.la_ptr_file_offset as usize;
        let slot = &bytes[slot_off..slot_off + 8];
        let expected_link = 0x8000_0000_0000_0000u64;
        assert_eq!(slot, &expected_link.to_le_bytes());
        assert_eq!(
            layout.la_ptr_slot_values,
            vec![expected_link],
            "la_ptr_slot_values must match the on-disk encoding",
        );

        // Header reflects the SD-3 LC chain: 12 cmds total
        // (PAGEZERO + TEXT + DATA + LINKEDIT segments = 4,
        // plus DYLINKER + LOAD_DYLIB + BUILD_VERSION + LC_MAIN +
        // SYMTAB + DYSYMTAB + DYLD_CHAINED_FIXUPS +
        // CODE_SIGNATURE = 8 → 12 total).
        let ncmds = u32::from_le_bytes(bytes[16..20].try_into().unwrap());
        assert_eq!(ncmds, 12);

        // SD-3 — the chained_fixups blob must land exactly at
        // layout.chained_fixups_dataoff and match the buffer the
        // layout cached.
        let cf_off = layout.chained_fixups_dataoff as usize;
        let cf_size = layout.chained_fixups_datasize as usize;
        assert_eq!(
            &bytes[cf_off..cf_off + cf_size],
            &layout.chained_fixups_blob[..]
        );
    }

    /// SD-2b — a user fn referencing a libSystem-resolved extern
    /// (`_malloc`) links successfully against an empty
    /// `cfg.sym_table`: `compute_required_members` routes
    /// `_malloc` into `dyld_imports`, `compute_archive_layout`
    /// allocates a `__stubs` trampoline, and
    /// `link_to_exec_with_archives` funnels that stub vaddr
    /// through `effective_sym_table` so `apply_relocs` patches
    /// the user fn's BL displacement onto the trampoline.
    ///
    /// The patched BL displacement must equal
    /// `stub_vaddr - bl_site_vaddr`, matching what
    /// `patch_branch26` would have written by hand. Exec is NOT
    /// asserted here — the `__la_symbol_ptr` slot is still
    /// zero-init until SD-3 wires `LC_DYLD_CHAINED_FIXUPS`, so
    /// any real run would jump through a null pointer. Link
    /// success + correct patched displacement = SD-2b
    /// acceptance.
    #[test]
    fn libsystem_extern_routes_through_stub_trampoline() {
        let main_cf = fn_calls_extern_then_returns("_main", "_malloc");
        let cfg = LinkConfig {
            funcs: vec![main_cf.clone()],
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
        };
        // SD-2b — link must succeed even though cfg.sym_table is
        // empty. SD-2a's plumbing populates `layout.stub_vaddrs`;
        // SD-2b's effective_sym_table extension forwards those
        // into apply_relocs.
        let bytes = link_to_exec_with_archives(&cfg).expect("link must succeed via stub routing");
        let layout = compute_archive_layout(&cfg).expect("layout");
        assert_eq!(layout.dyld_imports.len(), 1);
        assert!(layout.stub_vaddrs.contains_key("_malloc"));

        // _main lands at vaddr = TEXT_VMADDR_BASE + text_file_offset.
        let main_vaddr = crate::lc::TEXT_VMADDR_BASE + u64::from(layout.text_file_offset);
        // BL site offset within _main = 4 (after the stp prologue).
        let bl_site_vaddr = main_vaddr + 4;
        let stub_vaddr = layout.stub_vaddrs["_malloc"];
        let expected_disp = stub_vaddr as i64 - bl_site_vaddr as i64;
        // The patched BL instruction sits at file offset
        // text_file_offset + 4. Extract its imm26 and reconstruct
        // the displacement = imm26 << 2.
        let bl_off = layout.text_file_offset as usize + 4;
        let bl_word = u32::from_le_bytes(bytes[bl_off..bl_off + 4].try_into().unwrap());
        let imm26 = (bl_word & 0x03FF_FFFF) as i32;
        // Sign-extend to 32 bits.
        let imm26_se = (imm26 << 6) >> 6;
        let patched_disp = (imm26_se as i64) * 4;
        assert_eq!(
            patched_disp, expected_disp,
            "BL displacement must point at stub trampoline (expected {expected_disp:#X}, got {patched_disp:#X})",
        );
    }

    /// SD-2b — the user fn now refers to a libSystem symbol but
    /// the worklist also pulls archive members; the merged
    /// effective sym table must cover both populations (member
    /// defined_syms and stub_vaddrs) so both reloc kinds resolve.
    #[test]
    fn member_and_libsystem_externs_coexist_in_effective_sym_table() {
        // _main calls archive's `_foo` AND libSystem's `_malloc`.
        let stp_pre: [u8; 4] = [0xFD, 0x7B, 0xBF, 0xA9];
        let bl_placeholder: [u8; 4] = [0x00, 0x00, 0x00, 0x94];
        let ldp_post: [u8; 4] = [0xFD, 0x7B, 0xC1, 0xA8];
        let ret_bytes: [u8; 4] = [0xC0, 0x03, 0x5F, 0xD6];
        let mut bytes = Vec::with_capacity(24);
        bytes.extend_from_slice(&stp_pre); // 0
        bytes.extend_from_slice(&bl_placeholder); // 4 — bl _foo
        bytes.extend_from_slice(&bl_placeholder); // 8 — bl _malloc
        bytes.extend_from_slice(&ldp_post); // 12
        bytes.extend_from_slice(&ret_bytes); // 16
        let main_cf = CompiledFunction {
            name: "_main".into(),
            bytes,
            relocs: vec![
                Reloc {
                    byte_offset: 4,
                    kind: RelocKind::CallSite {
                        target: CallTarget::Extern("_foo".into()),
                    },
                },
                Reloc {
                    byte_offset: 8,
                    kind: RelocKind::CallSite {
                        target: CallTarget::Extern("_malloc".into()),
                    },
                },
            ],
            frame: FrameLayout::leaf_no_spill(),
        };

        let foo_cf = compile_function(&make_ret_const_fn("_foo", 7));
        let archive = build_short_name_archive(
            "a.o",
            &torajs_obj::write_object(std::slice::from_ref(&foo_cf)),
        );
        let cfg = LinkConfig {
            funcs: vec![main_cf],
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
        };
        let link_bytes =
            link_to_exec_with_archives(&cfg).expect("link must succeed against mixed externs");
        // Sanity — the binary must include the __stubs section
        // (libSystem reference) AND the archive member's __text.
        let layout = compute_archive_layout(&cfg).expect("layout");
        assert_eq!(layout.member_layouts.len(), 1);
        assert!(layout.dyld_imports.contains_key("_malloc"));
        assert_eq!(link_bytes.len() as u32, layout.total_size);
    }

    /// nm sanity check: integrated member's `_foo` shows up in
    /// the binary's symbol table as a defined-extern (type `T`).
    /// macOS arm64 only (requires Xcode's `nm`).
    #[test]
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    fn nm_reports_integrated_member_symbol() {
        use std::process::Command;

        let foo_cf = compile_function(&make_ret_const_fn("_foo", 7));
        let archive = build_short_name_archive(
            "a.o",
            &torajs_obj::write_object(std::slice::from_ref(&foo_cf)),
        );
        let main_cf = fn_calls_extern_then_returns("_main", "_foo");

        let cfg = LinkConfig {
            funcs: vec![main_cf],
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
        };
        let bytes = link_to_exec_with_archives(&cfg).unwrap();

        let path = format!("/private/tmp/torajs_link_s7c4nm_{}", std::process::id());
        std::fs::write(&path, &bytes).unwrap();

        let nm_out = match Command::new("/usr/bin/nm").arg(&path).output() {
            Ok(o) => o,
            Err(e) => {
                eprintln!("skip: nm not invokable: {e}");
                let _ = std::fs::remove_file(&path);
                return;
            }
        };
        let stdout = String::from_utf8_lossy(&nm_out.stdout).into_owned();
        let _ = std::fs::remove_file(&path);
        assert!(
            stdout.contains("_main"),
            "nm did not list _main; output:\n{stdout}"
        );
        assert!(
            stdout.contains("_foo"),
            "nm did not list _foo (from archive member); output:\n{stdout}"
        );
    }
}
