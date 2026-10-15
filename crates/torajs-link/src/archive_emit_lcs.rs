//! Load-command emission + symtab/strtab halves of `emit_binary`,
//! split out of `archive_emit.rs` (chunk 456) to keep that file under
//! the 500-line prod limit. Byte sequences verbatim — probe smoke
//! (6 probes byte-count) is the ground truth for this file.

use torajs_codegen::is_local_label;
use torajs_obj::{
    CPU_SUBTYPE_ARM64_ALL, CPU_TYPE_ARM64, MH_MAGIC_64, MachHeader64, N_EXT, N_SECT, Nlist64,
    S_ATTR_PURE_INSTRUCTIONS, S_ATTR_SOME_INSTRUCTIONS, SECTION_64_SIZE, SEGMENT_COMMAND_64_SIZE,
    SYMTAB_COMMAND_SIZE, Section64, SegmentCommand64, StringTable, SymtabCommand, VM_PROT_EXECUTE,
    VM_PROT_READ,
};

use crate::archive_emit::member_data_layouts;
use crate::archive_emit_lc_meta::{EmitLcMeta, compute_emit_lc_meta};
use crate::archive_link::ArchiveLayout;
use crate::data_section_emit::build_data_non_text_section_64_entries;
use crate::defined_extern_resolve::section_vaddr_for_sym;
use crate::dyld_emit::{build_data_segment, build_stubs_section};
use crate::exec::LinkConfig;
use crate::lc::{
    BUILD_VERSION_CMDSIZE, DYSYMTAB_CMDSIZE, LINKEDIT_DATA_CMDSIZE, LOAD_DYLINKER_CMDSIZE,
    MAIN_CMDSIZE, MH_EXECUTE, PAGEZERO_VMSIZE, write_build_version, write_code_signature,
    write_dyld_chained_fixups, write_dysymtab, write_lc_main, write_load_dylib_libcurl,
    write_load_dylib_libsystem, write_load_dylinker,
};

/// Mach-O header + all load commands (segments, dylinker, dylibs,
/// build-version, main, symtab, dysymtab, chained-fixups, codesign
/// LCs), asserted against `layout`'s sizeofcmds. Byte order matches
/// the original inline sequence exactly. Returns every section's
/// start address in ordinal order (`n_sect` = index + 1).
pub(crate) fn write_header_and_load_commands(
    buf: &mut Vec<u8>,
    layout: &ArchiveLayout,
) -> Vec<u64> {
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
        align_log2: crate::text_payload::TEXT_FN_ALIGN.trailing_zeros(),
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

    header.write_to(buf);
    pagezero.write_to(buf);
    text_segment.write_to(buf);
    // e8: __DATA_CONST → __DATA → __LINKEDIT in segment-vmaddr order.
    if let Some(ref s) = data_const_segment_opt {
        s.write_to(buf);
    }
    if let Some(ref data_segment) = data_segment_opt {
        data_segment.write_to(buf);
    }
    linkedit_segment.write_to(buf);
    write_load_dylinker(buf);
    // LC_LOAD_DYLIB: libSystem ord 1, libcurl ord 2 (matches chain enc).
    if has_libsystem_lc {
        write_load_dylib_libsystem(buf);
    }
    if has_libcurl_lc {
        write_load_dylib_libcurl(buf);
    }
    write_build_version(buf);
    write_lc_main(buf, u64::from(layout.entry_file_offset));
    symtab_cmd.write_to(buf);
    let nlocalsym: u32 = (layout.member_layouts.iter())
        .map(|m| m.symtab.locals.len() as u32)
        .sum();
    write_dysymtab(buf, nlocalsym, layout.nsyms - nlocalsym);
    if has_chained_fixups {
        write_dyld_chained_fixups(
            buf,
            layout.chained_fixups_dataoff,
            layout.chained_fixups_datasize,
        );
    }
    write_code_signature(buf, layout.codesign_dataoff, layout.codesign_datasize);

    debug_assert_eq!(
        (buf.len() as u32) - (MachHeader64::SIZE as u32),
        header.sizeofcmds,
        "load command size mismatch",
    );

    let segments = [
        Some(&text_segment),
        data_const_segment_opt.as_ref(),
        data_segment_opt.as_ref(),
    ];
    segments
        .into_iter()
        .flatten()
        .flat_map(|seg| seg.sections.iter().map(|s| s.addr))
        .collect()
}

/// `n_sect` of the output section holding `vaddr`: the last section
/// starting at or below it (`sect_addrs` ascend, as segments and their
/// sections are laid out in address order).
fn section_ordinal(sect_addrs: &[u64], vaddr: u64) -> u8 {
    let n = sect_addrs.partition_point(|&a| a <= vaddr);
    debug_assert!(n > 0, "symbol vaddr 0x{vaddr:x} below the first section");
    n as u8
}

/// nlist + strtab. Under `cfg.strip_member_symbols` only the user
/// fns (bodies only — see [`symtab_rows`]). Otherwise every member's
/// listed locals first (`LC_DYSYMTAB` wants locals contiguous and
/// ahead), then the user fns, then every member's listed externs, in
/// member_layouts order; member rows carry the ordinal of the output
/// section their address lands in. The layout budgeted `nsyms` /
/// `strsize` from the same lists, so the two stay in lockstep.
pub(crate) fn write_symtab_and_strtab(
    buf: &mut Vec<u8>,
    cfg: &LinkConfig,
    layout: &ArchiveLayout,
    sect_addrs: &[u64],
) {
    if cfg.strip_member_symbols {
        write_user_symtab_only(buf, cfg, layout);
        return;
    }
    let mut strtab = StringTable::new();
    let member_row = |buf: &mut Vec<u8>,
                      strtab: &mut StringTable,
                      mi: usize,
                      row: &(String, u64, u8),
                      n_type: u8| {
        let m = &layout.member_layouts[mi];
        let (name, n_value, n_sect) = row;
        let vaddr = section_vaddr_for_sym(
            *n_sect,
            *n_value,
            m.vaddr,
            &m.non_text_sections,
            member_data_layouts(layout, mi),
        );
        Nlist64 {
            n_strx: strtab.add(name),
            n_type,
            n_sect: section_ordinal(sect_addrs, vaddr),
            n_desc: 0,
            n_value: vaddr,
        }
        .write_to(buf);
    };
    for (mi, m) in layout.member_layouts.iter().enumerate() {
        for row in &m.symtab.locals {
            member_row(buf, &mut strtab, mi, row, N_SECT);
        }
    }
    let user: Vec<(usize, u32)> = symtab_rows(cfg)
        .map(|(i, f)| (i, strtab.add(&f.name)))
        .collect();
    for (i, strx) in user {
        Nlist64::defined_extern(strx, 1, layout.fn_vaddrs[i]).write_to(buf);
    }
    for (mi, m) in layout.member_layouts.iter().enumerate() {
        for row in &m.symtab.externs {
            member_row(buf, &mut strtab, mi, row, N_SECT | N_EXT);
        }
    }
    buf.extend_from_slice(strtab.as_bytes());
}

/// `strip_member_symbols` half of [`write_symtab_and_strtab`]: the
/// user program's functions are the whole symbol table.
fn write_user_symtab_only(buf: &mut Vec<u8>, cfg: &LinkConfig, layout: &ArchiveLayout) {
    let mut strtab = StringTable::new();
    let rows: Vec<(usize, u32)> = symtab_rows(cfg)
        .map(|(i, f)| (i, strtab.add(&f.name)))
        .collect();
    for (i, strx) in rows {
        Nlist64::defined_extern(strx, 1, layout.fn_vaddrs[i]).write_to(buf);
    }
    buf.extend_from_slice(strtab.as_bytes());
}

/// The `cfg.funcs` slots that own a symbol-table row: every user
/// function with a body, other than a local label in a stripped build
/// (an unstripped one keeps its row, so a sampled PC inside it is not
/// charged to the function laid out before it). Extern-declaration
/// slots (empty bytes — `compile_module_funcs` keeps them so
/// `FuncId`s stay stable) define nothing; a row for them would claim
/// the runtime symbol lives at the text start and mis-attribute crash
/// frames there. A compiler-supplied definition in the runtime's own
/// namespace (a dispatch or guarded stub, a synthesized allocator)
/// stands in for a runtime member's, so a stripped build drops its row
/// with the member rows.
pub(crate) fn symtab_rows(
    cfg: &LinkConfig,
) -> impl Iterator<Item = (usize, &torajs_codegen::CompiledFunction)> {
    cfg.funcs.iter().enumerate().filter(|(_, f)| {
        !f.bytes.is_empty()
            && !(cfg.strip_member_symbols
                && (is_local_label(&f.name) || f.name.starts_with(RUNTIME_SYM_PREFIX)))
    })
}

/// Mach-O spelling of the runtime's `__torajs_` symbol namespace.
const RUNTIME_SYM_PREFIX: &str = "___torajs_";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolve::SymTable;
    use torajs_codegen::CompiledFunction;
    use torajs_codegen::frame::FrameLayout;

    fn leaf(name: &str) -> CompiledFunction {
        CompiledFunction {
            name: name.into(),
            bytes: vec![0xc0, 0x03, 0x5f, 0xd6],
            relocs: Vec::new(),
            frame: FrameLayout::leaf_no_spill(),
        }
    }

    /// A local label and a definition in the runtime namespace lose
    /// their rows only when the build is stripped.
    #[test]
    fn local_label_rows_follow_the_strip_policy() {
        let mut cfg = LinkConfig {
            funcs: vec![leaf("_main"), leaf(".Lfib.int"), leaf("___torajs_hook")],
            entry: "_main".into(),
            sym_table: SymTable::new(),
            codesign_ident: "tora".into(),
            dead_strip: false,
            strip_member_symbols: false,
            elidable_sites: Vec::new(),
            guarded_stubs: Vec::new(),
            image: torajs_rtimage::image::build(&[]).unwrap().into(),
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
        let rows = |cfg: &LinkConfig| symtab_rows(cfg).map(|(i, _)| i).collect::<Vec<_>>();
        assert_eq!(rows(&cfg), vec![0, 1, 2]);
        cfg.strip_member_symbols = true;
        assert_eq!(rows(&cfg), vec![0]);
    }
}
