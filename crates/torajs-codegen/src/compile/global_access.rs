//! A load or store straight through a symbol's address folds the low
//! half of that address into the access itself.
//!
//! `GlobalRef(sym)` lowers to `ADRP Xd, page(sym); ADD Xd, Xd,
//! #lo12(sym)`, and a `Load` / `Store` at offset 0 through it then
//! adds an `LDR` / `STR [Xd]`. When the address has no other reader,
//! the `ADD` is redundant: `LDR Xt, [Xd, #lo12(sym)]` reads the same
//! word, with the PAGEOFF12 reloc on the access (the linker scales it
//! by the access size). The throw check after every call that may
//! raise is exactly this shape, as is every module `let` read and
//! write.
//!
//! The access must sit in the address's own block: an address the
//! optimizer hoisted out of a loop is materialized once, and folding
//! it into the access inside the loop would put an `ADRP` back on
//! every iteration.
//!
//! An address every reader of which is such an access in its block
//! (a module `let` read, changed and written back) keeps only its
//! `ADRP`: each access reads the page from the address's register and
//! carries its own PAGEOFF12.
//!
//! The scaled immediate only reaches a symbol aligned to the access
//! size, so the fold is limited to the 8-byte accesses of symbols
//! known to be 8-aligned: the module's own data globals of 8-byte
//! slots, and the runtime's 64-bit atomic flags.

use torajs_core::ssa::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{
    Function, Inst, InstKind, Module, Operand, THROW_ACTIVE_SYM, Type, ValueId, WEAKREF_ACTIVE_SYM,
};

use super::operand::{materialize_operand_fpr, materialize_operand_gpr, operand_is_f64};
use super::{
    FP_SCRATCH_LHS, FP_SCRATCH_RESULT, OP_SCRATCH_LHS, OP_SCRATCH_RESULT_GPR, OP_SCRATCH_RHS,
    write_def_spill_fpr, write_def_spill_gpr, write_u32,
};
use crate::enc::{adrp, ldr_d_imm12, ldr_x_imm12, str_d_imm12, str_x_imm12};
use crate::reg::Gpr;
use crate::regalloc::Assignment;
use crate::reloc::{Reloc, RelocKind};

/// The planned folds of one function: the `GlobalRef`s that emit
/// nothing, the ones that emit only their page, and the accesses that
/// fold the page offset, each keyed by (block id, inst index).
#[derive(Default)]
pub(super) struct GlobalAccesses {
    absorbed: HashSet<(u32, u32)>,
    paged: HashMap<(u32, u32), String>,
    accesses: HashMap<(u32, u32), Access>,
}

/// A folded access: its symbol, and the address whose register holds
/// the page already, if one does.
pub(super) struct Access {
    sym: String,
    page: Option<ValueId>,
}

impl GlobalAccesses {
    pub(super) fn absorbed(&self, b: u32, i: u32) -> bool {
        self.absorbed.contains(&(b, i))
    }

    pub(super) fn paged_sym(&self, b: u32, i: u32) -> Option<&str> {
        self.paged.get(&(b, i)).map(String::as_str)
    }

    pub(super) fn access(&self, b: u32, i: u32) -> Option<&Access> {
        self.accesses.get(&(b, i))
    }
}

pub(super) fn aligned8_syms(module: &Module) -> HashSet<String> {
    // the link lays a data global out at its slot's natural alignment,
    // 4 for an I32 and 8 for everything else
    let mut out: HashSet<String> = module
        .data_globals
        .iter()
        .filter(|g| g.ty != Type::I32)
        .map(|g| g.name.clone())
        .collect();
    out.insert(THROW_ACTIVE_SYM.to_string());
    out.insert(WEAKREF_ACTIVE_SYM.to_string());
    out
}

/// An 8-byte access whose address is `v`, if `kind` is one at offset
/// 0: the X / D forms of `emit_load` and `emit_store`. An I32 or
/// vector access, or a store of the address itself, never folds.
fn access_through(func: &Function, kind: &InstKind) -> Option<ValueId> {
    match kind {
        InstKind::Load(ty, Operand::Value(p), 0) if !matches!(ty, Type::I32 | Type::Vec(_)) => {
            Some(*p)
        }
        InstKind::Store(val, Operand::Value(p), 0) => {
            let narrow = match val {
                Operand::ConstI32(_) => true,
                Operand::Value(v) => {
                    *v == *p || matches!(func.value_type(*v), Type::I32 | Type::Vec(_))
                }
                _ => false,
            };
            (!narrow).then_some(*p)
        }
        _ => None,
    }
}

pub(super) fn plan(
    func: &Function,
    uses: &HashMap<ValueId, u32>,
    aligned8: &HashSet<String>,
) -> GlobalAccesses {
    // a value defined more than once may hold something other than the
    // address at an access, so only a single definition folds
    let mut defs: HashMap<ValueId, u32> = HashMap::default();
    let mut refs: HashMap<ValueId, ((u32, u32), &str)> = HashMap::default();
    for block in &func.blocks {
        for (ii, inst) in block.insts.iter().enumerate() {
            if let Some(r) = inst.result {
                *defs.entry(r).or_default() += 1;
            }
            if let (InstKind::GlobalRef(sym), Some(r)) = (&inst.kind, inst.result)
                && aligned8.contains(sym)
                && uses.contains_key(&r)
            {
                refs.insert(r, ((block.id.0, ii as u32), sym.as_str()));
            }
        }
    }
    refs.retain(|r, _| defs[r] == 1);
    let mut out = GlobalAccesses::default();
    if refs.is_empty() {
        return out;
    }
    // the accesses through each address after it in its own block; an
    // address with a reader of another kind, or elsewhere, keeps its ADD
    let mut through: HashMap<ValueId, Vec<(u32, u32)>> = HashMap::default();
    for block in &func.blocks {
        for (ii, inst) in block.insts.iter().enumerate() {
            if let Some(p) = access_through(func, &inst.kind)
                && let Some(&(at, _)) = refs.get(&p)
                && at.0 == block.id.0
                && ii as u32 > at.1
            {
                through.entry(p).or_default().push((block.id.0, ii as u32));
            }
        }
    }
    for (p, sites) in through {
        let (at, sym) = refs[&p];
        if uses.get(&p) != Some(&(sites.len() as u32)) {
            continue;
        }
        let page = if sites.len() == 1 {
            out.absorbed.insert(at);
            None
        } else {
            out.paged.insert(at, sym.to_string());
            Some(p)
        };
        for site in sites {
            let sym = sym.to_string();
            out.accesses.insert(site, Access { sym, page });
        }
    }
    out
}

/// A paged address: `ADRP` alone into the address's register.
pub(super) fn emit_paged(
    bytes: &mut Vec<u8>,
    relocs: &mut Vec<Reloc>,
    inst: &Inst,
    sym: &str,
    alloc: &Assignment,
) {
    let r = inst.result.expect("GlobalRef must have result");
    let (dst, spill_off) = alloc.def_gpr(r, OP_SCRATCH_RESULT_GPR);
    emit_page(bytes, relocs, dst, sym);
    write_def_spill_gpr(bytes, spill_off, dst);
}

/// The register holding the page an access reads through: the paged
/// address's, or `fresh` after an `ADRP` into it.
fn page_base(
    bytes: &mut Vec<u8>,
    relocs: &mut Vec<Reloc>,
    acc: &Access,
    fresh: Gpr,
    alloc: &Assignment,
) -> Gpr {
    match acc.page {
        Some(p) => materialize_operand_gpr(bytes, &Operand::Value(p), fresh, alloc),
        None => {
            emit_page(bytes, relocs, fresh, &acc.sym);
            fresh
        }
    }
}

/// `ADRP base, page(sym)` with its `Page21` reloc; the access that
/// follows carries the `PageOff12`.
fn emit_page(bytes: &mut Vec<u8>, relocs: &mut Vec<Reloc>, base: Gpr, sym: &str) {
    relocs.push(Reloc {
        byte_offset: bytes.len() as u32,
        kind: RelocKind::Page21 {
            target_sym: sym.to_string(),
        },
    });
    write_u32(bytes, adrp(base, 0));
}

fn emit_access(bytes: &mut Vec<u8>, relocs: &mut Vec<Reloc>, word: u32, sym: &str) {
    relocs.push(Reloc {
        byte_offset: bytes.len() as u32,
        kind: RelocKind::PageOff12 {
            target_sym: sym.to_string(),
        },
    });
    write_u32(bytes, word);
}

/// Emit a planned access: the address page into a base register (or
/// the paged address's own), then the load or store with the page
/// offset as its immediate. Register choices mirror `emit_load` /
/// `emit_store`.
pub(super) fn emit(
    bytes: &mut Vec<u8>,
    relocs: &mut Vec<Reloc>,
    inst: &Inst,
    acc: &Access,
    alloc: &Assignment,
) {
    let sym = acc.sym.as_str();
    match &inst.kind {
        InstKind::Load(ty, _, _) => {
            let vid = inst.result.expect("Load must have result");
            if *ty == Type::F64 {
                let (dst, spill_off) = alloc.def_fpr(vid, FP_SCRATCH_RESULT);
                let base = page_base(bytes, relocs, acc, OP_SCRATCH_LHS, alloc);
                emit_access(bytes, relocs, ldr_d_imm12(dst, base, 0), sym);
                write_def_spill_fpr(bytes, spill_off, dst);
            } else {
                let (dst, spill_off) = alloc.def_gpr(vid, OP_SCRATCH_RESULT_GPR);
                let base = page_base(bytes, relocs, acc, dst, alloc);
                emit_access(bytes, relocs, ldr_x_imm12(dst, base, 0), sym);
                write_def_spill_gpr(bytes, spill_off, dst);
            }
        }
        InstKind::Store(val, _, _) => {
            let word = if super::store_pair::zero_word(val) {
                let base = page_base(bytes, relocs, acc, OP_SCRATCH_RHS, alloc);
                str_x_imm12(Gpr::XZR, base, 0)
            } else if operand_is_f64(val, alloc) {
                let rs = materialize_operand_fpr(bytes, val, FP_SCRATCH_LHS, OP_SCRATCH_LHS, alloc);
                let base = page_base(bytes, relocs, acc, OP_SCRATCH_RHS, alloc);
                str_d_imm12(rs, base, 0)
            } else {
                let rs = materialize_operand_gpr(bytes, val, OP_SCRATCH_LHS, alloc);
                let base = page_base(bytes, relocs, acc, OP_SCRATCH_RHS, alloc);
                str_x_imm12(rs, base, 0)
            };
            emit_access(bytes, relocs, word, sym);
        }
        _ => unreachable!("global_access plans only loads and stores"),
    }
}

#[cfg(test)]
mod tests {
    use super::super::{ModuleFacts, compile_function_in};
    use crate::reloc::RelocKind;
    use torajs_core::ssa::{
        BinOp, Function, Inst, InstKind, Operand, THROW_ACTIVE_SYM, Terminator, Type, ValueId,
    };

    const LDR_X_IMM: (u32, u32) = (0xFFC0_0000, 0xF940_0000);
    const STR_X_IMM: (u32, u32) = (0xFFC0_0000, 0xF900_0000);
    const ADD_X_IMM: (u32, u32) = (0xFF80_0000, 0x9100_0000);
    const LDR_D_IMM: (u32, u32) = (0xFFC0_0000, 0xFD40_0000);
    const STR_D_IMM: (u32, u32) = (0xFFC0_0000, 0xFD00_0000);

    fn facts() -> ModuleFacts {
        let mut f = ModuleFacts::default();
        f.aligned8_syms.insert(THROW_ACTIVE_SYM.to_string());
        f.aligned8_syms.insert("g64".to_string());
        f
    }

    /// The word each PageOff12 reloc of `f` sits on.
    fn compile(f: &Function) -> Vec<u32> {
        let c = compile_function_in(f, &facts());
        let words: Vec<u32> = c
            .bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|w| u32::from_le_bytes(*w))
            .collect();
        c.relocs
            .iter()
            .filter(|r| matches!(r.kind, RelocKind::PageOff12 { .. }))
            .map(|r| words[r.byte_offset as usize / 4])
            .collect()
    }

    fn is(w: u32, (mask, val): (u32, u32)) -> bool {
        w & mask == val
    }

    /// `ret load(ty, global_ref(sym))`, the address read `reads` times.
    fn load_of(sym: &str, ty: Type, reads: usize) -> Function {
        let mut f = Function::new("t", ty);
        let b = f.add_block();
        let g = f.append_inst(b, InstKind::GlobalRef(sym.into()), Type::Ptr, None);
        let mut last = Operand::ConstI64(0);
        for _ in 0..reads {
            let v = f.append_inst(b, InstKind::Load(ty, Operand::Value(g), 0), ty, None);
            last = Operand::Value(v);
        }
        f.set_term(b, Terminator::Ret(Some(last)));
        f
    }

    #[test]
    fn a_single_read_of_an_aligned_symbol_folds_the_page_offset() {
        let lo12 = compile(&load_of(THROW_ACTIVE_SYM, Type::I64, 1));
        assert_eq!(lo12.len(), 1);
        assert!(is(lo12[0], LDR_X_IMM), "{:#x}", lo12[0]);
    }

    #[test]
    fn an_unknown_symbol_keeps_the_add() {
        let lo12 = compile(&load_of("g", Type::I64, 1));
        assert_eq!(lo12.len(), 1);
        assert!(is(lo12[0], ADD_X_IMM), "{:#x}", lo12[0]);
    }

    #[test]
    fn accesses_sharing_an_address_share_its_page() {
        let c = compile_function_in(&load_of("g64", Type::I64, 2), &facts());
        let lo12 = compile(&load_of("g64", Type::I64, 2));
        assert_eq!(lo12.len(), 2);
        assert!(lo12.iter().all(|w| is(*w, LDR_X_IMM)), "{lo12:x?}");
        let pages = (c.relocs.iter())
            .filter(|r| matches!(r.kind, RelocKind::Page21 { .. }))
            .count();
        assert_eq!(pages, 1);
    }

    #[test]
    fn a_read_and_a_write_back_share_one_page() {
        let mut f = Function::new("t", Type::I64);
        let b = f.add_block();
        let g = f.append_inst(b, InstKind::GlobalRef("g64".into()), Type::Ptr, None);
        let v = f.append_inst(
            b,
            InstKind::Load(Type::I64, Operand::Value(g), 0),
            Type::I64,
            None,
        );
        let w = f.append_inst(
            b,
            InstKind::BinOp(BinOp::Add, Operand::Value(v), Operand::ConstI64(1)),
            Type::I64,
            None,
        );
        f.append_void(b, InstKind::Store(Operand::Value(w), Operand::Value(g), 0));
        f.set_term(b, Terminator::Ret(Some(Operand::Value(w))));
        let lo12 = compile(&f);
        assert_eq!(lo12.len(), 2);
        assert!(
            is(lo12[0], LDR_X_IMM) && is(lo12[1], STR_X_IMM),
            "{lo12:x?}"
        );
        // the store reads the page from the register the load read it from
        assert_eq!((lo12[0] >> 5) & 0x1F, (lo12[1] >> 5) & 0x1F);
    }

    #[test]
    fn an_address_read_another_way_keeps_the_add() {
        let mut f = load_of("g64", Type::I64, 1);
        let b = f.blocks[0].id;
        let g = f.blocks[0].insts[0].result.expect("ref");
        f.set_term(b, Terminator::Ret(Some(Operand::Value(g))));
        let lo12 = compile(&f);
        assert_eq!(lo12.len(), 1);
        assert!(is(lo12[0], ADD_X_IMM), "{:#x}", lo12[0]);
    }

    #[test]
    fn a_four_byte_read_keeps_the_add() {
        let lo12 = compile(&load_of("g64", Type::I32, 1));
        assert_eq!(lo12.len(), 1);
        assert!(is(lo12[0], ADD_X_IMM), "{:#x}", lo12[0]);
    }

    #[test]
    fn a_store_folds_the_page_offset() {
        let mut f = Function::new("t", Type::I64);
        let p = f.add_param(Type::I64, "p");
        let b = f.add_block();
        let g = f.append_inst(b, InstKind::GlobalRef("g64".into()), Type::Ptr, None);
        f.append_void(b, InstKind::Store(Operand::Value(p), Operand::Value(g), 0));
        f.set_term(b, Terminator::Ret(Some(Operand::ConstI64(0))));
        let lo12 = compile(&f);
        assert_eq!(lo12.len(), 1);
        assert!(is(lo12[0], STR_X_IMM), "{:#x}", lo12[0]);
    }

    /// `store val -> global_ref("g64")`, then `ret 0`.
    fn store_of(val: Operand, param: Option<Type>) -> Function {
        let mut f = Function::new("t", Type::I64);
        let val = match param {
            Some(ty) => Operand::Value(f.add_param(ty, "p")),
            None => val,
        };
        let b = f.add_block();
        let g = f.append_inst(b, InstKind::GlobalRef("g64".into()), Type::Ptr, None);
        f.append_void(b, InstKind::Store(val, Operand::Value(g), 0));
        f.set_term(b, Terminator::Ret(Some(Operand::ConstI64(0))));
        f
    }

    #[test]
    fn an_f64_read_folds_into_ldr_d() {
        let lo12 = compile(&load_of("g64", Type::F64, 1));
        assert_eq!(lo12.len(), 1);
        assert!(is(lo12[0], LDR_D_IMM), "{:#x}", lo12[0]);
    }

    #[test]
    fn an_f64_store_folds_into_str_d() {
        let lo12 = compile(&store_of(Operand::ConstI64(0), Some(Type::F64)));
        assert_eq!(lo12.len(), 1);
        assert!(is(lo12[0], STR_D_IMM), "{:#x}", lo12[0]);
    }

    #[test]
    fn a_zero_store_folds_into_str_xzr() {
        for zero in [Operand::ConstI64(0), Operand::ConstF64(0.0)] {
            let lo12 = compile(&store_of(zero, None));
            assert_eq!(lo12.len(), 1);
            assert!(is(lo12[0], STR_X_IMM), "{:#x}", lo12[0]);
            assert_eq!(lo12[0] & 0x1F, 31, "Rt must be xzr: {:#x}", lo12[0]);
        }
    }

    #[test]
    fn a_four_byte_store_keeps_the_add() {
        for lo12 in [
            compile(&store_of(Operand::ConstI32(7), None)),
            compile(&store_of(Operand::ConstI32(0), None)),
            compile(&store_of(Operand::ConstI64(0), Some(Type::I32))),
        ] {
            assert_eq!(lo12.len(), 1);
            assert!(is(lo12[0], ADD_X_IMM), "{:#x}", lo12[0]);
        }
    }

    #[test]
    fn an_access_in_another_block_keeps_the_add() {
        let mut f = Function::new("t", Type::I64);
        let entry = f.add_block();
        let body = f.add_block();
        let g = f.append_inst(entry, InstKind::GlobalRef("g64".into()), Type::Ptr, None);
        f.set_term(entry, Terminator::Br(body));
        let v = f.append_inst(
            body,
            InstKind::Load(Type::I64, Operand::Value(g), 0),
            Type::I64,
            None,
        );
        f.set_term(body, Terminator::Ret(Some(Operand::Value(v))));
        let lo12 = compile(&f);
        assert_eq!(lo12.len(), 1);
        assert!(is(lo12[0], ADD_X_IMM), "{:#x}", lo12[0]);
    }

    /// What `plan` folds in `f`: (absorbed, paged, accesses).
    fn planned(f: &Function) -> (usize, usize, usize) {
        let uses = super::super::brfuse::count_uses(f);
        let p = super::plan(f, &uses, &facts().aligned8_syms);
        (p.absorbed.len(), p.paged.len(), p.accesses.len())
    }

    fn def(r: ValueId, kind: InstKind) -> Inst {
        Inst {
            result: Some(r),
            kind,
            origin: None,
        }
    }

    /// `n` values of type `ty` in a one-block function with no instructions.
    fn with_values(ty: Type, n: usize) -> (Function, Vec<ValueId>) {
        let mut f = Function::new("t", ty);
        let b = f.add_block();
        let vals = (0..n)
            .map(|_| f.append_inst(b, InstKind::Identity(Operand::ConstI64(0)), ty, None))
            .collect();
        f.blocks[0].insts.clear();
        (f, vals)
    }

    #[test]
    fn an_access_ahead_of_the_address_folds_nothing() {
        // the first load reads whatever `g` held before the address, so
        // neither load may take the symbol's page
        let (mut f, v) = with_values(Type::I64, 4);
        let (g, a, b, s) = (v[0], v[1], v[2], v[3]);
        f.blocks[0].insts = vec![
            def(a, InstKind::Load(Type::I64, Operand::Value(g), 0)),
            def(g, InstKind::GlobalRef("g64".into())),
            def(b, InstKind::Load(Type::I64, Operand::Value(g), 0)),
            def(
                s,
                InstKind::BinOp(BinOp::Add, Operand::Value(a), Operand::Value(b)),
            ),
        ];
        f.blocks[0].term = Terminator::Ret(Some(Operand::Value(s)));
        assert_eq!(planned(&f), (0, 0, 0));
    }

    #[test]
    fn an_address_defined_twice_folds_nothing() {
        // the load reads the second definition, not the symbol
        let (mut f, v) = with_values(Type::I64, 2);
        let (g, a) = (v[0], v[1]);
        f.blocks[0].insts = vec![
            def(g, InstKind::GlobalRef("g64".into())),
            def(g, InstKind::Identity(Operand::ConstI64(0))),
            def(a, InstKind::Load(Type::I64, Operand::Value(g), 0)),
        ];
        f.blocks[0].term = Terminator::Ret(Some(Operand::Value(a)));
        assert_eq!(planned(&f), (0, 0, 0));
        assert_eq!(planned(&load_of("g64", Type::I64, 1)), (1, 0, 1));
        assert_eq!(planned(&load_of("g64", Type::I64, 2)), (0, 1, 2));
    }
}
