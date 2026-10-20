//! Reloc resolver — walks per-function reloc tables, resolves
//! every target symbol to its final virtual address via the link
//! context's `SymTable`, and patches the affected instruction
//! word (or 8-byte data slot) in place against a mutable copy of
//! the function bytes.
//!
//! Inputs:
//! - `funcs: &[CompiledFunction]` — codegen output (bytes + relocs +
//!   name). The convention `FuncId(i) == funcs[i]` mirrors
//!   `torajs_core::ssa::Module::funcs`.
//! - `fn_vaddrs: &[u64]` — pre-computed final virtual address for
//!   each function (one entry per `funcs[i]`, same indexing). This
//!   comes from the layout pass (S3); for unit tests it's an
//!   arbitrary `u64` that mocks the post-layout vaddr.
//! - `sym_table: &SymTable` — external symbol name → vaddr map,
//!   for `CallTarget::Extern` and `Page21/PageOff12/AbsPtr64`
//!   target_sym that don't resolve to a defined function in
//!   `funcs`. Future: the staticlib (`.a`) symbol-extraction pass
//!   (S7) feeds this map; for now callers supply it directly.
//!
//! Output: one `ResolvedFunction` per input function, with `bytes`
//! cloned and patched in place. The driver's next stage (S3 layout)
//! concatenates `bytes` into the final `__TEXT` payload at
//! `vaddr` offsets that already match the displacements baked in
//! here.

use std::collections::{BTreeMap, HashMap};

use torajs_codegen::CompiledFunction;
use torajs_codegen::id_hash::HashMap as IdHashMap;
use torajs_codegen::reloc::{CallTarget, RelocKind};

use crate::fn_addr_syms::fn_addr_alias_index;
use crate::patch::{patch_branch26, patch_page21, patch_pageoff12, write_unsigned64};

/// External-symbol resolution map: name → final virtual address.
pub type SymTable = BTreeMap<String, u64>;

/// The final symbol table of one link, each name borrowed from where
/// it is defined (a member, a user fn, the link's own tables) instead
/// of copied into an owned key.
pub type SymIndex<'s> = IdHashMap<&'s str, u64>;

/// A name → final virtual address table the reloc passes resolve
/// against.
pub trait SymLookup {
    fn vaddr(&self, name: &str) -> Option<u64>;
}

impl SymLookup for SymTable {
    fn vaddr(&self, name: &str) -> Option<u64> {
        self.get(name).copied()
    }
}

impl SymLookup for SymIndex<'_> {
    fn vaddr(&self, name: &str) -> Option<u64> {
        self.get(name).copied()
    }
}

/// One function's bytes after every reloc has been resolved and
/// patched against the link context.
#[derive(Debug, Clone)]
pub struct ResolvedFunction {
    pub name: String,
    /// Final virtual address where this function's bytes will sit
    /// in the linked image's `__TEXT,__text`.
    pub vaddr: u64,
    pub bytes: Vec<u8>,
}

/// Walk each function's reloc table and patch the affected
/// instruction / 8-byte slot in place.
///
/// Panics on:
/// - `fn_vaddrs.len() != funcs.len()` (caller bug — layout pass
///   must enumerate every function).
/// - `CallTarget::Func(fid)` with `fid.0 >= funcs.len()` (stale
///   FuncId — codegen emitted a reloc for a function the linker
///   doesn't know about).
/// - `CallSite::Extern` / `Page21` / `PageOff12` / `AbsPtr64`
///   target_sym not in `sym_table` and not a defined function name
///   (unresolved external — the linker can't fabricate addresses).
/// - BRANCH26 displacement overflow (silent corruption would be
///   strictly worse than a loud panic; surfaces an over-sized
///   `__TEXT` segment as a build-time error).
/// - PAGEOFF12 on a scaled load / store whose target is not aligned to
///   the access size (the immediate would reach the word below it).
pub fn apply_relocs(
    funcs: &[CompiledFunction],
    fn_vaddrs: &[u64],
    sym_table: &dyn SymLookup,
) -> Vec<ResolvedFunction> {
    assert_eq!(
        funcs.len(),
        fn_vaddrs.len(),
        "fn_vaddrs must have one entry per CompiledFunction (got {} vs {})",
        fn_vaddrs.len(),
        funcs.len()
    );

    let mut out: Vec<ResolvedFunction> = Vec::with_capacity(funcs.len());
    let targets = Targets::new(funcs, fn_vaddrs, sym_table);

    for (i, f) in funcs.iter().enumerate() {
        let mut bytes = f.bytes.clone();
        let fn_vaddr = fn_vaddrs[i];

        for r in &f.relocs {
            let target_vaddr = targets.resolve(&r.kind);
            apply_one(&mut bytes, r.byte_offset, fn_vaddr, &r.kind, target_vaddr);
        }

        out.push(ResolvedFunction {
            name: f.name.clone(),
            vaddr: fn_vaddr,
            bytes,
        });
    }

    out
}

fn apply_one(
    bytes: &mut [u8],
    byte_offset: u32,
    fn_vaddr: u64,
    kind: &RelocKind,
    target_vaddr: u64,
) {
    let off = byte_offset as usize;
    let site_vaddr = fn_vaddr + u64::from(byte_offset);

    match kind {
        RelocKind::CallSite { .. } => {
            let displacement = (target_vaddr as i64) - (site_vaddr as i64);
            let displacement_i32 = i32::try_from(displacement).unwrap_or_else(|_| {
                panic!(
                    "BRANCH26 displacement {displacement} from 0x{site_vaddr:X} to 0x{target_vaddr:X} overflows i32"
                )
            });
            let insn = read_u32_le(bytes, off);
            let patched = patch_branch26(insn, displacement_i32);
            write_u32_le(bytes, off, patched);
        }
        RelocKind::Page21 { .. } => {
            let target_page = (target_vaddr & !0xFFF) as i64;
            let site_page = (site_vaddr & !0xFFF) as i64;
            let displacement = target_page - site_page;
            let insn = read_u32_le(bytes, off);
            let patched = patch_page21(insn, displacement);
            write_u32_le(bytes, off, patched);
        }
        RelocKind::PageOff12 { .. } => {
            let offset = (target_vaddr & 0xFFF) as u32;
            let insn = read_u32_le(bytes, off);
            let patched = patch_pageoff12(insn, offset).unwrap_or_else(|e| {
                panic!(
                    "PAGEOFF12 target 0x{target_vaddr:X} at 0x{site_vaddr:X} is not aligned to the {}-byte access of 0x{:08X}",
                    1u32 << e.scale,
                    e.insn
                )
            });
            write_u32_le(bytes, off, patched);
        }
        RelocKind::AbsPtr64 { .. } => {
            let slot: &mut [u8; 8] = (&mut bytes[off..off + 8])
                .try_into()
                .expect("AbsPtr64 needs an 8-byte data slot");
            write_unsigned64(slot, target_vaddr);
        }
    }
}

/// Where a reloc's target lands: user fns by `FuncId` or by name,
/// then the fn address aliases, then the sym table.
struct Targets<'a> {
    /// the first fn of each name, the one a front-to-back scan finds
    by_name: HashMap<&'a str, usize>,
    fn_vaddrs: &'a [u64],
    sym_table: &'a dyn SymLookup,
}

impl<'a> Targets<'a> {
    fn new(
        funcs: &'a [CompiledFunction],
        fn_vaddrs: &'a [u64],
        sym_table: &'a dyn SymLookup,
    ) -> Self {
        let mut by_name = HashMap::with_capacity(funcs.len());
        for (i, f) in funcs.iter().enumerate() {
            by_name.entry(f.name.as_str()).or_insert(i);
        }
        Targets {
            by_name,
            fn_vaddrs,
            sym_table,
        }
    }

    fn resolve(&self, kind: &RelocKind) -> u64 {
        match kind {
            RelocKind::CallSite {
                target: CallTarget::Func(fid),
            } => {
                let idx = fid.0 as usize;
                assert!(
                    idx < self.fn_vaddrs.len(),
                    "CallSite::Func references FuncId({}) but only {} functions in input",
                    fid.0,
                    self.fn_vaddrs.len()
                );
                self.fn_vaddrs[idx]
            }
            RelocKind::CallSite {
                target: CallTarget::Extern(name),
            } => self.lookup(name),
            RelocKind::Page21 { target_sym }
            | RelocKind::PageOff12 { target_sym }
            | RelocKind::AbsPtr64 { target_sym } => self.lookup(target_sym),
        }
    }

    fn lookup(&self, name: &str) -> u64 {
        if let Some(&i) = self.by_name.get(name) {
            return self.fn_vaddrs[i];
        }
        // a user fn's address alias resolves straight off the vaddr table,
        // ahead of any same-named entry in `sym_table`
        if let Some(i) = fn_addr_alias_index(name).filter(|&i| i < self.fn_vaddrs.len()) {
            return self.fn_vaddrs[i];
        }
        self.sym_table.vaddr(name).unwrap_or_else(|| {
            panic!("unresolved symbol {name:?} (not a defined function, not in sym_table)")
        })
    }
}

fn read_u32_le(bytes: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap())
}

fn write_u32_le(bytes: &mut [u8], off: usize, value: u32) {
    bytes[off..off + 4].copy_from_slice(&value.to_le_bytes());
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

    /// `fn _foo() -> i64 { 7 + 0 }` — same shape as the
    /// torajs-obj S5-b multi-fn fixture; compiles to 16 bytes.
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

    /// `fn _caller() -> i64 { _foo() }` calling `FuncId(0)` (i.e.
    /// the callee at `funcs[0]`). 20 bytes, one BL reloc at
    /// byte_offset 8.
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

    /// Headline acceptance — `_caller` calling `_foo`. Place
    /// `_foo` at 0x100000 and `_caller` at 0x100020 (32 byte gap).
    /// The BL site at `_caller + 8 = 0x100028` must encode a
    /// displacement of `0x100000 - 0x100028 = -0x28 = -40 B`.
    /// imm26 = -40 / 4 = -10 = 0x03FF_FFF6 in 26-bit two's
    /// complement. BL opcode 0x94000000 patched → 0x97FFFFF6.
    #[test]
    fn apply_relocs_patches_bl_displacement_in_place() {
        let foo = compile_function(&build_foo_returns_7());
        let caller = compile_function(&build_caller_calls_foo(FuncId(0)));

        // Confirm pre-patch state: BL site at caller[8..12] is
        // the unpatched 0x94000000 instruction word.
        assert_eq!(caller.relocs.len(), 1);
        assert_eq!(caller.relocs[0].byte_offset, 8);
        let pre_bl = u32::from_le_bytes(caller.bytes[8..12].try_into().unwrap());
        assert_eq!(pre_bl, 0x9400_0000, "pre-patch BL = bare opcode");

        let fn_vaddrs = vec![0x0000_0000_0010_0000, 0x0000_0000_0010_0020];
        let sym = SymTable::new();
        let out = apply_relocs(&[foo.clone(), caller.clone()], &fn_vaddrs, &sym);

        // _foo unchanged (no relocs).
        assert_eq!(out[0].bytes, foo.bytes);
        assert_eq!(out[0].vaddr, 0x100000);
        assert_eq!(out[0].name, "_foo");

        // _caller BL site patched.
        let post_bl = u32::from_le_bytes(out[1].bytes[8..12].try_into().unwrap());
        // Expected imm26 = -10 = 0x03FFFFF6.
        // patched = 0x94000000 | 0x03FFFFF6 = 0x97FFFFF6.
        assert_eq!(post_bl, 0x97FF_FFF6);
        assert_eq!(post_bl & 0x03FF_FFFF, 0x03FF_FFF6);
    }

    /// External symbol resolution — `_caller` calling extern
    /// `_libm_fmod` (codegen would emit this for FRem). The link
    /// driver looks up `_libm_fmod` in `sym_table`, computes the
    /// displacement, and patches in place.
    #[test]
    fn apply_relocs_resolves_extern_call() {
        let mut caller = compile_function(&build_caller_calls_foo(FuncId(0)));
        // Replace the FuncId reloc with an Extern one targeting
        // _libm_fmod.
        caller.relocs.clear();
        caller.relocs.push(Reloc {
            byte_offset: 8,
            kind: RelocKind::CallSite {
                target: CallTarget::Extern("_libm_fmod".into()),
            },
        });

        let fn_vaddrs = vec![0x100020];
        let mut sym = SymTable::new();
        sym.insert("_libm_fmod".into(), 0x100040);

        let out = apply_relocs(std::slice::from_ref(&caller), &fn_vaddrs, &sym);

        // Site = 0x100020 + 8 = 0x100028; target = 0x100040.
        // Displacement = 0x100040 - 0x100028 = +24 B → imm26 = 6.
        let post_bl = u32::from_le_bytes(out[0].bytes[8..12].try_into().unwrap());
        assert_eq!(post_bl & 0x03FF_FFFF, 6, "extern call displacement / 4 = 6");
        assert_eq!(post_bl, 0x9400_0006);
    }

    /// Page21 + PageOff12 pair resolution — both patchers fire
    /// over the same `target_sym`. Synthetic instruction bytes
    /// stand in for an ADRP + ADD pair codegen would emit at the
    /// head of a `StringRef` / `StaticStrRef` lowering.
    #[test]
    fn apply_relocs_resolves_page21_and_pageoff12_pair() {
        // Build a leaf function whose bytes we'll hand-pack: ADRP
        // at offset 0, ADD at offset 4, RET at offset 8. The two
        // reloc kinds attach to the matching offsets.
        let mut caller = compile_function(&build_caller_calls_foo(FuncId(0)));
        // Overwrite the body to be a 12-byte sequence carrying an
        // ADRP + ADD + RET. We only care that apply_relocs touches
        // the right bytes; the body shape is contrived.
        caller.bytes = vec![
            0x00, 0x00, 0x00, 0x90, // ADRP X0, #0 (immlo+immhi zeroed)
            0x00, 0x00, 0x00, 0x91, // ADD X0, X0, #0
            0xC0, 0x03, 0x5F, 0xD6, // RET
        ];
        caller.relocs.clear();
        caller.relocs.push(Reloc {
            byte_offset: 0,
            kind: RelocKind::Page21 {
                target_sym: "_str_lit_0".into(),
            },
        });
        caller.relocs.push(Reloc {
            byte_offset: 4,
            kind: RelocKind::PageOff12 {
                target_sym: "_str_lit_0".into(),
            },
        });

        // Place caller at page 0x100000, target string at page
        // 0x101000 + offset 0x123. So page displacement = +0x1000
        // and PAGEOFF12 imm12 = 0x123.
        let fn_vaddrs = vec![0x100000];
        let mut sym = SymTable::new();
        sym.insert("_str_lit_0".into(), 0x101123);

        let out = apply_relocs(std::slice::from_ref(&caller), &fn_vaddrs, &sym);

        // ADRP: immlo = 1 (bit 30..29 = 0b01), immhi = 0. Base
        // ADRP X0 word = 0x90000000 → patched = 0xB0000000.
        let adrp = u32::from_le_bytes(out[0].bytes[0..4].try_into().unwrap());
        assert_eq!(adrp, 0xB000_0000, "ADRP patched with +1 page");

        // ADD: imm12 = 0x123 → bit 21..10 = 0x123 << 10 = 0x48C00.
        // Base ADD X0,X0,#0 = 0x91000000 → patched = 0x91048C00.
        let add = u32::from_le_bytes(out[0].bytes[4..8].try_into().unwrap());
        assert_eq!(add, 0x9104_8C00);
        assert_eq!((add >> 10) & 0xFFF, 0x123);

        // RET untouched.
        let ret = u32::from_le_bytes(out[0].bytes[8..12].try_into().unwrap());
        assert_eq!(ret, 0xD65F_03C0);
    }

    /// Symbol resolution preference: if a target name matches a
    /// defined function AND lives in sym_table, the defined
    /// function's vaddr wins. This keeps in-source intrinsics
    /// (when later torajs adds intrinsic-inlining) from being
    /// shadowed by an accidental staticlib symbol export.
    #[test]
    fn apply_relocs_defined_function_wins_over_sym_table() {
        let mut caller = compile_function(&build_caller_calls_foo(FuncId(0)));
        // Replace the FuncId reloc with a Page21 targeting "_foo"
        // — same name as the defined callee.
        caller.bytes = vec![
            0x00, 0x00, 0x00, 0x90, // ADRP X0, #0
            0xC0, 0x03, 0x5F, 0xD6, // RET
        ];
        caller.relocs.clear();
        caller.relocs.push(Reloc {
            byte_offset: 0,
            kind: RelocKind::Page21 {
                target_sym: "_foo".into(),
            },
        });

        let foo = compile_function(&build_foo_returns_7());

        // Defined _foo at page 0x100000, _caller at 0x200000.
        // sym_table also lists "_foo" at a bogus 0xDEAD_PAGE —
        // must NOT win.
        let fn_vaddrs = vec![0x100000, 0x200000];
        let mut sym = SymTable::new();
        sym.insert("_foo".into(), 0xDEAD_0000);

        let out = apply_relocs(&[foo, caller], &fn_vaddrs, &sym);

        let adrp = u32::from_le_bytes(out[1].bytes[0..4].try_into().unwrap());
        // Site page = _caller page (0x200000); target page = _foo
        // page (0x100000). Page displacement = -0x100000 B = -256
        // pages → imm21 = -256 = 0x1FFF00 (21-bit two's complement).
        //   immlo = imm21 & 0b11   = 0
        //   immhi = (imm21 >> 2) & 0x7FFFF = 0x7FFC0
        //   patched bits = (immhi << 5) = 0x00FFF800
        // patched = 0x90000000 | 0x00FFF800 = 0x90FFF800.
        assert_eq!(adrp, 0x90FF_F800);
        // Sanity: extract back and confirm imm21 = -256.
        let immlo = (adrp >> 29) & 0b11;
        let immhi = (adrp >> 5) & 0x7_FFFF;
        let imm21_u = (immhi << 2) | immlo;
        let imm21 = ((imm21_u as i32) << 11) >> 11;
        assert_eq!(imm21, -256);
    }

    /// AbsPtr64: 8-byte slot in a data section gets written with
    /// the target's full virtual address, little-endian.
    #[test]
    fn apply_relocs_writes_unsigned64_data_slot() {
        let mut caller = compile_function(&build_caller_calls_foo(FuncId(0)));
        // 8-byte zero-filled data slot followed by a RET (not
        // executable, just shape).
        caller.bytes = vec![
            0, 0, 0, 0, 0, 0, 0, 0, //
            0xC0, 0x03, 0x5F, 0xD6, // RET
        ];
        caller.relocs.clear();
        caller.relocs.push(Reloc {
            byte_offset: 0,
            kind: RelocKind::AbsPtr64 {
                target_sym: "_vtable_Foo".into(),
            },
        });

        let fn_vaddrs = vec![0x100000];
        let mut sym = SymTable::new();
        sym.insert("_vtable_Foo".into(), 0x0000_0002_DEAD_BEEF);

        let out = apply_relocs(std::slice::from_ref(&caller), &fn_vaddrs, &sym);

        assert_eq!(
            &out[0].bytes[0..8],
            &[0xEF, 0xBE, 0xAD, 0xDE, 0x02, 0x00, 0x00, 0x00]
        );
        // RET untouched.
        assert_eq!(
            u32::from_le_bytes(out[0].bytes[8..12].try_into().unwrap()),
            0xD65F_03C0
        );
    }

    /// `__torajs_fn_<i>` / `__torajs_boxed_<i>` resolve to
    /// `fn_vaddrs[i]` over a same-named table entry; a spelling the
    /// aliases never use, or an index past the fns, goes to the table.
    #[test]
    fn fn_addr_aliases_resolve_off_the_vaddr_table() {
        let funcs = [
            compile_function(&build_foo_returns_7()),
            compile_function(&build_caller_calls_foo(FuncId(0))),
        ];
        let fn_vaddrs = [0x10_0000, 0x10_0020];
        let mut sym = SymTable::new();
        sym.insert("__torajs_fn_1".into(), 0xDEAD);
        sym.insert("__torajs_fn_01".into(), 0x1111);
        sym.insert("__torajs_boxed_2".into(), 0x2222);
        let targets = Targets::new(&funcs, &fn_vaddrs, &sym);
        let at = |name: &str| targets.lookup(name);
        assert_eq!(at("__torajs_fn_0"), 0x10_0000);
        assert_eq!(at("__torajs_fn_1"), 0x10_0020);
        assert_eq!(at("__torajs_boxed_1"), 0x10_0020);
        assert_eq!(at("__torajs_fn_01"), 0x1111);
        assert_eq!(at("__torajs_boxed_2"), 0x2222);
    }

    #[test]
    #[should_panic(expected = "unresolved symbol")]
    fn apply_relocs_panics_on_unresolved_extern() {
        let mut caller = compile_function(&build_caller_calls_foo(FuncId(0)));
        caller.relocs.clear();
        caller.relocs.push(Reloc {
            byte_offset: 8,
            kind: RelocKind::CallSite {
                target: CallTarget::Extern("_does_not_exist".into()),
            },
        });

        let _ = apply_relocs(std::slice::from_ref(&caller), &[0x100000], &SymTable::new());
    }

    #[test]
    #[should_panic(expected = "fn_vaddrs must have one entry per CompiledFunction")]
    fn apply_relocs_panics_on_vaddr_count_mismatch() {
        let foo = compile_function(&build_foo_returns_7());
        let _ = apply_relocs(std::slice::from_ref(&foo), &[], &SymTable::new());
    }
}
