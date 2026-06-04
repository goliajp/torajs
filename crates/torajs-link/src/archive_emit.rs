//! Archive-aware emit pass — S7-C4.
//!
//! Builds on `archive_link::compute_archive_layout` (S7-C3) to
//! emit a complete Mach-O `MH_EXECUTE` byte stream that includes
//! both user `CompiledFunction`s and `.o` members pulled from
//! `LinkConfig.archives`. User-function relocations now resolve
//! against an extended `SymTable` whose entries also cover every
//! defined-external symbol in every integrated member.
//!
//! Layout structure inherited from S7-C3:
//!
//! ```text
//!   __TEXT,__text:
//!     [ user funcs (resolved bytes) ][ member 0 __text ][ member 1 __text ]…
//!   __LINKEDIT:
//!     nlist  = user fns + every integrated member's defined externs
//!     strtab = "\0" + user fn names + member sym names
//!     codesign blob (SHA-256 + ad-hoc CMS, self-research)
//! ```
//!
//! **Scope of cross-member reloc patching in S7-C4**: only
//! user-function relocs are patched. Member-internal relocations
//! (a member referencing another archive member by symbol) are
//! NOT patched yet — the integrated `__text` bytes go in
//! verbatim. That keeps the diff small and matches the S7-C4
//! acceptance fixture (`_main → _foo`, where `_foo` is a leaf).
//! S7-C5 will surface the production smoke-test case where real
//! `libtorajs_*.a` members have intra-archive relocs that need
//! `apply_relocs_with_archives` to fan out across members. The
//! split is intentional — each step's correctness is
//! independently verifiable.

use torajs_obj::{
    CPU_SUBTYPE_ARM64_ALL, CPU_TYPE_ARM64, MH_MAGIC_64, MachHeader64, NLIST_64_SIZE, Nlist64,
    S_ATTR_PURE_INSTRUCTIONS, S_ATTR_SOME_INSTRUCTIONS, SECTION_64_SIZE, SEGMENT_COMMAND_64_SIZE,
    SYMTAB_COMMAND_SIZE, Section64, SegmentCommand64, StringTable, SymtabCommand, VM_PROT_EXECUTE,
    VM_PROT_READ,
};

use crate::archive_link::{ArchiveLayout, ArchiveLayoutError, compute_archive_layout};
use crate::archives_merge::{MergedArchives, merge_archive_indexes};
use crate::exec::LinkConfig;
use crate::lc::{
    BUILD_VERSION_CMDSIZE, DYSYMTAB_CMDSIZE, LINKEDIT_DATA_CMDSIZE, LOAD_DYLINKER_CMDSIZE,
    MAIN_CMDSIZE, MH_DYLDLINK, MH_EXECUTE, MH_NOUNDEFS, MH_PIE, MH_TWOLEVEL, PAGEZERO_VMSIZE,
    write_build_version, write_code_signature, write_dysymtab, write_lc_main, write_load_dylinker,
};
use crate::resolve::apply_relocs;
use crate::sign::build_adhoc_codesign_blob;

/// Link a `LinkConfig` whose `archives` field is populated into a
/// complete Mach-O `MH_EXECUTE` byte stream — ad-hoc codesigned,
/// kernel-loadable on macOS 14+.
///
/// User-function relocs against `Extern(name)` resolve to either
/// `cfg.sym_table` entries or member-defined externs (member's
/// vaddr + `nlist_64.n_value`). Member-internal relocs (members
/// referencing other archive symbols) are NOT yet patched —
/// integrated member `__text` bytes go in verbatim. See module
/// doc for the S7-C5 follow-up.
pub fn link_to_exec_with_archives(cfg: &LinkConfig) -> Result<Vec<u8>, ArchiveLayoutError> {
    let layout = compute_archive_layout(cfg)?;
    let merged = merge_archive_indexes(&cfg.archives).map_err(ArchiveLayoutError::Merge)?;

    // Effective sym table — caller-supplied externs union every
    // defined-external symbol any integrated member exposes.
    let mut effective_sym_table = cfg.sym_table.clone();
    for m in &layout.member_layouts {
        for (name, n_value) in &m.defined_syms {
            effective_sym_table.insert(name.clone(), m.vaddr + n_value);
        }
    }

    // Resolve user-function relocs. Member-internal relocs are
    // intentionally not patched in S7-C4.
    let resolved = apply_relocs(&cfg.funcs, &layout.fn_vaddrs, &effective_sym_table);

    let bytes = emit_binary(cfg, &layout, &merged, &resolved);
    Ok(bytes)
}

/// Concatenate header + load commands + __text + __LINKEDIT
/// (nlist, strtab, codesign blob) into a single byte stream.
/// Layout decisions all come from `ArchiveLayout`; this fn is
/// pure assembly of bytes.
fn emit_binary(
    cfg: &LinkConfig,
    layout: &ArchiveLayout,
    merged: &MergedArchives<'_>,
    resolved: &[crate::resolve::ResolvedFunction],
) -> Vec<u8> {
    let header = MachHeader64 {
        magic: MH_MAGIC_64,
        cputype: CPU_TYPE_ARM64,
        cpusubtype: CPU_SUBTYPE_ARM64_ALL,
        filetype: MH_EXECUTE,
        ncmds: 9,
        sizeofcmds: SEGMENT_COMMAND_64_SIZE * 3
            + SECTION_64_SIZE
            + LOAD_DYLINKER_CMDSIZE
            + BUILD_VERSION_CMDSIZE
            + MAIN_CMDSIZE
            + SYMTAB_COMMAND_SIZE
            + DYSYMTAB_CMDSIZE
            + LINKEDIT_DATA_CMDSIZE,
        flags: MH_NOUNDEFS | MH_DYLDLINK | MH_TWOLEVEL | MH_PIE,
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
    let text_segment = SegmentCommand64 {
        segname: "__TEXT".into(),
        vmaddr: layout.text_vmaddr,
        vmsize: layout.text_vmsize,
        fileoff: 0,
        filesize: layout.text_vmsize,
        maxprot: VM_PROT_READ | VM_PROT_EXECUTE,
        initprot: VM_PROT_READ | VM_PROT_EXECUTE,
        flags: 0,
        sections: vec![text_section],
    };

    let linkedit_data_size = u64::from(layout.nsyms) * u64::from(NLIST_64_SIZE)
        + u64::from(layout.strsize)
        + u64::from(layout.codesign_datasize);
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
    linkedit_segment.write_to(&mut buf);
    write_load_dylinker(&mut buf);
    write_build_version(&mut buf);
    write_lc_main(&mut buf, u64::from(layout.entry_file_offset));
    symtab_cmd.write_to(&mut buf);
    write_dysymtab(&mut buf, layout.nsyms);
    write_code_signature(&mut buf, layout.codesign_dataoff, layout.codesign_datasize);

    debug_assert_eq!(
        (buf.len() as u32) - (MachHeader64::SIZE as u32),
        header.sizeofcmds,
        "load command size mismatch",
    );

    pad_to(&mut buf, layout.text_file_offset as usize);

    // __text payload: user funcs (resolved) followed by each
    // integrated member's __text slice.
    for r in resolved {
        buf.extend_from_slice(&r.bytes);
    }
    for m in &layout.member_layouts {
        let member = &merged.per_archive_members[m.key.0][m.key.1];
        let off = m.member_text_offset_in_member as usize;
        let end = off + m.text_size as usize;
        buf.extend_from_slice(&member.data[off..end]);
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
        for (name, _) in &m.defined_syms {
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
        for (si, (_name, n_value)) in m.defined_syms.iter().enumerate() {
            let nlist = Nlist64::defined_extern(member_strx[mi][si], 1, m.vaddr + n_value);
            nlist.write_to(&mut buf);
        }
    }
    buf.extend_from_slice(strtab.as_bytes());

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
