//! Mach-O `MH_EXECUTE` baseline emit — takes `CompiledFunction`s,
//! computes the static layout, applies relocations, and writes the
//! byte stream. Load commands: PAGEZERO + TEXT + LINKEDIT segments,
//! LC_LOAD_DYLINKER, LC_BUILD_VERSION, LC_MAIN, LC_SYMTAB, LC_DYSYMTAB,
//! LC_CODE_SIGNATURE (ad-hoc, mandatory on macOS 14+). Apple Silicon
//! page size is 16 KiB; `__PAGEZERO` covers the low 4 GiB to trap
//! null derefs. Archive-aware path with dyld bind chain lives in
//! `archive_emit.rs` (built on top of this baseline shape).

use torajs_codegen::CompiledFunction;
use torajs_obj::{
    CPU_SUBTYPE_ARM64_ALL, CPU_TYPE_ARM64, MH_MAGIC_64, MachHeader64, NLIST_64_SIZE, Nlist64,
    S_ATTR_PURE_INSTRUCTIONS, S_ATTR_SOME_INSTRUCTIONS, SECTION_64_SIZE, SEGMENT_COMMAND_64_SIZE,
    SYMTAB_COMMAND_SIZE, Section64, SegmentCommand64, StringTable, SymtabCommand, VM_PROT_EXECUTE,
    VM_PROT_READ,
};

use crate::lc::{
    APPLE_SILICON_PAGE_SIZE, BUILD_VERSION_CMDSIZE, DYSYMTAB_CMDSIZE, LINKEDIT_DATA_CMDSIZE,
    LOAD_DYLINKER_CMDSIZE, MAIN_CMDSIZE, MH_DYLDLINK, MH_EXECUTE, MH_NOUNDEFS, MH_PIE, MH_TWOLEVEL,
    PAGEZERO_VMSIZE, TEXT_VMADDR_BASE, write_build_version, write_code_signature, write_dysymtab,
    write_lc_main, write_load_dylinker,
};
use crate::resolve::{SymTable, apply_relocs};
use crate::sign::{adhoc_codesign_blob_size, build_adhoc_codesign_blob};

// ---- Public API ----

pub use crate::dead_strip_elide::{ElidableCall, GuardedStub, MemberGuard};
pub use crate::exec_user_entries::{
    UserBakedRegexEntry, UserClassLayoutEntry, UserClassNameEntry, UserDataGlobalEntry,
    UserFieldMetaEntry, UserFnNameEntry, UserMethodMetaEntry, UserStringEntry, UserStringKind,
    UserVtableEntry,
};

/// Configuration for `link_to_exec` — caller supplies the per-fn
/// codegen output, the entry-point symbol name (e.g. `"_main"`),
/// and a symbol table for any externs the relocs reference.
#[derive(Debug, Clone)]
pub struct LinkConfig {
    /// Compiled functions in `FuncId` order — `funcs[i]`
    /// corresponds to `FuncId(i)`.
    pub funcs: Vec<CompiledFunction>,
    /// Entry-point symbol name. Must match one of `funcs[i].name`.
    pub entry: String,
    /// External symbol resolution map for the linker's resolve
    /// pass. Empty for self-contained binaries.
    pub sym_table: SymTable,
    /// Ad-hoc codesign `Identifier=…` (default `"tora"`).
    pub codesign_ident: String,
    /// S2 dead-strip (RFC 20260824-s2-dead-strip): rewrite the
    /// archives dropping unreachable `__text` atoms before linking.
    /// `tr build` sets `true` (AOT artifacts ship slim); `tr run`
    /// keeps `false` (the pre-pass costs ~tens of ms compile time).
    /// `TORAJS_LINK_DEADSTRIP=1|0` overrides in both directions.
    pub dead_strip: bool,
    /// ld64 `-x`-class symtab policy at the runtime boundary: `true`
    /// writes only the user program's functions to `LC_SYMTAB`, dropping
    /// every member's defined extern (dyld never reads them — 173 KB of
    /// a 273 KB empty program, r498). `tr build` default; `--no-strip`
    /// / `tr run` keep `false` so runtime frames still symbolicate.
    pub strip_member_symbols: bool,
    /// r499 — `bl` sites in the user fns the dead-strip pre-pass may
    /// replace with a fixed instruction when the guard member holds no
    /// live text once the site's own edge is set aside (module doc in
    /// `dead_strip_elide`). Empty = no site is conditional.
    pub elidable_calls: Vec<ElidableCall>,
    /// r499 — definitions the pre-pass adds to shadow a runtime
    /// member's when that member's guard text is dead with the stub
    /// assumed (same module). Empty = nothing conditional.
    pub guarded_stubs: Vec<GuardedStub>,
    /// Static-library `.a` archives in Apple `ld64` search order.
    /// S7-C1 only carries bytes; resolution + integration in C2..C5.
    /// Empty for self-contained binaries. `Cow` so the baked
    /// `TORAJS_STATICLIBS` statics ride in borrowed (~50MB deep-copied
    /// per `tr run` pre-fix — ~36% of the per-case compile wall);
    /// file-read probe/test archives stay owned.
    pub archives: Vec<std::borrow::Cow<'static, [u8]>>,
    /// SD-4c-prereq+e0/e1/e2 — `ssa::Module.strings` materialized.
    /// e1/e2 emit rodata to `__TEXT,__cstring` + register sym→vaddr.
    /// Empty default keeps pre-e1 byte streams unchanged.
    pub strings: Vec<UserStringEntry>,
    /// SD-4c-prereq+e4 — `ssa::Module.data_globals` materialized as
    /// `__DATA,__bss` zerofill slots; loader supplies zero bytes.
    /// Requires the has_dyld path; non-empty + !has_dyld → reject.
    /// Empty default preserves pre-e4 byte streams.
    pub data_globals: Vec<UserDataGlobalEntry>,
    /// SD-4c-prereq+e7 — `__vtable_<C>` tables emitted as `[N x ptr]`
    /// rodata sharing `__TEXT,__cstring` with `strings`.
    /// `slot_syms[i] = Some(name)` writes vaddr-of-`name`; `None` → 0.
    pub vtable_globals: Vec<UserVtableEntry>,
    /// SD-4c-prereq+e8 — per-class `child_offsets` for the cycle
    /// collector (T-26.C); materializes the `__torajs_class_layouts`
    /// + `__torajs_n_class_layouts` pair (LLVM-era ABI).
    /// Empty default keeps pre-e8 callers byte-identical.
    pub class_layouts: Vec<UserClassLayoutEntry>,
    /// e8-2c — `true` emits the 4-byte count global + registers both
    /// class_layouts syms even on class-free programs. `tr build` sets
    /// this because it pulls libtorajs_cycle.a; probes default `false`.
    pub force_emit_class_layouts_globals: bool,
    /// Fn-name registry Phase 2 Step 5 — `true` emits the 8-byte count
    /// global (= 0) + registers both fn_name_table syms even on
    /// declaration-free programs. `tr build` sets this because every
    /// staticlib bundle now pulls `libtorajs_fnname.a` transitively
    /// (inspect.rs's Tag::Closure / Type::FnSig arms reference
    /// `__torajs_fn_print_inline`, whose definition pulls the table
    /// extern statics into the worklist). Probes default `false`.
    pub force_emit_fn_name_globals: bool,
    /// Fn-name registry Phase 2 Step 3b.2 — `ssa::Module.fn_name_globals`
    /// materialized as the `__torajs_fn_name_table[]` rodata global
    /// per `fn_name_table_layout.rs`. Each entry pairs a fn body's
    /// `__torajs_fn_<fid>` alias with its name's
    /// `__user_string_<sid>` alias so the link-time chain-fixup
    /// pipeline can resolve both into final vaddrs. Empty default
    /// keeps pre-Phase-2 byte streams unchanged.
    pub fn_name_globals: Vec<UserFnNameEntry>,
    /// W-J Phase A3c — `__torajs_class_name_table[]` rodata global
    /// per `class_name_table_layout.rs`. Each entry pairs an SSA-
    /// assigned `class_tag` (immediate u32) with the source-text
    /// class name's `__torajs_str_dyn_<sid>` alias. Read by the
    /// runtime helper `__torajs_struct_class_name(class_tag)` to
    /// supply the `Name {…}` prefix for Phase D's struct print
    /// walker + future reflection consumers. Empty default keeps
    /// pre-A3c byte streams unchanged.
    pub class_names: Vec<UserClassNameEntry>,
    /// W-J Phase A3c — `true` emits the 8-byte count global (= 0)
    /// + registers both `__torajs_class_name_table` syms even on
    /// class-name-free programs. `tr build` sets this because
    /// `libtorajs_structmeta.a` references the table externs
    /// unconditionally (same rationale as
    /// `force_emit_fn_name_globals`). Probes default `false`.
    pub force_emit_class_names_globals: bool,
    /// V0.2 P14 chunk 7.7 v2 step 12 C2 Phase C-5a — per-literal-RegExp
    /// AOT-baked DFA blob. ssa_lower's Phase C-6 `Expr::Regex` AOT
    /// gate pushes one entry per DFA-eligible literal regex (`/abc/`,
    /// `/foo\d+/g`, ...); torajs-link's Phase C-5b
    /// `user_regex_baked_layout` materialises each as a
    /// `BakedDfaMeta` 32-byte struct + `[DfaState; N]` payload in
    /// `__DATA_CONST`. The user binary calls
    /// `__torajs_regex_compile_from_static_dfa(meta_ptr, pat, flag)`
    /// against each `__torajs_baked_regex_<index>` symbol instead of
    /// the runtime `__torajs_regex_compile` for the corresponding
    /// literal. Empty default keeps every pre-C-5 byte stream
    /// byte-identical until the upstream emit lands.
    pub baked_regex_entries: Vec<UserBakedRegexEntry>,
}

/// Layout decisions the link driver makes before any bytes are emitted.
#[derive(Debug, Clone)]
pub struct ExecLayout {
    /// File offset where the `__text` payload begins — packed
    /// right after the header + load commands, 16-aligned (ld64
    /// layout; the whole `__TEXT` segment maps r-x anyway).
    pub text_file_offset: u32,
    /// `__text` payload size in bytes (sum of all
    /// `ResolvedFunction.bytes.len()`).
    pub text_size: u32,
    /// `__TEXT` segment vmaddr (`0x1_0000_0000`).
    pub text_vmaddr: u64,
    /// `__TEXT` segment vmsize — page-aligned, covers header +
    /// load commands + `__text` payload.
    pub text_vmsize: u64,
    /// File offset where `__LINKEDIT` data begins (start of the
    /// nlist table).
    pub linkedit_file_offset: u32,
    /// `__LINKEDIT` vmaddr — `text_vmaddr + text_vmsize`.
    pub linkedit_vmaddr: u64,
    /// `__LINKEDIT` vmsize — page-aligned, covers symtab + strtab.
    pub linkedit_vmsize: u64,
    /// File offset of the nlist table.
    pub symoff: u32,
    /// File offset of the string table.
    pub stroff: u32,
    /// Total file size = `linkedit_file_offset + linkedit_data_size`.
    pub total_size: u32,
    /// Number of defined-extern symbols (one per `funcs[i]`).
    pub nsyms: u32,
    /// String table byte length.
    pub strsize: u32,
    /// Virtual address of the entry function (`_main`) in the
    /// final image — used to fill `LC_MAIN.entry_off`.
    pub entry_file_offset: u32,
    /// Final virtual address of each function — `fn_vaddrs[i]` =
    /// `text_vmaddr + (text section file offset relative to __TEXT)
    /// + fn_offset_in_text`. Same indexing as
    /// `LinkConfig.funcs`.
    pub fn_vaddrs: Vec<u64>,
    /// File offset of the embedded codesign SuperBlob —
    /// `LC_CODE_SIGNATURE.dataoff` points here.
    pub codesign_dataoff: u32,
    /// Codesign blob byte length (= `LC_CODE_SIGNATURE.datasize`).
    pub codesign_datasize: u32,
}

/// Round `value` up to the nearest multiple of `align` (which
/// must be a power of two).
fn round_up_to(value: u64, align: u64) -> u64 {
    debug_assert!(align.is_power_of_two());
    (value + align - 1) & !(align - 1)
}

/// Compute the file + virtual layout for a `LinkConfig` ahead of
/// any bytes being emitted. Pure function; trivial to unit-test
/// against expected layout values.
pub fn compute_layout(cfg: &LinkConfig) -> ExecLayout {
    let n_funcs = cfg.funcs.len() as u32;

    // Sum the load-commands block size up front so we know where
    // the page-aligned `__text` payload starts.
    let sizeofcmds = (SEGMENT_COMMAND_64_SIZE * 3) // PAGEZERO + TEXT (with 1 section) + LINKEDIT
        + SECTION_64_SIZE                          // __text section under __TEXT
        + LOAD_DYLINKER_CMDSIZE
        + BUILD_VERSION_CMDSIZE
        + MAIN_CMDSIZE
        + SYMTAB_COMMAND_SIZE
        + DYSYMTAB_CMDSIZE
        + LINKEDIT_DATA_CMDSIZE; // LC_CODE_SIGNATURE
    let header_plus_lc = (MachHeader64::SIZE as u32) + sizeofcmds;

    // `__text` packs right after the load commands, 16-aligned —
    // the whole `__TEXT` segment maps r-x, header included, so a
    // page boundary here only wasted file bytes (r498; mirrors the
    // archive path in `archive_layout_text.rs`).
    let text_file_offset = round_up_to(u64::from(header_plus_lc), 16) as u32;

    // Concatenate function bytes; remember each fn's section-
    // relative offset for the vaddr table.
    let mut text_size: u32 = 0;
    let mut fn_offsets_in_text: Vec<u32> = Vec::with_capacity(cfg.funcs.len());
    for f in &cfg.funcs {
        debug_assert_eq!(
            f.bytes.len() % 4,
            0,
            "CompiledFunction bytes must be 4-aligned (aarch64 instruction width)"
        );
        fn_offsets_in_text.push(text_size);
        text_size += f.bytes.len() as u32;
    }

    // `__TEXT` segment vmsize is page-aligned and covers
    // mach_header + load commands + __text payload + slack to
    // the next page boundary.
    let text_segment_filesize_unrounded = u64::from(text_file_offset) + u64::from(text_size);
    let text_vmsize = round_up_to(text_segment_filesize_unrounded, APPLE_SILICON_PAGE_SIZE);

    // `__LINKEDIT` follows directly after `__TEXT` in vmaddr and
    // file order. symtab + strtab live inside.
    let linkedit_file_offset = text_vmsize as u32;
    let linkedit_vmaddr = TEXT_VMADDR_BASE + text_vmsize;

    let symoff = linkedit_file_offset;
    let nlist_size = n_funcs * NLIST_64_SIZE;
    let stroff = symoff + nlist_size;

    // String table: "\0" sentinel + every defined-function name
    // NUL-terminated.
    let mut strsize: u32 = 1;
    for f in &cfg.funcs {
        strsize += f.name.len() as u32 + 1;
    }
    // Codesign blob sits after symtab + strtab inside __LINKEDIT.
    // `code_limit` covers every byte up to (but not including) the
    // signature blob itself — that's the offset we hash.
    let codesign_dataoff = stroff + strsize;
    let codesign_datasize = adhoc_codesign_blob_size(codesign_dataoff, &cfg.codesign_ident);

    let linkedit_data_size = nlist_size + strsize + codesign_datasize;
    let linkedit_vmsize = round_up_to(u64::from(linkedit_data_size), APPLE_SILICON_PAGE_SIZE);
    let total_size = linkedit_file_offset + linkedit_data_size;

    // Each function's final vaddr = text_vmaddr + section start
    // within __TEXT (file offset relative to segment start, which
    // is also the in-segment vmaddr offset because fileoff and
    // vmaddr both originate at 0 for __TEXT).
    let text_vmaddr = TEXT_VMADDR_BASE;
    let fn_vaddrs: Vec<u64> = fn_offsets_in_text
        .iter()
        .map(|off| text_vmaddr + u64::from(text_file_offset) + u64::from(*off))
        .collect();

    // Entry point file offset — find the entry symbol in funcs.
    let entry_idx = cfg
        .funcs
        .iter()
        .position(|f| f.name == cfg.entry)
        .unwrap_or_else(|| panic!("entry symbol {:?} not in LinkConfig.funcs", cfg.entry));
    let entry_file_offset = text_file_offset + fn_offsets_in_text[entry_idx];

    ExecLayout {
        text_file_offset,
        text_size,
        text_vmaddr,
        text_vmsize,
        linkedit_file_offset,
        linkedit_vmaddr,
        linkedit_vmsize,
        symoff,
        stroff,
        total_size,
        nsyms: n_funcs,
        strsize,
        entry_file_offset,
        fn_vaddrs,
        codesign_dataoff,
        codesign_datasize,
    }
}

/// Link a `LinkConfig` into a Mach-O `MH_EXECUTE` byte stream
/// signed with an in-house ad-hoc code signature.
///
/// Caller responsibility:
///   * `cfg.entry` must match one of `cfg.funcs[i].name`.
///   * Function names that the codegen will reference via
///     `CallTarget::Extern` / `Page21` / etc. that aren't defined
///     functions must be supplied in `cfg.sym_table` with their
///     final virtual addresses.
///
/// Output: a complete runnable Mach-O `MH_EXECUTE` image. 9 load
/// commands (3 LC_SEGMENT_64 + LC_LOAD_DYLINKER + LC_BUILD_VERSION
/// + LC_MAIN + LC_SYMTAB + LC_DYSYMTAB + LC_CODE_SIGNATURE).
/// macOS 14+ kernel + dyld accept the binary directly — no
/// external `codesign` tool needed.
pub fn link_to_exec(cfg: &LinkConfig) -> Vec<u8> {
    let layout = compute_layout(cfg);

    // Resolve relocations now that we know every function's final
    // virtual address. `apply_relocs` clones the input bytes and
    // patches them in place against `fn_vaddrs`.
    let resolved = apply_relocs(&cfg.funcs, &layout.fn_vaddrs, &cfg.sym_table);

    // Build the load commands.
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

    // Allocate the output buffer and emit each region in order.
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
        "emitted load commands must match sizeofcmds"
    );

    // Pad to first page boundary.
    pad_to(&mut buf, layout.text_file_offset as usize);

    // `__text` payload — concat the resolved (patched) bytes.
    for r in &resolved {
        buf.extend_from_slice(&r.bytes);
    }

    // Pad to __LINKEDIT page boundary.
    pad_to(&mut buf, layout.linkedit_file_offset as usize);

    // nlist + strtab. `n_value` carries the final virtual
    // address of the defined symbol so dyld + `nm` can both
    // resolve symbol → address without walking the section
    // table.
    let mut strtab = StringTable::new();
    let mut strx_per_fn: Vec<u32> = Vec::with_capacity(cfg.funcs.len());
    for f in &cfg.funcs {
        strx_per_fn.push(strtab.add(&f.name));
    }
    for (i, _f) in cfg.funcs.iter().enumerate() {
        let nlist = Nlist64::defined_extern(strx_per_fn[i], 1, layout.fn_vaddrs[i]);
        nlist.write_to(&mut buf);
    }
    buf.extend_from_slice(strtab.as_bytes());

    // At this point `buf` covers every byte of the binary up to —
    // but not including — the codesign blob. That's exactly the
    // `code_limit` region the SHA-256 page hashes cover.
    debug_assert_eq!(buf.len() as u32, layout.codesign_dataoff);
    let blob = build_adhoc_codesign_blob(&buf, &cfg.codesign_ident);
    debug_assert_eq!(blob.len() as u32, layout.codesign_datasize);
    buf.extend_from_slice(&blob);

    debug_assert_eq!(
        buf.len() as u32,
        layout.total_size,
        "emitted file size must match computed layout total"
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
    use torajs_codegen::compile_function;
    use torajs_core::ssa::{
        BinOp, Block, BlockId, Function, Inst, InstKind, Operand, Terminator, Type, ValueId,
        ValueInfo,
    };

    /// `fn _main() -> i64 { 42 + 0 }` — the canonical S6
    /// acceptance fixture, used here as a layout / emit driver.
    /// Compiles to a 4-instruction sequence = 16 bytes.
    fn build_main_returns_42() -> Function {
        let v0 = ValueId(0);
        Function {
            name: "_main".into(),
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
                    kind: InstKind::BinOp(BinOp::Add, Operand::ConstI64(42), Operand::ConstI64(0)),
                    origin: None,
                }],
                term: Terminator::Ret(Some(Operand::Value(v0))),
            }],
            current_origin: None,
        }
    }

    #[test]
    fn compute_layout_pin_known_offsets() {
        let main = compile_function(&build_main_returns_42());
        let cfg = LinkConfig {
            funcs: vec![main],
            entry: "_main".into(),
            sym_table: SymTable::new(),
            codesign_ident: "tora".into(),
            dead_strip: false,
            strip_member_symbols: false,
            elidable_calls: Vec::new(),
            guarded_stubs: Vec::new(),
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
        let layout = compute_layout(&cfg);

        // header (32) + 3 LC_SEGMENT_64 (72*3 = 216) + 1 section (80) +
        // LC_LOAD_DYLINKER (32) + LC_BUILD_VERSION (24) + LC_MAIN (24) +
        // LC_SYMTAB (24) + LC_DYSYMTAB (80) + LC_CODE_SIGNATURE (16) = 528.
        // `__text` packs right after (16-aligned; 528 already is).
        assert_eq!(layout.text_file_offset, 528);

        // __text payload = the compiled `_main` body. Pinning a byte
        // count here couples this layout test to codegen instruction
        // selection (it broke when const-folding shortened `42 + 0`
        // from 16 to 12 bytes) — pin the layout invariant instead.
        assert_eq!(layout.text_size, cfg.funcs[0].bytes.len() as u32);

        // __TEXT vmsize = round_up(528 + text_size, 0x4000) = 0x4000
        // (payload is a handful of instructions, far below one page).
        assert_eq!(layout.text_vmsize, 0x4000);

        // __LINKEDIT lands at the first page boundary.
        assert_eq!(layout.linkedit_file_offset, 0x4000);
        assert_eq!(layout.linkedit_vmaddr, TEXT_VMADDR_BASE + 0x4000);

        // 1 sym × 16 byte + strtab ("\0" + "_main\0" = 7 byte) = 23 byte
        // → __LINKEDIT vmsize page-rounded = 0x4000.
        assert_eq!(layout.nsyms, 1);
        assert_eq!(layout.strsize, 7);
        assert_eq!(layout.linkedit_vmsize, 0x4000);

        // _main vaddr = TEXT_VMADDR_BASE + 528 (text packs right
        // after the load commands).
        assert_eq!(layout.fn_vaddrs.len(), 1);
        assert_eq!(layout.fn_vaddrs[0], TEXT_VMADDR_BASE + 528);
        assert_eq!(layout.entry_file_offset, 528);

        // Total file size = linkedit_file_offset + nlist + strtab +
        // codesign blob. Codesign blob = SB(12) + index(8) + CD
        // header(48) + ident "tora\0"(5) + ceil(code_limit/4096)
        // *32. code_limit = stroff + strsize = 0x4010 + 7 = 0x4017
        // = 16407 → 5 code slots → blob 73 + 160 = 233.
        assert_eq!(layout.codesign_dataoff, 0x4000 + 16 + 7);
        assert_eq!(layout.codesign_datasize, 233);
        assert_eq!(layout.total_size, 0x4000 + 16 + 7 + 233);
    }

    #[test]
    fn link_to_exec_produces_expected_total_size() {
        let main = compile_function(&build_main_returns_42());
        let cfg = LinkConfig {
            funcs: vec![main],
            entry: "_main".into(),
            sym_table: SymTable::new(),
            codesign_ident: "tora".into(),
            dead_strip: false,
            strip_member_symbols: false,
            elidable_calls: Vec::new(),
            guarded_stubs: Vec::new(),
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
        let bytes = link_to_exec(&cfg);
        let layout = compute_layout(&cfg);
        assert_eq!(bytes.len() as u32, layout.total_size);
    }

    #[test]
    fn link_to_exec_header_filetype_is_execute() {
        let main = compile_function(&build_main_returns_42());
        let cfg = LinkConfig {
            funcs: vec![main],
            entry: "_main".into(),
            sym_table: SymTable::new(),
            codesign_ident: "tora".into(),
            dead_strip: false,
            strip_member_symbols: false,
            elidable_calls: Vec::new(),
            guarded_stubs: Vec::new(),
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
        let bytes = link_to_exec(&cfg);
        // mach_header_64.filetype @ offset 12..16
        let filetype = u32::from_le_bytes(bytes[12..16].try_into().unwrap());
        assert_eq!(filetype, MH_EXECUTE);

        // ncmds @ 16..20 = 9 (8 + LC_CODE_SIGNATURE)
        let ncmds = u32::from_le_bytes(bytes[16..20].try_into().unwrap());
        assert_eq!(ncmds, 9);

        // flags @ 24..28 = MH_NOUNDEFS | MH_DYLDLINK | MH_TWOLEVEL | MH_PIE
        let flags = u32::from_le_bytes(bytes[24..28].try_into().unwrap());
        assert_eq!(flags, MH_NOUNDEFS | MH_DYLDLINK | MH_TWOLEVEL | MH_PIE);
    }

    #[test]
    fn link_to_exec_text_payload_lands_at_text_file_offset() {
        let main = compile_function(&build_main_returns_42());
        let cfg = LinkConfig {
            funcs: vec![main.clone()],
            entry: "_main".into(),
            sym_table: SymTable::new(),
            codesign_ident: "tora".into(),
            dead_strip: false,
            strip_member_symbols: false,
            elidable_calls: Vec::new(),
            guarded_stubs: Vec::new(),
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
        let bytes = link_to_exec(&cfg);
        let layout = compute_layout(&cfg);
        // The bytes at text_file_offset should match the compiled
        // `_main` body (length follows codegen output).
        let off = layout.text_file_offset as usize;
        assert_eq!(&bytes[off..off + main.bytes.len()], &main.bytes[..]);
    }

    /// Host `otool -hlv` must accept the emitted binary and
    /// report the layout we computed. Gated to macOS arm64 hosts
    /// since `otool` is Xcode-bundled.
    #[test]
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    fn otool_accepts_emitted_minimal_executable() {
        use std::process::Command;

        let main = compile_function(&build_main_returns_42());
        let cfg = LinkConfig {
            funcs: vec![main],
            entry: "_main".into(),
            sym_table: SymTable::new(),
            codesign_ident: "tora".into(),
            dead_strip: false,
            strip_member_symbols: false,
            elidable_calls: Vec::new(),
            guarded_stubs: Vec::new(),
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
        let bytes = link_to_exec(&cfg);

        let path = format!("/private/tmp/torajs_link_s3_{}", std::process::id());
        std::fs::write(&path, &bytes).expect("write exec");

        let output = match Command::new("/usr/bin/otool")
            .args(["-hlv", &path])
            .output()
        {
            Ok(o) => o,
            Err(e) => {
                eprintln!("skip: otool not invokable: {e}");
                let _ = std::fs::remove_file(&path);
                return;
            }
        };
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        let _ = std::fs::remove_file(&path);

        assert!(
            output.status.success(),
            "otool failed (exit {:?})\n--- stderr ---\n{stderr}\n--- stdout ---\n{stdout}",
            output.status.code()
        );

        // `otool -hlv` prints header + load command structure but
        // not the contents of LC_SYMTAB. Verify the structural
        // markers here; symbol-table contents go through `nm`
        // below.
        for needle in [
            "MH_MAGIC_64",
            "ARM64",
            "EXECUTE",
            "__PAGEZERO",
            "__TEXT",
            "__text",
            "__LINKEDIT",
            "LC_LOAD_DYLINKER",
            "/usr/lib/dyld",
            "LC_BUILD_VERSION",
            "LC_MAIN",
            "LC_SYMTAB",
            "LC_DYSYMTAB",
            "minos 14.0",
        ] {
            assert!(
                stdout.contains(needle),
                "otool output missing {needle:?}; full output:\n{stdout}"
            );
        }

        // Re-write the file (the previous `remove_file` ran) so
        // `nm` can read it.
        std::fs::write(&path, &bytes).expect("rewrite exec");
        let nm_out = match Command::new("/usr/bin/nm").arg(&path).output() {
            Ok(o) => o,
            Err(e) => {
                eprintln!("skip: nm not invokable: {e}");
                let _ = std::fs::remove_file(&path);
                return;
            }
        };
        let nm_stdout = String::from_utf8_lossy(&nm_out.stdout).into_owned();
        let _ = std::fs::remove_file(&path);

        assert!(
            nm_out.status.success(),
            "nm failed: {}",
            String::from_utf8_lossy(&nm_out.stderr)
        );
        // `_main` is a defined external symbol — `nm` prints it
        // with type `T` (text segment, external).
        assert!(
            nm_stdout.contains("_main"),
            "nm did not list _main; output:\n{nm_stdout}"
        );
        assert!(
            nm_stdout.contains(" T "),
            "_main expected type T (text/external); nm output:\n{nm_stdout}"
        );
    }

    /// S6 end-to-end acceptance — emit the binary for
    /// `fn _main() -> i64 { 42 }` *with the in-house ad-hoc
    /// codesignature already embedded by `link_to_exec`*, chmod
    /// +x, exec it, and verify the process exits with status 42.
    ///
    /// macOS 14+ refuses to load unsigned arm64 binaries: an
    /// unsigned image is killed by SIGKILL before `main` even
    /// runs (exit code 137). This test catches both flavors of
    /// failure — wrong layout (kernel/dyld refuses the load
    /// command set or vmaddr layout) and wrong signature (kernel
    /// kills before main runs because the page hashes don't
    /// match). The fact that the test passes means torajs-link's
    /// in-house SHA-256 + CodeDirectory blob writer produces a
    /// signature byte-compatible with macOS's kernel codesign
    /// verifier.
    ///
    /// No external `codesign` invocation — the entire ad-hoc CMS
    /// signature blob is computed by `sign::build_adhoc_codesign_blob`
    /// at link time. The torajs build pipeline now has zero
    /// external toolchain dependencies.
    ///
    /// Gated to macOS arm64; non-Apple-Silicon hosts skip.
    #[test]
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    fn end_to_end_run_returns_42_with_self_research_codesign() {
        use std::os::unix::fs::PermissionsExt;
        use std::process::Command;

        let main = compile_function(&build_main_returns_42());
        let cfg = LinkConfig {
            funcs: vec![main],
            entry: "_main".into(),
            sym_table: SymTable::new(),
            codesign_ident: "tora".into(),
            dead_strip: false,
            strip_member_symbols: false,
            elidable_calls: Vec::new(),
            guarded_stubs: Vec::new(),
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
        let bytes = link_to_exec(&cfg);

        let path = format!("/private/tmp/torajs_link_e2e_{}", std::process::id());
        std::fs::write(&path, &bytes).expect("write binary");
        let mut perms = std::fs::metadata(&path).expect("stat").permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).expect("chmod +x");

        let run = Command::new(&path).output().expect("exec emitted binary");
        let exit = run.status.code();
        let _ = std::fs::remove_file(&path);

        assert_eq!(
            exit,
            Some(42),
            "binary did not exit 42 — stdout={:?} stderr={:?}",
            String::from_utf8_lossy(&run.stdout),
            String::from_utf8_lossy(&run.stderr)
        );
    }
}
