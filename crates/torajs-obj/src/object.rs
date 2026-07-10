//! High-level Mach-O object builder.
//!
//! Translates a `&[CompiledFunction]` (the per-function output of
//! `torajs_codegen::compile_function`) into a complete Mach-O 64
//! `MH_OBJECT` byte stream that downstream tools (`otool` / Apple
//! `ld` / torajs-link #8) can consume.
//!
//! S5-a shipped the scaffold (single-function, no-reloc path).
//! S5-b (this commit) lights up:
//!   * Multi-function `__TEXT,__text` payload — `CompiledFunction`s
//!     concatenated in input order, `fn_offsets[i]` becomes that
//!     function's section-relative address.
//!   * Symbol table sort — Apple `ld` requires
//!     `local | extdef | undef` partition order. Defined externs
//!     (the input functions, in input order = `FuncId` order) come
//!     first; undefined externs (everything `relocs[]` references
//!     that isn't a defined function) come after.
//!   * `RelocKind → RelocationInfo` translation:
//!       `CallSite::Func(fid)`     → `branch26 (sym_idx = fid.0)`
//!       `CallSite::Extern(name)`  → `branch26 (sym_idx = extern_map[name])`
//!       `Page21 { target_sym }`   → `page21`
//!       `PageOff12 { target_sym }`→ `pageoff12`
//!       `AbsPtr64 { target_sym }` → `unsigned64`
//!     `r_address = fn_offsets[i] + reloc.byte_offset` is section-
//!     relative.
//!
//! S5-c will add the optional macOS host link round-trip; S5-d
//! adds the phase-close docs marker.
//!
//! File-byte layout (the order `clang -c` emits and Apple `ld`
//! reads — section data + per-section relocs + symtab + strtab):
//!
//! ```text
//!   [   0]                       mach_header_64           (32 B)
//!   [  32]                       LC_SEGMENT_64 + section  (72 + 80 = 152 B)
//!   [ 184]                       LC_SYMTAB                (24 B)
//!   [ 208]                       __TEXT,__text payload    (4-aligned)
//!   [ 208 + text_size]           relocation entries       (8 B per reloc)
//!   [ ... + reloc_size]          nlist_64 table           (16 B per sym)
//!   [ ... + nsyms*16]            string table             ("\0" sentinel + names)
//! ```
//!
//! `LC_DYSYMTAB` (the third load command that carries the
//! `local | extdef | undef` index ranges) is *not* emitted yet —
//! Apple `ld` accepts `.o`s without it (it falls back to scanning
//! `LC_SYMTAB` directly), and skipping it keeps the header pair
//! count stable at 2. If a future link round-trip fails for this
//! reason, S5-c (or S5-d) can add it.
//!
//! `FuncId` → defined-symbol-index identity:
//! `funcs[i]` always lands at defined-sym index `i`, by convention
//! mirroring `torajs_core::ssa::Module::funcs[i] == FuncId(i)`. The
//! caller must pass `&[CompiledFunction]` in `FuncId` order.

use std::collections::BTreeMap;

use torajs_codegen::CompiledFunction;
use torajs_codegen::reloc::{CallTarget, RelocKind};

use crate::macho::header::MachHeader64;
use crate::macho::reloc::{RELOCATION_INFO_SIZE, RelocationInfo};
use crate::macho::segment::{SECTION_64_SIZE, SEGMENT_COMMAND_64_SIZE, SegmentCommand64};
use crate::macho::symtab::{
    NLIST_64_SIZE, Nlist64, SYMTAB_COMMAND_SIZE, StringTable, SymtabCommand,
};

/// `align_log2 = 2` matches what `clang -c` emits for `__TEXT,__text`
/// holding aarch64 code: 2^2 = 4-byte instruction-word alignment.
/// S5-c+ can lift this to 4 (16-byte function-prologue alignment)
/// once inter-fn padding is desired.
const TEXT_ALIGN_LOG2: u32 = 2;

/// Section index in the in-memory `__TEXT,__text` segment. Mach-O
/// `n_sect` is 1-based and the only section we emit is `__text` at
/// slot 1.
const SECTION_TEXT_INDEX: u8 = 1;

/// Collect the unique extern symbol names referenced by `funcs`'s
/// relocs that are *not* among the defined functions. Returned in
/// the canonical (sorted) order Apple `ld` expects for the
/// `undef` partition of the symbol table.
///
/// The sort is `BTreeMap`-derived (lexicographic) so two runs over
/// the same input produce byte-identical `.o`s — important for
/// reproducible builds and for caching downstream (`torajs-link`,
/// `~/.torajs/cache`).
fn collect_externs(funcs: &[CompiledFunction]) -> Vec<String> {
    let defined: std::collections::HashSet<&str> = funcs.iter().map(|f| f.name.as_str()).collect();
    let mut externs: BTreeMap<String, ()> = BTreeMap::new();
    for f in funcs {
        for r in &f.relocs {
            let target_name: Option<&str> = match &r.kind {
                RelocKind::CallSite {
                    target: CallTarget::Func(_),
                } => None,
                RelocKind::CallSite {
                    target: CallTarget::Extern(name),
                } => Some(name.as_str()),
                RelocKind::Page21 { target_sym }
                | RelocKind::PageOff12 { target_sym }
                | RelocKind::AbsPtr64 { target_sym } => Some(target_sym.as_str()),
            };
            if let Some(name) = target_name {
                if !defined.contains(name) {
                    externs.insert(name.to_owned(), ());
                }
            }
        }
    }
    externs.into_keys().collect()
}

/// Translate one `Reloc` to its on-disk `RelocationInfo`.
///
/// `fn_offset` is the section-relative byte offset where this
/// function's `bytes` begin (= `fn_offsets[i]`); the final
/// `r_address` is that plus the reloc's intra-function
/// `byte_offset`. `extern_idx` maps extern symbol names to their
/// global `nlist_64` index (= `funcs.len() + extern_sort_pos`).
/// Defined `FuncId(j)` maps directly to global index `j`.
fn translate_reloc(
    r: &torajs_codegen::reloc::Reloc,
    fn_offset: u32,
    extern_idx: &BTreeMap<&str, u32>,
) -> RelocationInfo {
    let r_address = fn_offset + r.byte_offset;
    match &r.kind {
        RelocKind::CallSite {
            target: CallTarget::Func(fid),
        } => RelocationInfo::branch26(r_address, fid.0),
        RelocKind::CallSite {
            target: CallTarget::Extern(name),
        } => {
            let idx = *extern_idx
                .get(name.as_str())
                .unwrap_or_else(|| panic!("extern symbol {name:?} not in extern_idx map"));
            RelocationInfo::branch26(r_address, idx)
        }
        RelocKind::Page21 { target_sym } => {
            let idx = *extern_idx
                .get(target_sym.as_str())
                .unwrap_or_else(|| panic!("Page21 target {target_sym:?} not in extern_idx map"));
            RelocationInfo::page21(r_address, idx)
        }
        RelocKind::PageOff12 { target_sym } => {
            let idx = *extern_idx
                .get(target_sym.as_str())
                .unwrap_or_else(|| panic!("PageOff12 target {target_sym:?} not in extern_idx map"));
            RelocationInfo::pageoff12(r_address, idx)
        }
        RelocKind::AbsPtr64 { target_sym } => {
            let idx = *extern_idx
                .get(target_sym.as_str())
                .unwrap_or_else(|| panic!("AbsPtr64 target {target_sym:?} not in extern_idx map"));
            RelocationInfo::unsigned64(r_address, idx)
        }
    }
}

/// Translate a list of compiled functions into a Mach-O `.o` byte
/// stream.
///
/// Invariants:
///   * Every `CompiledFunction.bytes` length is a multiple of 4
///     (aarch64 fixed-width instruction words — enforced by
///     `torajs_codegen`).
///   * `funcs[i]` corresponds to `FuncId(i)` — same convention as
///     `torajs_core::ssa::Module::funcs[i]`. Defined-symbol index
///     in the resulting `nlist_64` table is `i` for `funcs[i]`.
///   * Symbol names are written into the string table verbatim —
///     callers wire the Mach-O underscore convention themselves
///     (defined fns must already start with `_`, extern call targets
///     emitted by codegen must already include the leading `_`).
pub fn write_object(funcs: &[CompiledFunction]) -> Vec<u8> {
    for f in funcs {
        debug_assert_eq!(
            f.bytes.len() % 4,
            0,
            "CompiledFunction.bytes must be 4-aligned (aarch64 fixed-width instructions); \
             function {:?} has {} bytes",
            f.name,
            f.bytes.len()
        );
    }

    // 1. __text payload: concatenate function bytes; remember each
    //    function's section-relative start offset.
    let mut text: Vec<u8> = Vec::new();
    let mut fn_offsets: Vec<u32> = Vec::with_capacity(funcs.len());
    for f in funcs {
        fn_offsets.push(text.len() as u32);
        text.extend_from_slice(&f.bytes);
    }
    let text_size = text.len() as u32;

    // 2. Symbol partition:
    //    - defined externs in input order (= FuncId order)
    //    - undefined externs in lexicographic order (reproducible)
    let extern_names = collect_externs(funcs);
    let mut extern_idx: BTreeMap<&str, u32> = BTreeMap::new();
    let local_count = funcs.len() as u32;
    for (j, name) in extern_names.iter().enumerate() {
        extern_idx.insert(name.as_str(), local_count + j as u32);
    }

    // 3. String table: sentinel + every defined name + every undef
    //    name (in the same order they'll be emitted in the nlist
    //    table — defined first, then undef).
    let mut strtab = StringTable::new();
    let mut defined_strx: Vec<u32> = Vec::with_capacity(funcs.len());
    for f in funcs {
        defined_strx.push(strtab.add(&f.name));
    }
    let mut extern_strx: Vec<u32> = Vec::with_capacity(extern_names.len());
    for name in &extern_names {
        extern_strx.push(strtab.add(name));
    }
    let strsize = strtab.len();

    // 4. Reloc translation. Each function's relocs translate
    //    independently because `r_address` is section-relative
    //    (= fn_offsets[i] + reloc.byte_offset) and `r_symbolnum`
    //    indexes the global nlist table (FuncId for defined; sorted
    //    extern index for undef).
    let mut reloc_entries: Vec<RelocationInfo> = Vec::new();
    for (i, f) in funcs.iter().enumerate() {
        for r in &f.relocs {
            reloc_entries.push(translate_reloc(r, fn_offsets[i], &extern_idx));
        }
    }
    let reloc_size = (reloc_entries.len() as u32) * RELOCATION_INFO_SIZE;

    // 5. File offsets:
    //    header → segment+section → symtab → __text → relocs →
    //    nlist → strtab.
    let nsyms = local_count + (extern_names.len() as u32);
    let nlist_size = nsyms * NLIST_64_SIZE;

    let ncmds: u32 = 2;
    let sizeofcmds = SEGMENT_COMMAND_64_SIZE + SECTION_64_SIZE + SYMTAB_COMMAND_SIZE;
    let cmds_end = (MachHeader64::SIZE as u32) + sizeofcmds;
    let text_offset = cmds_end;
    let reloc_offset = text_offset + text_size;
    let symoff = reloc_offset + reloc_size;
    let stroff = symoff + nlist_size;
    let total_size = stroff + strsize;

    let mut header = MachHeader64::arm64_object();
    header.ncmds = ncmds;
    header.sizeofcmds = sizeofcmds;

    let mut segment = SegmentCommand64::text_skeleton(TEXT_ALIGN_LOG2);
    segment.vmaddr = 0;
    segment.vmsize = u64::from(text_size);
    segment.fileoff = u64::from(text_offset);
    segment.filesize = u64::from(text_size);
    {
        let section = &mut segment.sections[0];
        section.addr = 0;
        section.size = u64::from(text_size);
        section.offset = text_offset;
        section.reloff = if reloc_entries.is_empty() {
            0
        } else {
            reloc_offset
        };
        section.nreloc = reloc_entries.len() as u32;
    }

    let symtab_cmd = SymtabCommand {
        symoff,
        nsyms,
        stroff,
        strsize,
    };

    // 6. Emit byte stream in file-byte order.
    let mut buf: Vec<u8> = Vec::with_capacity(total_size as usize);
    header.write_to(&mut buf);
    debug_assert_eq!(buf.len(), MachHeader64::SIZE);

    segment.write_to(&mut buf);
    symtab_cmd.write_to(&mut buf);
    debug_assert_eq!(buf.len() as u32, text_offset);

    buf.extend_from_slice(&text);
    debug_assert_eq!(buf.len() as u32, reloc_offset);

    for r in &reloc_entries {
        r.write_to(&mut buf);
    }
    debug_assert_eq!(buf.len() as u32, symoff);

    // Defined externs first, in input/FuncId order.
    for i in 0..funcs.len() {
        let entry = Nlist64::defined_extern(
            defined_strx[i],
            SECTION_TEXT_INDEX,
            u64::from(fn_offsets[i]),
        );
        entry.write_to(&mut buf);
    }
    // Undefined externs second, in lexicographic order (matches
    // `extern_names` build above).
    for j in 0..extern_names.len() {
        let entry = Nlist64::undefined_extern(extern_strx[j]);
        entry.write_to(&mut buf);
    }
    debug_assert_eq!(buf.len() as u32, stroff);

    buf.extend_from_slice(strtab.as_bytes());
    debug_assert_eq!(buf.len() as u32, total_size);

    buf
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_codegen::compile_function;
    use torajs_codegen::reloc::{CallTarget, Reloc, RelocKind};
    use torajs_core::ssa::{
        BinOp, Block, BlockId, FuncId, Function, Inst, InstKind, Operand, Terminator, Type,
        ValueId, ValueInfo,
    };

    use crate::macho::reloc::{ARM64_RELOC_BRANCH26, ARM64_RELOC_UNSIGNED};

    /// Hand-build the `fn _one_plus_two() -> i64 { 1 + 2 }` SSA
    /// `Function`. Mirrors `torajs_codegen::compile::test_fixtures::
    /// build_one_plus_two` (which is `#[cfg(test)] pub(crate)` and
    /// not reachable from this crate) — we keep the symbol name with
    /// the leading `_` so the Mach-O underscore convention is
    /// preserved end-to-end through `write_object`.
    fn build_one_plus_two_with_underscore() -> Function {
        let v0 = ValueId(0);
        Function {
            name: "_one_plus_two".into(),
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
                    kind: InstKind::BinOp(BinOp::Add, Operand::ConstI64(1), Operand::ConstI64(2)),
                    origin: None,
                }],
                term: Terminator::Ret(Some(Operand::Value(v0))),
            }],
            current_origin: None,
        }
    }

    /// Acceptance A — `write_object` for the single-fn, no-reloc
    /// `_one_plus_two` matches the hand-encoded 251-byte Mach-O `.o`
    /// reference layout (Round 5 imm12: the const RHS folds into
    /// `ADD x0, x9, #2`, so the text is 3 instructions / 12 B).
    ///
    /// Each section is annotated with the spec source (Apple
    /// `mach-o/loader.h`) so a future audit can diff slot-for-slot.
    #[test]
    fn write_object_one_plus_two_byte_equal_reference() {
        let func = build_one_plus_two_with_underscore();
        let compiled = compile_function(&func);
        assert!(compiled.relocs.is_empty());
        assert_eq!(compiled.bytes.len(), 12);

        let obj = write_object(std::slice::from_ref(&compiled));

        let mut expected: Vec<u8> = Vec::with_capacity(251);

        // ------ mach_header_64 @ 0..32 ------
        expected.extend_from_slice(&[
            0xCF, 0xFA, 0xED, 0xFE, // magic    = MH_MAGIC_64
            0x0C, 0x00, 0x00, 0x01, // cputype  = CPU_TYPE_ARM64
            0x00, 0x00, 0x00, 0x00, // cpusubtype = CPU_SUBTYPE_ARM64_ALL
            0x01, 0x00, 0x00, 0x00, // filetype = MH_OBJECT
            0x02, 0x00, 0x00, 0x00, // ncmds    = 2 (SEGMENT_64 + SYMTAB)
            0xB0, 0x00, 0x00, 0x00, // sizeofcmds = 176 (152 + 24)
            0x00, 0x00, 0x00, 0x00, // flags    = 0
            0x00, 0x00, 0x00, 0x00, // reserved = 0
        ]);

        // ------ LC_SEGMENT_64 @ 32..104 (72 B) ------
        expected.extend_from_slice(&[
            0x19, 0x00, 0x00, 0x00, // cmd      = LC_SEGMENT_64
            0x98, 0x00, 0x00, 0x00, // cmdsize  = 152 (72 + 80 * 1)
            // segname "__TEXT" + 10 NUL pad to 16 byte:
            b'_', b'_', b'T', b'E', b'X', b'T', 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, // vmaddr   = 0
            0x0C, 0, 0, 0, 0, 0, 0, 0, // vmsize   = 12
            0xD0, 0, 0, 0, 0, 0, 0, 0, // fileoff  = 208
            0x0C, 0, 0, 0, 0, 0, 0, 0, // filesize = 12
            0x07, 0x00, 0x00, 0x00, // maxprot  = VM_PROT_RWX
            0x07, 0x00, 0x00, 0x00, // initprot = VM_PROT_RWX
            0x01, 0x00, 0x00, 0x00, // nsects   = 1
            0x00, 0x00, 0x00, 0x00, // flags    = 0
        ]);

        // ------ section_64 @ 104..184 (80 B) ------
        expected.extend_from_slice(&[
            // sectname "__text" + 10 NUL pad to 16 byte:
            b'_', b'_', b't', b'e', b'x', b't', 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            // segname "__TEXT" + 10 NUL pad to 16 byte:
            b'_', b'_', b'T', b'E', b'X', b'T', 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, // addr   = 0
            0x0C, 0, 0, 0, 0, 0, 0, 0, // size   = 12
            0xD0, 0, 0, 0, // offset = 208
            0x02, 0, 0, 0, // align  = 2 (log2 → 4-byte)
            0x00, 0, 0, 0, // reloff = 0 (S5-a)
            0x00, 0, 0, 0, // nreloc = 0 (S5-a)
            0x00, 0x04, 0x00,
            0x80, // flags  = S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS
            0, 0, 0, 0, // reserved1
            0, 0, 0, 0, // reserved2
            0, 0, 0, 0, // reserved3
        ]);

        // ------ LC_SYMTAB @ 184..208 (24 B) ------
        expected.extend_from_slice(&[
            0x02, 0x00, 0x00, 0x00, // cmd     = LC_SYMTAB
            0x18, 0x00, 0x00, 0x00, // cmdsize = 24
            0xDC, 0x00, 0x00, 0x00, // symoff  = 220 (header 32 + cmds 176 + text 12)
            0x01, 0x00, 0x00, 0x00, // nsyms   = 1
            0xEC, 0x00, 0x00, 0x00, // stroff  = 236 (220 + 1*16)
            0x0F, 0x00, 0x00, 0x00, // strsize = 15 (\0 sentinel + "_one_plus_two\0")
        ]);

        // ------ __TEXT,__text payload @ 208..220 (12 B) ------
        // build_one_plus_two (imm12 form):
        //   MOVZ x9, #1           0xD2800029
        //   ADD  x0, x9, #2       0x91000920
        //   RET                   0xD65F03C0
        expected.extend_from_slice(&[
            0x29, 0x00, 0x80, 0xD2, 0x20, 0x09, 0x00, 0x91, 0xC0, 0x03, 0x5F, 0xD6,
        ]);

        // ------ nlist_64[0] @ 220..236 (16 B) ------
        expected.extend_from_slice(&[
            0x01, 0x00, 0x00, 0x00, // n_strx = 1
            0x0F, // n_type = N_SECT | N_EXT
            0x01, // n_sect = 1 (__TEXT,__text)
            0x00, 0x00, // n_desc = 0
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // n_value = 0
        ]);

        // ------ string table @ 236..251 (15 B) ------
        // \0 sentinel + "_one_plus_two\0"
        expected.extend_from_slice(b"\0_one_plus_two\0");

        assert_eq!(expected.len(), 251, "expected layout = 251 bytes");
        assert_eq!(
            obj, expected,
            "write_object byte stream diverged from hand-encoded reference"
        );
    }

    #[test]
    fn write_object_total_size_matches_layout_formula() {
        // total = header (32) + segment (72) + section (80) +
        //         symtab (24) + text + nsyms * 16 + strsize
        let func = build_one_plus_two_with_underscore();
        let compiled = compile_function(&func);
        let obj = write_object(std::slice::from_ref(&compiled));

        let header_size = MachHeader64::SIZE as u32;
        let cmds = SEGMENT_COMMAND_64_SIZE + SECTION_64_SIZE + SYMTAB_COMMAND_SIZE;
        let text = compiled.bytes.len() as u32;
        let nsyms = 1u32;
        let nlist = nsyms * NLIST_64_SIZE;
        // strtab = "\0" + "_one_plus_two" + "\0" = 1 + 13 + 1 = 15
        let strtab = 15u32;
        let expected_total = header_size + cmds + text + nlist + strtab;

        assert_eq!(obj.len() as u32, expected_total);
        assert_eq!(expected_total, 251);
    }

    #[test]
    fn write_object_round_trip_recovers_fn_bytes_and_name() {
        // Cheap structural round-trip — parse the few fields S5-a
        // populates back out of the byte stream and confirm they
        // match what we fed in. This catches off-by-one offset bugs
        // that the giant byte-equal test would also catch, but in a
        // form that's far easier to debug if a future change shifts
        // a section.
        let func = build_one_plus_two_with_underscore();
        let compiled = compile_function(&func);
        let obj = write_object(std::slice::from_ref(&compiled));

        // mach_header_64.ncmds @ 16..20
        let ncmds = u32::from_le_bytes(obj[16..20].try_into().unwrap());
        assert_eq!(ncmds, 2);

        // section_64.size @ section + 40..48 (section starts at 104)
        let sec_size = u64::from_le_bytes(obj[104 + 40..104 + 48].try_into().unwrap());
        assert_eq!(sec_size, 12);
        // section_64.offset @ section + 48..52
        let sec_off = u32::from_le_bytes(obj[104 + 48..104 + 52].try_into().unwrap());
        assert_eq!(sec_off, 208);

        // text bytes round-trip
        assert_eq!(
            &obj[sec_off as usize..(sec_off as usize) + 12],
            &compiled.bytes[..]
        );

        // symtab_command @ 184..208
        let symoff = u32::from_le_bytes(obj[184 + 8..184 + 12].try_into().unwrap());
        let nsyms = u32::from_le_bytes(obj[184 + 12..184 + 16].try_into().unwrap());
        let stroff = u32::from_le_bytes(obj[184 + 16..184 + 20].try_into().unwrap());
        let strsize = u32::from_le_bytes(obj[184 + 20..184 + 24].try_into().unwrap());
        assert_eq!(symoff, 220);
        assert_eq!(nsyms, 1);
        assert_eq!(stroff, 236);
        assert_eq!(strsize, 15);

        // nlist_64[0] @ symoff..symoff+16
        let n_strx = u32::from_le_bytes(
            obj[symoff as usize..(symoff as usize) + 4]
                .try_into()
                .unwrap(),
        );
        let n_type = obj[(symoff as usize) + 4];
        let n_sect = obj[(symoff as usize) + 5];
        let n_value = u64::from_le_bytes(
            obj[(symoff as usize) + 8..(symoff as usize) + 16]
                .try_into()
                .unwrap(),
        );
        assert_eq!(n_strx, 1);
        assert_eq!(n_type, 0x0F); // N_SECT | N_EXT
        assert_eq!(n_sect, 1);
        assert_eq!(n_value, 0);

        // String table: name at offset n_strx, NUL-terminated.
        let name_start = stroff as usize + n_strx as usize;
        let name_end = obj[name_start..]
            .iter()
            .position(|&b| b == 0)
            .map(|p| name_start + p)
            .expect("name must be NUL-terminated");
        let name = std::str::from_utf8(&obj[name_start..name_end]).unwrap();
        assert_eq!(name, "_one_plus_two");
    }

    /// `fn _foo() -> i64 { 7 + 0 }` — minimal callee for the S5-b
    /// multi-fn fixture. Add(7, 0) (not bare const 7) because
    /// `Ret(Some(Operand::ConstI64(...)))` skips the
    /// `collect_ret_value_ids` pinning that puts the result in x0;
    /// the binop instruction defines a value the allocator can pin.
    /// Compiles to a 3-instruction sequence — the const RHS takes
    /// the Round 5 ADD-imm12 form
    /// (MOVZ x9, #7 / ADD x0, x9, #0 / RET) = 12 B.
    fn build_foo_returns_7() -> Function {
        let v0 = ValueId(0);
        Function {
            name: "_foo".into(),
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
                    kind: InstKind::BinOp(BinOp::Add, Operand::ConstI64(7), Operand::ConstI64(0)),
                    origin: None,
                }],
                term: Terminator::Ret(Some(Operand::Value(v0))),
            }],
            current_origin: None,
        }
    }

    /// `fn _caller() -> i64 { _foo() }`. `Call(FuncId(0), ...)` —
    /// the caller is built assuming the callee lives at `funcs[0]`,
    /// preserving the `FuncId(i) == funcs[i]` convention `Module`
    /// uses.
    fn build_caller_calls_foo(callee: FuncId) -> Function {
        let v0 = ValueId(0);
        Function {
            name: "_caller".into(),
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
                    kind: InstKind::Call(callee, Vec::new()),
                    origin: None,
                }],
                term: Terminator::Ret(Some(Operand::Value(v0))),
            }],
            current_origin: None,
        }
    }

    /// S5-b acceptance — `[_foo, _caller]` where `_caller` invokes
    /// `_foo` via `InstKind::Call(FuncId(0), …)` translates to:
    ///
    ///   * __text payload = foo (12 B) ++ caller (20 B) = 32 B
    ///   * section.nreloc = 1, section.reloff = 208 + 32 = 240
    ///   * one BRANCH26 reloc at section-relative byte 12 + 8 = 20
    ///     (caller_offset + BL site offset), r_symbolnum = 0
    ///     (defined sym index for FuncId(0) = _foo)
    ///   * nsyms = 2 (both defined; no externs)
    ///   * sym[0] = `_foo` @ offset 0, sym[1] = `_caller` @ offset 12
    #[test]
    fn write_object_caller_calls_foo_translates_callsite_reloc() {
        let foo = compile_function(&build_foo_returns_7());
        let caller = compile_function(&build_caller_calls_foo(FuncId(0)));

        assert!(foo.relocs.is_empty(), "_foo is a leaf, has no relocs");
        assert_eq!(foo.bytes.len(), 12);
        assert_eq!(caller.relocs.len(), 1, "_caller has exactly one BL reloc");
        assert_eq!(caller.bytes.len(), 20);
        assert_eq!(
            caller.relocs[0].byte_offset, 8,
            "BL site at offset 8 in _caller"
        );
        match &caller.relocs[0].kind {
            RelocKind::CallSite {
                target: CallTarget::Func(fid),
            } => assert_eq!(*fid, FuncId(0)),
            other => panic!("expected Func(0), got {other:?}"),
        }

        let obj = write_object(&[foo.clone(), caller.clone()]);

        // section_64 starts at 104; size @ +40..48, offset @ +48..52,
        // reloff @ +56..60, nreloc @ +60..64.
        let sec_size = u64::from_le_bytes(obj[104 + 40..104 + 48].try_into().unwrap());
        let sec_offset = u32::from_le_bytes(obj[104 + 48..104 + 52].try_into().unwrap());
        let reloff = u32::from_le_bytes(obj[104 + 56..104 + 60].try_into().unwrap());
        let nreloc = u32::from_le_bytes(obj[104 + 60..104 + 64].try_into().unwrap());
        assert_eq!(sec_size, 32);
        assert_eq!(sec_offset, 208);
        assert_eq!(nreloc, 1);
        assert_eq!(reloff, 208 + 32); // 240

        // Reloc entry @ reloff..reloff+8: bit-packed
        //   r_address @ 0..4 = 20 (caller_offset 12 + BL offset 8)
        //   packed    @ 4..8 = sym 0 | pcrel 1 | length 2 |
        //                      extern 1 | type BRANCH26 (=2)
        let r_address = u32::from_le_bytes(
            obj[reloff as usize..(reloff as usize) + 4]
                .try_into()
                .unwrap(),
        );
        let packed = u32::from_le_bytes(
            obj[(reloff as usize) + 4..(reloff as usize) + 8]
                .try_into()
                .unwrap(),
        );
        assert_eq!(r_address, 20);
        assert_eq!(packed & 0xFF_FFFF, 0, "r_symbolnum = 0 (FuncId(0) → _foo)");
        assert_eq!((packed >> 24) & 1, 1, "r_pcrel = 1");
        assert_eq!((packed >> 25) & 3, 2, "r_length = 2 (4-byte instruction)");
        assert_eq!((packed >> 27) & 1, 1, "r_extern = 1");
        assert_eq!((packed >> 28) & 0xF, u32::from(ARM64_RELOC_BRANCH26));

        // symtab_command @ 184..208: symoff, nsyms, stroff, strsize.
        let symoff = u32::from_le_bytes(obj[184 + 8..184 + 12].try_into().unwrap());
        let nsyms = u32::from_le_bytes(obj[184 + 12..184 + 16].try_into().unwrap());
        let stroff = u32::from_le_bytes(obj[184 + 16..184 + 20].try_into().unwrap());
        let strsize = u32::from_le_bytes(obj[184 + 20..184 + 24].try_into().unwrap());
        assert_eq!(symoff, reloff + 8); // 248
        assert_eq!(nsyms, 2);
        assert_eq!(stroff, symoff + 2 * 16); // 280
        assert_eq!(strsize, 1 + 5 + 8); // "\0" + "_foo\0" (5) + "_caller\0" (8) = 14

        // sym[0] @ symoff: _foo, n_value = 0
        let sym0_strx = u32::from_le_bytes(
            obj[symoff as usize..(symoff as usize) + 4]
                .try_into()
                .unwrap(),
        );
        let sym0_type = obj[(symoff as usize) + 4];
        let sym0_sect = obj[(symoff as usize) + 5];
        let sym0_value = u64::from_le_bytes(
            obj[(symoff as usize) + 8..(symoff as usize) + 16]
                .try_into()
                .unwrap(),
        );
        assert_eq!(sym0_type, 0x0F);
        assert_eq!(sym0_sect, 1);
        assert_eq!(sym0_value, 0);

        // sym[1] @ symoff + 16: _caller, n_value = 12
        let sym1_strx = u32::from_le_bytes(
            obj[(symoff as usize) + 16..(symoff as usize) + 20]
                .try_into()
                .unwrap(),
        );
        let sym1_value = u64::from_le_bytes(
            obj[(symoff as usize) + 24..(symoff as usize) + 32]
                .try_into()
                .unwrap(),
        );
        assert_eq!(sym1_value, 12);

        // String table names round-trip.
        let read_name = |strx: u32| -> String {
            let start = stroff as usize + strx as usize;
            let end = start + obj[start..].iter().position(|&b| b == 0).unwrap();
            std::str::from_utf8(&obj[start..end]).unwrap().to_string()
        };
        assert_eq!(read_name(sym0_strx), "_foo");
        assert_eq!(read_name(sym1_strx), "_caller");

        // Total file size: header (32) + cmds (176) + text (32) +
        // reloc (8) + nlist (32) + strtab (14) = 294.
        assert_eq!(obj.len(), 294);
    }

    /// Extern call sites translate to an undefined `nlist_64` entry
    /// at the end of the symtab (post-defined partition). The reloc
    /// `r_symbolnum` points to the global sym index (= defined
    /// count + lex-sorted extern position).
    #[test]
    fn write_object_extern_call_assigns_undef_sym_after_defined() {
        // Hand-fabricate a `_caller` whose reloc points at extern
        // `_libm_fmod`. We can't get codegen to emit `CallTarget::
        // Extern` directly from a `Call(FuncId, ...)` instruction
        // (FRem is S2-D, not landed), so we splice a synthetic
        // reloc onto an already-compiled fn. This is acceptable
        // for the writer-level test: write_object's contract is to
        // translate whatever `Reloc`s it receives, not to police
        // how they were produced.
        let mut caller = compile_function(&build_caller_calls_foo(FuncId(0)));
        // Replace the existing CallSite/Func reloc with an
        // Extern one targeting "_libm_fmod" (a name codegen would
        // legitimately emit for FRem).
        caller.relocs.clear();
        caller.relocs.push(Reloc {
            byte_offset: 8,
            kind: RelocKind::CallSite {
                target: CallTarget::Extern("_libm_fmod".into()),
            },
        });

        let obj = write_object(std::slice::from_ref(&caller));

        // nsyms = 1 defined (_caller) + 1 undef (_libm_fmod) = 2
        let nsyms = u32::from_le_bytes(obj[184 + 12..184 + 16].try_into().unwrap());
        assert_eq!(nsyms, 2);

        // section.nreloc = 1
        let nreloc = u32::from_le_bytes(obj[104 + 60..104 + 64].try_into().unwrap());
        assert_eq!(nreloc, 1);

        // The single reloc must point at the undef sym (idx 1).
        let reloff = u32::from_le_bytes(obj[104 + 56..104 + 60].try_into().unwrap());
        let packed = u32::from_le_bytes(
            obj[(reloff as usize) + 4..(reloff as usize) + 8]
                .try_into()
                .unwrap(),
        );
        assert_eq!(
            packed & 0xFF_FFFF,
            1,
            "extern sym lands at global idx 1 (after defined _caller)"
        );

        // sym[1] is N_UNDF | N_EXT = 0x01, n_sect = NO_SECT (0).
        let symoff = u32::from_le_bytes(obj[184 + 8..184 + 12].try_into().unwrap());
        let sym1_type = obj[(symoff as usize) + 16 + 4];
        let sym1_sect = obj[(symoff as usize) + 16 + 5];
        assert_eq!(sym1_type, 0x01);
        assert_eq!(sym1_sect, 0);
    }

    /// Externs are deduplicated and sorted lexicographically so the
    /// emitted `.o` is bit-reproducible across runs.
    #[test]
    fn write_object_externs_dedup_and_lex_sort() {
        let mut caller = compile_function(&build_caller_calls_foo(FuncId(0)));
        caller.relocs.clear();
        // Order chosen so insertion != sorted order; "_bar" should
        // come before "_zzz" in the resulting undef partition.
        caller.relocs.push(Reloc {
            byte_offset: 0,
            kind: RelocKind::CallSite {
                target: CallTarget::Extern("_zzz".into()),
            },
        });
        caller.relocs.push(Reloc {
            byte_offset: 4,
            kind: RelocKind::CallSite {
                target: CallTarget::Extern("_bar".into()),
            },
        });
        // Duplicate of _bar — should be deduped.
        caller.relocs.push(Reloc {
            byte_offset: 8,
            kind: RelocKind::CallSite {
                target: CallTarget::Extern("_bar".into()),
            },
        });

        let obj = write_object(std::slice::from_ref(&caller));

        // 1 defined + 2 undef = 3 syms
        let nsyms = u32::from_le_bytes(obj[184 + 12..184 + 16].try_into().unwrap());
        assert_eq!(nsyms, 3);

        // The reloc at byte_offset=0 (zzz, lex-later) must get sym
        // idx 2; the byte_offset=4 (bar, lex-earlier) and the dup at
        // byte_offset=8 both must get sym idx 1.
        let reloff = u32::from_le_bytes(obj[104 + 56..104 + 60].try_into().unwrap());
        let read_packed = |i: usize| {
            let start = reloff as usize + i * 8 + 4;
            u32::from_le_bytes(obj[start..start + 4].try_into().unwrap())
        };
        assert_eq!(
            read_packed(0) & 0xFF_FFFF,
            2,
            "reloc 0 (extern _zzz, r_address=8) → undef idx 2"
        );
        assert_eq!(
            read_packed(1) & 0xFF_FFFF,
            1,
            "reloc 1 (extern _bar, r_address=12) → undef idx 1"
        );
        assert_eq!(
            read_packed(2) & 0xFF_FFFF,
            1,
            "reloc 2 (extern _bar dup, r_address=16) → same undef idx 1"
        );
    }

    /// `Page21 / PageOff12 / AbsPtr64` target syms also land in the
    /// undef partition (same translation path as extern call
    /// targets — none of these refer to defined functions in the
    /// `.o`, by spec).
    #[test]
    fn write_object_page21_and_abs_ptr64_translate_to_undef_sym() {
        let mut caller = compile_function(&build_caller_calls_foo(FuncId(0)));
        caller.relocs.clear();
        // Page21 + PageOff12 pair for a string literal reference;
        // Apple convention is that the ADRP and ADD share one
        // symbol name even though they emit two reloc entries.
        caller.relocs.push(Reloc {
            byte_offset: 0,
            kind: RelocKind::Page21 {
                target_sym: "_str_lit_42".into(),
            },
        });
        caller.relocs.push(Reloc {
            byte_offset: 4,
            kind: RelocKind::PageOff12 {
                target_sym: "_str_lit_42".into(),
            },
        });
        // AbsPtr64 pointing at a vtable global (would normally land
        // in __DATA, not __TEXT, but the writer-level reloc-
        // translation contract is the same).
        caller.relocs.push(Reloc {
            byte_offset: 8,
            kind: RelocKind::AbsPtr64 {
                target_sym: "_vtable_Foo".into(),
            },
        });

        let obj = write_object(std::slice::from_ref(&caller));

        // 1 defined (_caller) + 2 undef (_str_lit_42, _vtable_Foo) = 3
        let nsyms = u32::from_le_bytes(obj[184 + 12..184 + 16].try_into().unwrap());
        assert_eq!(nsyms, 3);

        // 3 reloc entries.
        let nreloc = u32::from_le_bytes(obj[104 + 60..104 + 64].try_into().unwrap());
        assert_eq!(nreloc, 3);

        // reloc[0] = Page21:    type=3, length=2, pcrel=1
        // reloc[1] = PageOff12: type=4, length=2, pcrel=0
        // reloc[2] = AbsPtr64:  type=0, length=3, pcrel=0
        let reloff = u32::from_le_bytes(obj[104 + 56..104 + 60].try_into().unwrap());
        let read_packed = |i: usize| {
            let start = reloff as usize + i * 8 + 4;
            u32::from_le_bytes(obj[start..start + 4].try_into().unwrap())
        };
        let r0 = read_packed(0);
        let r1 = read_packed(1);
        let r2 = read_packed(2);

        // _str_lit_42 lex-sorts before _vtable_Foo → undef idx 1 and 2.
        assert_eq!(r0 & 0xFF_FFFF, 1, "Page21 → _str_lit_42 (idx 1)");
        assert_eq!((r0 >> 28) & 0xF, 3, "type = PAGE21");
        assert_eq!((r0 >> 24) & 1, 1, "PAGE21 r_pcrel = 1");

        assert_eq!(r1 & 0xFF_FFFF, 1, "PageOff12 → _str_lit_42 (idx 1)");
        assert_eq!((r1 >> 28) & 0xF, 4, "type = PAGEOFF12");
        assert_eq!((r1 >> 24) & 1, 0, "PAGEOFF12 r_pcrel = 0");

        assert_eq!(r2 & 0xFF_FFFF, 2, "AbsPtr64 → _vtable_Foo (idx 2)");
        assert_eq!((r2 >> 28) & 0xF, u32::from(ARM64_RELOC_UNSIGNED));
        assert_eq!((r2 >> 25) & 3, 3, "UNSIGNED r_length = 3 (8 byte)");
    }

    #[test]
    fn write_object_empty_funcs_emits_minimal_skeleton() {
        // Zero functions still produces a valid 32 + 152 + 24 + 1 =
        // 209-byte file: the strtab still carries its sentinel `\0`
        // so `nm` can read it. nsyms=0 means the symtab is empty.
        let obj = write_object(&[]);
        // header (32) + cmds (176) + text (0) + nlist (0) + strtab (1)
        assert_eq!(obj.len(), 32 + 176 + 1);

        // ncmds still = 2
        let ncmds = u32::from_le_bytes(obj[16..20].try_into().unwrap());
        assert_eq!(ncmds, 2);

        // section_64.size = 0
        let sec_size = u64::from_le_bytes(obj[104 + 40..104 + 48].try_into().unwrap());
        assert_eq!(sec_size, 0);

        // symtab.nsyms = 0, strsize = 1 (just the sentinel)
        let nsyms = u32::from_le_bytes(obj[184 + 12..184 + 16].try_into().unwrap());
        let strsize = u32::from_le_bytes(obj[184 + 20..184 + 24].try_into().unwrap());
        assert_eq!(nsyms, 0);
        assert_eq!(strsize, 1);
    }

    /// S5-c host-toolchain sanity check — write the multi-fn .o to
    /// a temp file and run Apple's `otool -hlrtv` against it. The
    /// .o is real only if a stock Apple toolchain accepts it; this
    /// test catches layout regressions our hand-encoded asserts
    /// might miss (e.g. an `MH_OBJECT` flag the Apple parser
    /// enforces strictly but our internal struct tolerates).
    ///
    /// Gated to `target_os = "macos" + target_arch = "aarch64"` —
    /// the only host where `otool` ships by default and where the
    /// ARM64 architecture matches our emitted CPU type. Non-macOS
    /// CI / cross-build hosts skip silently (no fail).
    ///
    /// If `otool` is missing or non-executable on a macOS host (rare:
    /// developer hasn't installed Xcode CLT), we `eprintln` and skip
    /// rather than fail — the offline byte-equal tests above already
    /// pin the layout; this test is a "smell check against the real
    /// parser", not the primary acceptance gate.
    #[test]
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    fn otool_accepts_emitted_multi_fn_object() {
        use std::process::Command;

        let foo = compile_function(&build_foo_returns_7());
        let caller = compile_function(&build_caller_calls_foo(FuncId(0)));
        let obj = write_object(&[foo, caller]);

        // /tmp on macOS is a symlink to /private/tmp — write to the
        // real path so paths in test output are unambiguous.
        let path = format!("/private/tmp/torajs_obj_s5c_{}.o", std::process::id());
        std::fs::write(&path, &obj).expect("write .o");

        let output = match Command::new("/usr/bin/otool")
            .args(["-hlrtv", &path])
            .output()
        {
            Ok(o) => o,
            Err(e) => {
                eprintln!("skip: otool not invokable: {e}");
                let _ = std::fs::remove_file(&path);
                return;
            }
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let _ = std::fs::remove_file(&path);
            panic!(
                "otool failed (exit {:?}):\n--- stderr ---\n{stderr}\n--- stdout ---\n{stdout}",
                output.status.code()
            );
        }

        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let _ = std::fs::remove_file(&path);

        // Required header / section / symtab / reloc / disasm
        // markers — Apple's MH_OBJECT decoded form. `BR26` is the
        // short name otool prints for `ARM64_RELOC_BRANCH26`; the
        // disasm includes `bl` resolving to `_foo` via the reloc.
        for needle in [
            "MH_MAGIC_64", // mach_header_64.magic
            "ARM64",       // cputype
            "OBJECT",      // filetype = MH_OBJECT
            "__TEXT",      // segment name
            "__text",      // section name
            "_foo",        // sym[0]
            "_caller",     // sym[1]
            "BR26",        // ARM64_RELOC_BRANCH26 short name
            "bl",          // disasm: the BL instruction at the reloc site
        ] {
            assert!(
                stdout.contains(needle),
                "otool output missing {needle:?}; full output:\n{stdout}"
            );
        }
    }
}
