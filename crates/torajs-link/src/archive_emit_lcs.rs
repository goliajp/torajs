//! Load-command emission + symtab/strtab halves of `emit_binary`,
//! split out of `archive_emit.rs` (chunk 456) to keep that file under
//! the 500-line prod limit. Byte sequences verbatim — probe smoke
//! (6 probes byte-count) is the ground truth for this file.

use torajs_obj::{
    CPU_SUBTYPE_ARM64_ALL, CPU_TYPE_ARM64, MH_MAGIC_64, MachHeader64, Nlist64,
    S_ATTR_PURE_INSTRUCTIONS, S_ATTR_SOME_INSTRUCTIONS, SECTION_64_SIZE, SEGMENT_COMMAND_64_SIZE,
    SYMTAB_COMMAND_SIZE, Section64, SegmentCommand64, StringTable, SymtabCommand, VM_PROT_EXECUTE,
    VM_PROT_READ,
};

use crate::archive_emit::member_data_layouts;
use crate::archive_emit_lc_meta::{EmitLcMeta, compute_emit_lc_meta};
use crate::archive_link::{ArchiveLayout, symtab_rows};
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
/// the original inline sequence exactly.
pub(crate) fn write_header_and_load_commands(buf: &mut Vec<u8>, layout: &ArchiveLayout) {
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
    write_dysymtab(buf, layout.nsyms);
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
}

/// nlist + strtab: user fns first (bodies only — see
/// [`symtab_rows`]), then every member's defined externs in
/// member_layouts order (strings added in the same order the nlist
/// entries are written). Under `cfg.strip_member_symbols`
/// the member half is absent — the layout budgeted `nsyms` /
/// `strsize` the same way, so the two halves stay in lockstep.
pub(crate) fn write_symtab_and_strtab(buf: &mut Vec<u8>, cfg: &LinkConfig, layout: &ArchiveLayout) {
    // nlist + strtab: user fns first, then every member's
    // defined externs in member_layouts order.
    if cfg.strip_member_symbols {
        write_user_symtab_only(buf, cfg, layout);
        return;
    }
    let mut strtab = StringTable::new();
    let mut user_strx: Vec<(usize, u32)> = Vec::with_capacity(cfg.funcs.len());
    for (i, f) in symtab_rows(cfg) {
        user_strx.push((i, strtab.add(&f.name)));
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
    for (i, strx) in &user_strx {
        let nlist = Nlist64::defined_extern(*strx, 1, layout.fn_vaddrs[*i]);
        nlist.write_to(buf);
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
            nlist.write_to(buf);
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
