//! High-level Mach-O object builder.
//!
//! Translates a `&[CompiledFunction]` (the per-function output of
//! `torajs_codegen::compile_function`) into a complete Mach-O 64
//! `MH_OBJECT` byte stream that downstream tools (`otool` / Apple
//! `ld` / torajs-link #8) can consume.
//!
//! S5-a (this commit) ships the scaffold: single-function, no-reloc
//! path. Functions whose `relocs` are non-empty cause `write_object`
//! to panic with a clear "S5-b lands reloc translation" message —
//! that machinery (sym-index map + `RelocKind` → `RelocationInfo`
//! translation) wires up in the next sub-step. S5-c lands the
//! optional macOS host link round-trip; S5-d adds the phase-close
//! docs marker.
//!
//! File-byte layout (the strict order Apple `mach-o/loader.h`
//! expects from a `.o`):
//!
//! ```text
//!   [   0]                       mach_header_64           (32 B)
//!   [  32]                       LC_SEGMENT_64 + section  (72 + 80 = 152 B)
//!   [ 184]                       LC_SYMTAB                (24 B)
//!   [ 208]                       __TEXT,__text payload    (4-aligned)
//!   [ 208 + text_size]           relocation entries       (S5-b)
//!   [ ... + reloc_size]          nlist_64 table           (16 B per sym)
//!   [ ... + nsyms*16]            string table             ("\0" sentinel + names)
//! ```
//!
//! The two load commands (`LC_SEGMENT_64`, `LC_SYMTAB`) are fixed in
//! order — `clang -c` emits them in this exact sequence and Apple
//! `ld` reads them positionally for the common `.o` shape. Once
//! `LC_DYSYMTAB` lands (S5-b, with relocations carving local/extdef/
//! undef sub-ranges), the third load command appears immediately
//! after `LC_SYMTAB`.

use torajs_codegen::CompiledFunction;

use crate::macho::header::MachHeader64;
use crate::macho::segment::{SECTION_64_SIZE, SEGMENT_COMMAND_64_SIZE, SegmentCommand64};
use crate::macho::symtab::{
    NLIST_64_SIZE, Nlist64, SYMTAB_COMMAND_SIZE, StringTable, SymtabCommand,
};

/// `align_log2 = 2` matches what `clang -c` emits for `__TEXT,__text`
/// holding aarch64 code: 2^2 = 4-byte instruction-word alignment.
/// S5-b can lift this to 4 (16-byte function-prologue alignment)
/// once multi-fn layout lands and inter-fn padding is needed.
const TEXT_ALIGN_LOG2: u32 = 2;

/// Section index in the in-memory `__TEXT,__text` segment. Mach-O
/// `n_sect` is 1-based and the only section we emit in S5-a is
/// `__text` at slot 1.
const SECTION_TEXT_INDEX: u8 = 1;

/// Translate a list of compiled functions into a Mach-O `.o` byte
/// stream.
///
/// S5-a invariants:
///   * Every `CompiledFunction.bytes` length is a multiple of 4
///     (aarch64 fixed-width instruction words — enforced by
///     `torajs_codegen`).
///   * `CompiledFunction.relocs` is empty. The S5-a scaffold rejects
///     non-empty relocs with a panic; S5-b lifts that limit.
///   * Symbol names are written into the string table verbatim (no
///     automatic `_` prefix). Callers wire the Mach-O underscore
///     convention themselves so the API matches what an external
///     linker reads from `nm` byte-for-byte.
pub fn write_object(funcs: &[CompiledFunction]) -> Vec<u8> {
    for f in funcs {
        assert!(
            f.relocs.is_empty(),
            "torajs-obj S5-a: reloc translation lands in S5-b — function {:?} has {} reloc(s)",
            f.name,
            f.relocs.len()
        );
        debug_assert_eq!(
            f.bytes.len() % 4,
            0,
            "CompiledFunction.bytes must be 4-aligned (aarch64 fixed-width instructions); \
             function {:?} has {} bytes",
            f.name,
            f.bytes.len()
        );
    }

    let mut text: Vec<u8> = Vec::new();
    let mut fn_offsets: Vec<u32> = Vec::with_capacity(funcs.len());
    for f in funcs {
        fn_offsets.push(text.len() as u32);
        text.extend_from_slice(&f.bytes);
    }
    let text_size = text.len() as u32;

    let mut strtab = StringTable::new();
    let mut name_strx: Vec<u32> = Vec::with_capacity(funcs.len());
    for f in funcs {
        name_strx.push(strtab.add(&f.name));
    }
    let strsize = strtab.len();

    let nsyms = funcs.len() as u32;
    let nlist_size = nsyms * NLIST_64_SIZE;

    let ncmds: u32 = 2;
    let sizeofcmds = SEGMENT_COMMAND_64_SIZE + SECTION_64_SIZE + SYMTAB_COMMAND_SIZE;
    let cmds_end = (MachHeader64::SIZE as u32) + sizeofcmds;
    let text_offset = cmds_end;
    let reloc_offset = text_offset + text_size;
    let symoff = reloc_offset;
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
        section.reloff = 0;
        section.nreloc = 0;
    }

    let symtab_cmd = SymtabCommand {
        symoff,
        nsyms,
        stroff,
        strsize,
    };

    let mut buf: Vec<u8> = Vec::with_capacity(total_size as usize);
    header.write_to(&mut buf);
    debug_assert_eq!(buf.len(), MachHeader64::SIZE);

    segment.write_to(&mut buf);
    symtab_cmd.write_to(&mut buf);
    debug_assert_eq!(buf.len() as u32, text_offset);

    buf.extend_from_slice(&text);
    debug_assert_eq!(buf.len() as u32, symoff);

    for i in 0..funcs.len() {
        let entry =
            Nlist64::defined_extern(name_strx[i], SECTION_TEXT_INDEX, u64::from(fn_offsets[i]));
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
    use torajs_core::ssa::{
        BinOp, Block, BlockId, Function, Inst, InstKind, Operand, Terminator, Type, ValueId,
        ValueInfo,
    };

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
    /// `_one_plus_two` matches the hand-encoded 255-byte Mach-O `.o`
    /// reference layout.
    ///
    /// Each section is annotated with the spec source (Apple
    /// `mach-o/loader.h`) so a future audit can diff slot-for-slot.
    #[test]
    fn write_object_one_plus_two_byte_equal_reference() {
        let func = build_one_plus_two_with_underscore();
        let compiled = compile_function(&func);
        assert!(compiled.relocs.is_empty());
        assert_eq!(compiled.bytes.len(), 16);

        let obj = write_object(std::slice::from_ref(&compiled));

        let mut expected: Vec<u8> = Vec::with_capacity(255);

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
            0x10, 0, 0, 0, 0, 0, 0, 0, // vmsize   = 16
            0xD0, 0, 0, 0, 0, 0, 0, 0, // fileoff  = 208
            0x10, 0, 0, 0, 0, 0, 0, 0, // filesize = 16
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
            0x10, 0, 0, 0, 0, 0, 0, 0, // size   = 16
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
            0xE0, 0x00, 0x00, 0x00, // symoff  = 224 (header 32 + cmds 176 + text 16)
            0x01, 0x00, 0x00, 0x00, // nsyms   = 1
            0xF0, 0x00, 0x00, 0x00, // stroff  = 240 (224 + 1*16)
            0x0F, 0x00, 0x00, 0x00, // strsize = 15 (\0 sentinel + "_one_plus_two\0")
        ]);

        // ------ __TEXT,__text payload @ 208..224 (16 B) ------
        // build_one_plus_two:
        //   MOVZ x9,  #1         0xD2800029
        //   MOVZ x10, #2          0xD280004A
        //   ADD  x0, x9, x10      0x8B0A0120
        //   RET                   0xD65F03C0
        expected.extend_from_slice(&[
            0x29, 0x00, 0x80, 0xD2, 0x4A, 0x00, 0x80, 0xD2, 0x20, 0x01, 0x0A, 0x8B, 0xC0, 0x03,
            0x5F, 0xD6,
        ]);

        // ------ nlist_64[0] @ 224..240 (16 B) ------
        expected.extend_from_slice(&[
            0x01, 0x00, 0x00, 0x00, // n_strx = 1
            0x0F, // n_type = N_SECT | N_EXT
            0x01, // n_sect = 1 (__TEXT,__text)
            0x00, 0x00, // n_desc = 0
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // n_value = 0
        ]);

        // ------ string table @ 240..255 (15 B) ------
        // \0 sentinel + "_one_plus_two\0"
        expected.extend_from_slice(b"\0_one_plus_two\0");

        assert_eq!(expected.len(), 255, "expected layout = 255 bytes");
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
        assert_eq!(expected_total, 255);
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
        assert_eq!(sec_size, 16);
        // section_64.offset @ section + 48..52
        let sec_off = u32::from_le_bytes(obj[104 + 48..104 + 52].try_into().unwrap());
        assert_eq!(sec_off, 208);

        // text bytes round-trip
        assert_eq!(
            &obj[sec_off as usize..(sec_off as usize) + 16],
            &compiled.bytes[..]
        );

        // symtab_command @ 184..208
        let symoff = u32::from_le_bytes(obj[184 + 8..184 + 12].try_into().unwrap());
        let nsyms = u32::from_le_bytes(obj[184 + 12..184 + 16].try_into().unwrap());
        let stroff = u32::from_le_bytes(obj[184 + 16..184 + 20].try_into().unwrap());
        let strsize = u32::from_le_bytes(obj[184 + 20..184 + 24].try_into().unwrap());
        assert_eq!(symoff, 224);
        assert_eq!(nsyms, 1);
        assert_eq!(stroff, 240);
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

    #[test]
    #[should_panic(expected = "torajs-obj S5-a: reloc translation lands in S5-b")]
    fn write_object_panics_on_non_empty_relocs() {
        use torajs_codegen::reloc::{CallTarget, Reloc, RelocKind};

        let mut cf = compile_function(&build_one_plus_two_with_underscore());
        cf.relocs.push(Reloc {
            byte_offset: 0,
            kind: RelocKind::CallSite {
                target: CallTarget::Extern("_does_not_matter".into()),
            },
        });
        let _ = write_object(std::slice::from_ref(&cf));
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
}
