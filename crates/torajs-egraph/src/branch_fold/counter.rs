//! The counting argument of a loop: how many times its header lets
//! the body run, and what a cell holds on entry. `accum` bounds a loop
//! sum by `entry + trip * inc` with these.

use torajs_core::ssa::loop_analysis::NaturalLoop;
use torajs_core::ssa::{Function, IPred, InstKind, Operand, Terminator, ValueId};

use super::point_eval::PointEval;
use crate::interval::transfer::{NumFact, op_const_int};

/// The loop's counting argument: cell, constant step, the header's
/// exclusive-or-inclusive bound and the entry value.
pub(super) struct LoopCounter {
    pub(super) cell: ValueId,
    step: i128,
    /// `K − 1` for `slt K`, `K` for `sle K`: the largest value the
    /// header lets into the body.
    last: i128,
    entry: NumFact,
}

impl LoopCounter {
    /// Max header-true outcomes per loop episode.
    pub(super) fn trip(&self) -> i128 {
        let span = self.last - self.entry.lo + 1;
        (span.max(0) + self.step - 1) / self.step
    }

    /// The counter's range at any point of the body: at most `last`
    /// on entry to the body, plus the one step the latch adds.
    pub(super) fn body_range(&self) -> NumFact {
        NumFact::new(self.entry.lo, self.last + self.step)
    }
}

/// Does `op` name `cell`, directly or through single-def `copy`s
/// (the post-φ form reads a loop cell through a block-tail copy)?
pub(super) fn resolves_to(func: &Function, pe: &PointEval, op: &Operand, cell: ValueId) -> bool {
    let mut cur = match op {
        Operand::Value(v) => *v,
        _ => return false,
    };
    for _ in 0..8 {
        if cur == cell {
            return true;
        }
        let Some((bi, ii)) = pe.single_def(cur) else {
            return false;
        };
        let InstKind::Copy(_, Operand::Value(src)) = &func.blocks[bi].insts[ii].kind else {
            return false;
        };
        cur = *src;
    }
    false
}

/// The counting argument from the header's `counter <slt/sle> K`
/// exit compare and the counter's `+s` step. `K` is bounded by its
/// fact at the compare; the counter never enters the body above the
/// fact's high end.
pub(super) fn loop_counter(
    func: &Function,
    lp: &NaturalLoop,
    pe: &PointEval,
) -> Option<LoopCounter> {
    let hb = lp.header.0 as usize;
    let header = &func.blocks[hb];
    let Terminator::CondBr {
        cond: Operand::Value(cv),
        then_blk,
        else_blk,
    } = &header.term
    else {
        return None;
    };
    if !lp.body.contains(then_blk) || lp.body.contains(else_blk) {
        return None;
    }
    let cond_def = pe.single_def(*cv)?;
    let InstKind::ICmp(pred @ (IPred::Slt | IPred::Sle), Operand::Value(counter), k_op) =
        &func.blocks[cond_def.0].insts[cond_def.1].kind
    else {
        return None;
    };
    let k = match op_const_int(k_op) {
        Some(k) => k,
        None => pe.eval_operand_with_flow(k_op, cond_def)?.hi,
    };
    // Counter: only in-loop def is `copy (add counter, s)`, s >= 1.
    let mut step: Option<i128> = None;
    for block in &func.blocks {
        if !lp.body.contains(&block.id) {
            continue;
        }
        for inst in &block.insts {
            if inst.result != Some(*counter) {
                continue;
            }
            if step.is_some() {
                return None;
            }
            let InstKind::Copy(_, Operand::Value(sv)) = &inst.kind else {
                return None;
            };
            let sdef = pe.single_def(*sv)?;
            let InstKind::BinOp(torajs_core::ssa::BinOp::Add, sa, sb) =
                &func.blocks[sdef.0].insts[sdef.1].kind
            else {
                return None;
            };
            let s = match (sa, sb) {
                (Operand::Value(c2), other) if c2 == counter => op_const_int(other)?,
                (other, Operand::Value(c2)) if c2 == counter => op_const_int(other)?,
                _ => return None,
            };
            if s < 1 {
                return None;
            }
            step = Some(s);
        }
    }
    let step = step?;
    let entry = entry_fact(func, lp, pe, *counter)?;
    if entry.lo <= crate::interval::transfer::NEG_INF {
        return None;
    }
    let last = k - if matches!(pred, IPred::Sle) { 0 } else { 1 };
    Some(LoopCounter {
        cell: *counter,
        step,
        last,
        entry,
    })
}

/// Join of the cell's value over every header predecessor outside the
/// loop (the entry edges).
pub(super) fn entry_fact(
    func: &Function,
    lp: &NaturalLoop,
    pe: &PointEval,
    cell: ValueId,
) -> Option<NumFact> {
    let mut entry: Option<NumFact> = None;
    let mut saw_pred = false;
    for (bi, block) in func.blocks.iter().enumerate() {
        if lp.body.contains(&block.id) {
            continue;
        }
        if !super::point_eval::successors(&block.term).contains(&lp.header) {
            continue;
        }
        saw_pred = true;
        let f = pe.eval_value(cell, (bi, block.insts.len()))?;
        entry = Some(match entry {
            Some(e) => e.join(f),
            None => f,
        });
    }
    if saw_pred { entry } else { None }
}
