//! Scaled-addressing fold — `LoadDyn(ty, base, Shl(idx, 3))` /
//! `StoreDyn(val, base, Shl(idx, 3))` become the `LoadDynScaled8` /
//! `StoreDynScaled8` forms and the standalone shift dies (S7 knife
//! c2). aarch64's register-offset addressing does the ×8 inside the
//! AGU (`LDR Xd, [Xn, Xm, LSL #3]`), so the shift that array-index
//! lowering (`emit_arr_slot_byte_offset`) puts on the address
//! dependency chain every loop iteration disappears entirely.
//!
//! Conditions, both load-bearing:
//!
//! * The scale is exactly ×8 — spelled `Shl(x, 3)` or `Mul(x, 8)`
//!   (index lowering emits both) — because the scaled register form
//!   requires shift == log2(access width) and every DynScaled8
//!   access is 8 bytes. The Any-array 16-byte stride (`Shl(i, 4)`)
//!   is NOT foldable on aarch64 and keeps the plain form.
//! * The shift result's ONLY use is this one address operand
//!   (counted across every inst operand and terminator in the
//!   function). With another use the shift must be materialized
//!   anyway, and folding would save nothing while hiding the reuse.
//!
//! Runs before regalloc (it is an SSA→SSA rewrite), so liveness for
//! the shift's own operand is recomputed downstream and stays exact —
//! the codegen-level peephole alternative was rejected precisely
//! because the operand's live range ends at the shift under the
//! already-fixed assignment (decomposition note
//! `.claude/tasks/2026-08-24/perf-s7-abstraction-gap-decomposition.md`).

use std::collections::HashMap;

use torajs_core::ssa::{
    BinOp, Function, InstKind, Module, Operand, Terminator, ValueId, visit_value_operands,
};

#[derive(Debug, Default)]
pub struct ScaledAddrStats {
    pub folded_loads: usize,
    pub folded_stores: usize,
}

pub fn fold_scaled_addresses(module: &mut Module) -> ScaledAddrStats {
    let mut stats = ScaledAddrStats::default();
    for func in &mut module.funcs {
        fold_function(func, &mut stats);
    }
    stats
}

fn fold_function(func: &mut Function, stats: &mut ScaledAddrStats) {
    // Pass 1 — use counts over every operand position, terminators
    // included (missing one would delete a shift something still
    // reads), and the `Shl(x, 3)` def sites.
    let mut uses: HashMap<ValueId, usize> = HashMap::new();
    let mut shl3_defs: HashMap<ValueId, (usize, usize, Operand)> = HashMap::new();
    for (bi, block) in func.blocks.iter().enumerate() {
        for (ii, inst) in block.insts.iter().enumerate() {
            visit_value_operands(&inst.kind, |v| *uses.entry(v).or_insert(0) += 1);
            // `i << 3` and `i * 8` are the same ×8 under two's-
            // complement wrap; index lowering emits both spellings
            // (Shl on the read path, Mul on the mutator path).
            let scaled_idx = match &inst.kind {
                InstKind::BinOp(BinOp::Shl, x, Operand::ConstI64(3)) => Some(x),
                InstKind::BinOp(BinOp::Mul, x, Operand::ConstI64(8))
                | InstKind::BinOp(BinOp::Mul, Operand::ConstI64(8), x) => Some(x),
                _ => None,
            };
            if let Some(x) = scaled_idx
                && let Some(result) = inst.result
            {
                shl3_defs.insert(result, (bi, ii, x.clone()));
            }
        }
        match &block.term {
            Terminator::Ret(Some(Operand::Value(v))) => *uses.entry(*v).or_insert(0) += 1,
            Terminator::CondBr {
                cond: Operand::Value(v),
                ..
            } => *uses.entry(*v).or_insert(0) += 1,
            _ => {}
        }
    }

    // Pass 2 — rewrite single-use shift addresses; remember the dead
    // shift's position.
    let mut dead: Vec<(usize, usize)> = Vec::new();
    for block in &mut func.blocks {
        for inst in &mut block.insts {
            let addr_value = match &inst.kind {
                InstKind::LoadDyn(_, _, Operand::Value(v))
                | InstKind::StoreDyn(_, _, Operand::Value(v)) => *v,
                _ => continue,
            };
            let Some((bi, ii, x)) = shl3_defs.get(&addr_value) else {
                continue;
            };
            if uses.get(&addr_value) != Some(&1) {
                continue;
            }
            match &inst.kind {
                InstKind::LoadDyn(ty, base, _) => {
                    inst.kind = InstKind::LoadDynScaled8(*ty, base.clone(), x.clone());
                    stats.folded_loads += 1;
                }
                InstKind::StoreDyn(val, base, _) => {
                    inst.kind = InstKind::StoreDynScaled8(val.clone(), base.clone(), x.clone());
                    stats.folded_stores += 1;
                }
                _ => unreachable!(),
            }
            dead.push((*bi, *ii));
        }
    }

    // Pass 3 — drop the dead shifts, back to front so indices stay
    // valid within each block.
    dead.sort_unstable_by(|a, b| b.cmp(a));
    for (bi, ii) in dead {
        func.blocks[bi].insts.remove(ii);
    }
}
