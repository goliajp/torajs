//! Low-half demand: which int32 values never have their upper half
//! read, and the instructions that then shrink.
//!
//! `FpToInt32` writes ToInt32 into Wd (upper half zeroed) and then
//! sign-extends, so the i64 holds the int32. The SXTW is dead when
//! every use reads only the low 32 bits:
//!
//!   - an `SiToFp` of a value every def of which is a sign-extended
//!     int32 (it converts from the W view instead);
//!   - a `shl` by a constant of 32 or more;
//!   - an add / sub / mul / and / or / xor, or a `shl` by a smaller
//!     constant, whose own result is demanded only in its low half
//!     (the low 32 bits of these depend on the low 32 of the operands).
//!
//! Any other use (store, call argument, compare, index, return, copy,
//! select, terminator) demands all 64 bits. The demand is a least
//! fixpoint seeded from those uses, so a cycle through multi-def
//! values settles on the sound side.
//!
//! Independently of demand, `shl x, 32` followed at once by `ashr _, 32`
//! (its only reader) is one SXTW; followed at once by the `sitofp` that
//! is the pair's only reader, the three are one SCVTF Dd, Wx.

use torajs_core::ssa::id_hash::HashMap;
use torajs_core::ssa::{
    BinOp, Function, Inst, InstKind, Operand, Terminator, Type, ValueId, visit_value_operands,
};

use super::operand::materialize_operand_gpr;
use super::{OP_SCRATCH_LHS, OP_SCRATCH_RESULT_GPR, write_def_spill_gpr, write_u32};
use crate::enc::sxtw_x_w;
use crate::regalloc::Assignment;

/// What the instruction at one position emits instead.
#[derive(Clone, Copy)]
enum Shrunk {
    /// a pair half a later instruction of the group emits
    Absorbed,
    /// `ashr _, 32` emitting `sxtw dst, w(x)`, with x
    Sxtw(Operand),
    /// `SiToFp` converting from the W view of the operand
    WConvert(Operand),
    /// `FpToInt32` of the operand whose sign extension no use reads
    Bare(Operand),
}

/// The shrunk instructions of one function, by (block, index); empty
/// when the function has nothing to shrink, so `emit` answers without
/// a lookup.
#[derive(Default)]
pub(super) struct Low32 {
    at: Option<HashMap<(u32, u32), Shrunk>>,
}

impl Low32 {
    pub(super) fn plan(func: &Function, uses: &HashMap<ValueId, u32>) -> Self {
        // every shrink needs an FpToInt32 or a `shl _, 32` to exist
        let seeded = func.blocks.iter().flat_map(|b| &b.insts).any(|i| {
            matches!(
                i.kind,
                InstKind::FpToInt32(_) | InstKind::BinOp(BinOp::Shl, _, Operand::ConstI64(32))
            )
        });
        if !seeded {
            return Low32::default();
        }
        let defs = Defs::of(func);
        let full = full_demand(func, &defs);
        let mut at = HashMap::default();
        for block in &func.blocks {
            let b = block.id.0;
            let insts = &block.insts;
            for (ii, inst) in insts.iter().enumerate() {
                let pos = (b, ii as u32);
                match (&inst.kind, inst.result) {
                    (InstKind::FpToInt32(src), Some(r))
                        if func.value_type(r) == Type::I64 && !full[r.0 as usize] =>
                    {
                        at.insert(pos, Shrunk::Bare(*src));
                    }
                    (InstKind::SiToFp(Operand::Value(x)), _) if defs.sext32(*x) => {
                        // a pair right before may claim it below
                        at.entry(pos)
                            .or_insert(Shrunk::WConvert(Operand::Value(*x)));
                    }
                    _ => {}
                }
                let Some(x) = insts
                    .get(ii..ii + 2)
                    .and_then(|pair| sext_pair(pair, &defs, uses))
                else {
                    continue;
                };
                let ashr = (b, ii as u32 + 1);
                let r = insts[ii + 1].result.expect("ashr has a result");
                let convert = insts.get(ii + 2).is_some_and(
                    |c| matches!(c.kind, InstKind::SiToFp(Operand::Value(v)) if v == r),
                );
                at.insert(pos, Shrunk::Absorbed);
                if convert && uses.get(&r) == Some(&1) && defs.single(r).is_some() {
                    at.insert(ashr, Shrunk::Absorbed);
                    at.insert((b, ii as u32 + 2), Shrunk::WConvert(x));
                } else {
                    at.insert(ashr, Shrunk::Sxtw(x));
                }
            }
        }
        Low32 { at: Some(at) }
    }

    /// True when the instruction at `at` was emitted here (shrunk, or
    /// absorbed into the group a later instruction emits).
    pub(super) fn emit(
        &self,
        bytes: &mut Vec<u8>,
        at: (u32, u32),
        inst: &Inst,
        alloc: &Assignment,
    ) -> bool {
        let Some(shrunk) = self.at.as_ref().and_then(|m| m.get(&at)) else {
            return false;
        };
        match *shrunk {
            Shrunk::Absorbed => {}
            Shrunk::Sxtw(x) => emit_sxtw(bytes, inst, &x, alloc),
            Shrunk::WConvert(x) => super::cast::emit_si_to_fp(bytes, inst, &x, alloc, true),
            Shrunk::Bare(src) => super::cast::emit_fp_to_int32(bytes, inst, &src, alloc, false),
        }
        true
    }
}

/// `sxtw dst, w(x)` for the pair ending in `inst`.
fn emit_sxtw(bytes: &mut Vec<u8>, inst: &Inst, x: &Operand, alloc: &Assignment) {
    let r = inst.result.expect("ashr has a result");
    let (dst, spill_off) = alloc.def_gpr(r, OP_SCRATCH_RESULT_GPR);
    let rx = materialize_operand_gpr(bytes, x, OP_SCRATCH_LHS, alloc);
    write_u32(bytes, sxtw_x_w(dst, rx));
    write_def_spill_gpr(bytes, spill_off, dst);
}

/// The x of `shl t, x, 32; ashr r, t, 32` at the head of `insts`, when
/// `t` has that one def and that one use.
fn sext_pair(insts: &[Inst], defs: &Defs, uses: &HashMap<ValueId, u32>) -> Option<Operand> {
    let InstKind::BinOp(BinOp::AShr, Operand::Value(t), Operand::ConstI64(32)) = insts[1].kind
    else {
        return None;
    };
    let x = shl32_src(&insts[0].kind)?;
    (insts[0].result == Some(t) && uses.get(&t) == Some(&1) && defs.single(t).is_some())
        .then_some(x)
}

fn shl32_src(kind: &InstKind) -> Option<Operand> {
    match kind {
        InstKind::BinOp(BinOp::Shl, x, Operand::ConstI64(32)) => Some(*x),
        _ => None,
    }
}

/// Every def of every value, dense by value id; a parameter counts
/// as a def no instruction makes.
struct Defs<'a> {
    count: Vec<u32>,
    /// the def when it is the only one and an instruction made it
    single: Vec<Option<&'a InstKind>>,
    /// every def leaves a sign-extended int32 in the value
    sext32: Vec<bool>,
}

impl<'a> Defs<'a> {
    fn of(func: &'a Function) -> Self {
        let n = func.values.len();
        let mut count = vec![0u32; n];
        let mut single = vec![None; n];
        for &p in &func.params {
            count[p.0 as usize] += 1;
        }
        let insts = || func.blocks.iter().flat_map(|b| &b.insts);
        for inst in insts() {
            if let Some(r) = inst.result {
                count[r.0 as usize] += 1;
                single[r.0 as usize] = Some(&inst.kind);
            }
        }
        for (c, s) in count.iter().zip(&mut single) {
            if *c != 1 {
                *s = None;
            }
        }
        let mut defs = Defs {
            sext32: count.iter().map(|&c| c > 0).collect(),
            count,
            single,
        };
        for &p in &func.params {
            defs.sext32[p.0 as usize] = false;
        }
        for inst in insts() {
            if let Some(r) = inst.result
                && !defs.sext_def(&inst.kind)
            {
                defs.sext32[r.0 as usize] = false;
            }
        }
        defs
    }

    fn single(&self, v: ValueId) -> Option<&'a InstKind> {
        self.single[v.0 as usize]
    }

    /// Every def of `v` leaves a sign-extended int32 in it.
    fn sext32(&self, v: ValueId) -> bool {
        self.sext32[v.0 as usize]
    }

    fn sext_def(&self, kind: &InstKind) -> bool {
        match kind {
            InstKind::FpToInt32(_) => true,
            InstKind::BinOp(BinOp::AShr, Operand::Value(t), Operand::ConstI64(32)) => {
                self.count[t.0 as usize] == 1 && self.single(*t).and_then(shl32_src).is_some()
            }
            _ => false,
        }
    }
}

/// The values some use reads all 64 bits of, by value id.
fn full_demand(func: &Function, defs: &Defs) -> Vec<bool> {
    // the operands demanded as far as a result is, as linked lists:
    // `head[r]` indexes `links`, each link (operand, next)
    const NONE: u32 = u32::MAX;
    let mut head = vec![NONE; func.values.len()];
    let mut links: Vec<(ValueId, u32)> = Vec::new();
    let mut follow = |r: ValueId, x: ValueId| {
        links.push((x, head[r.0 as usize]));
        head[r.0 as usize] = links.len() as u32 - 1;
    };
    let mut work: Vec<ValueId> = Vec::new();
    for block in &func.blocks {
        for inst in &block.insts {
            match (&inst.kind, inst.result) {
                (InstKind::BinOp(op, a, b), Some(r))
                    if func.value_type(r) == Type::I64
                        && matches!(
                            op,
                            BinOp::Add
                                | BinOp::Sub
                                | BinOp::Mul
                                | BinOp::And
                                | BinOp::Or
                                | BinOp::Xor
                        ) =>
                {
                    for x in [a, b].into_iter().filter_map(value) {
                        follow(r, x);
                    }
                }
                (InstKind::BinOp(BinOp::Shl, Operand::Value(x), Operand::ConstI64(c)), Some(r))
                    if func.value_type(r) == Type::I64 && (0..64).contains(c) =>
                {
                    if *c < 32 {
                        follow(r, *x);
                    }
                }
                (InstKind::SiToFp(Operand::Value(x)), _) if defs.sext32(*x) => {}
                (kind, _) => visit_value_operands(kind, |v| work.push(v)),
            }
        }
        match &block.term {
            Terminator::Ret(Some(Operand::Value(v)))
            | Terminator::CondBr {
                cond: Operand::Value(v),
                ..
            } => work.push(*v),
            _ => {}
        }
    }
    let mut full = vec![false; func.values.len()];
    while let Some(v) = work.pop() {
        if std::mem::replace(&mut full[v.0 as usize], true) {
            continue;
        }
        let mut at = head[v.0 as usize];
        while at != NONE {
            let (x, next) = links[at as usize];
            work.push(x);
            at = next;
        }
    }
    full
}

fn value(op: &Operand) -> Option<ValueId> {
    match op {
        Operand::Value(v) => Some(*v),
        _ => None,
    }
}

#[cfg(test)]
#[path = "low32_tests.rs"]
mod tests;
