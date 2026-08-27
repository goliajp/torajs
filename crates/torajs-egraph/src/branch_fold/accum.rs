//! Loop-accumulator bound — the SCEV-lite half of branch_fold (LLVM
//! ScalarEvolution add-recurrence analogue). A guarded value `%x =
//! add %acc, %inc` whose cell `%acc` is a plain add-recurrence over a
//! counted loop is bounded by `entry + trip * inc` even though the
//! interval lattice widens the cell to a sentinel: the float_demote
//! accumulator guard (`total > 2^53-1` on a popcount sum) is exactly
//! this shape, with `trip = 1e7` from the loop-counter compare and
//! `inc ∈ [0, 64]` from the ctpop fact.
//!
//! Soundness conditions (all checked structurally):
//! * the recurrence def (`%acc = copy %x`) is the only def of `%acc`
//!   inside the loop, its block sits at nest depth 1 (no inner loop
//!   re-executes it within one header visit — a body cycle avoiding
//!   the header would be its own natural loop and deepen the nest),
//!   and the loop header itself is not nested (so the loop runs one
//!   episode per function entry and the entry value cannot compound
//!   across episodes);
//! * the loop has a single back edge and its header exits on
//!   `counter <slt/sle> K` with the in-loop side on `then`;
//! * the counter's only in-loop def is `copy (add counter, s)` with
//!   constant step `s >= 1`;
//! * entry values for both cells come from `PointEval` at every
//!   header predecessor outside the loop (join over entries).
//!
//! The increment is evaluated at the add's point; when that read is
//! the loop counter itself (`total += i` — through the block-tail
//! `copy` the post-φ form threads it through), point evaluation has
//! no unique last def inside the body (the latch def and the entry
//! def both reach) and answers nothing. The counter's own trip
//! argument bounds it instead: at any body point it lies in
//! `[entry, K (−1 for slt) + s]` — the header check bounds it at
//! iteration start and one step is all a body pass adds (r506; the
//! `sum += i` guard never folded before, so its slow version and
//! f64 exit bridge stayed).

use torajs_core::ssa::{BlockId, Function, IPred, InstKind, Operand, Terminator, ValueId};

use super::point_eval::PointEval;
use crate::interval::transfer::{NumFact, op_const_int};
use crate::loop_analysis::{LoopAnalysis, NaturalLoop};

/// Bound `x` (a single-def `add cell, inc` result) via the
/// add-recurrence argument. Returns a fact valid at every execution
/// of `x`'s def.
pub(crate) fn accum_bound(
    func: &Function,
    la: &LoopAnalysis,
    pe: &PointEval,
    x: ValueId,
    x_def: (usize, usize),
) -> Option<NumFact> {
    let inst = &func.blocks[x_def.0].insts[x_def.1];
    let InstKind::BinOp(torajs_core::ssa::BinOp::Add, a, b) = &inst.kind else {
        return None;
    };
    // One side is the accumulator cell, the other the increment.
    let (acc, inc_op) = match (a, b) {
        (Operand::Value(va), _) if is_rec_cell(func, *va, x) => (*va, b),
        (_, Operand::Value(vb)) if is_rec_cell(func, *vb, x) => (*vb, a),
        _ => return None,
    };
    let x_block = func.blocks[x_def.0].id;
    let lp = innermost_loop(la, x_block)?;
    if lp.back_edge_sources.len() != 1
        || la.nest_depth(lp.header) != 1
        || la.nest_depth(x_block) != 1
    {
        return None;
    }
    // The recurrence def must be the only in-loop def of the cell,
    // and it must live in this loop at nest depth 1.
    let mut rec_def: Option<(usize, usize)> = None;
    for (bi, block) in func.blocks.iter().enumerate() {
        if !lp.body.contains(&block.id) {
            continue;
        }
        for (ii, i2) in block.insts.iter().enumerate() {
            if i2.result == Some(acc) {
                if rec_def.is_some() {
                    return None;
                }
                if !matches!(&i2.kind, InstKind::Copy(_, Operand::Value(src)) if *src == x) {
                    return None;
                }
                rec_def = Some((bi, ii));
            }
        }
    }
    let rec_def = rec_def?;
    if la.nest_depth(func.blocks[rec_def.0].id) != 1 {
        return None;
    }
    let counter = loop_counter(func, lp, pe)?;
    let trip = counter.trip();
    let entry = entry_fact(func, lp, pe, acc)?;
    let inc = match pe.eval_operand(inc_op, x_def) {
        Some(f) => f,
        None if resolves_to(func, inc_op, counter.cell) => counter.body_range(),
        None => return None,
    };
    // After k <= trip additions the checked (post-add) value is
    // entry + k * inc; bound each side by the worst-signed growth.
    let hi = entry.hi.checked_add(trip.checked_mul(inc.hi.max(0))?)?;
    let lo = entry.lo.checked_add(trip.checked_mul(inc.lo.min(0))?)?;
    Some(NumFact::new(lo, hi))
}

/// Bound a loop cell read AFTER its loop (the `console.log(sum)` /
/// `return sum` accumulator shape — the exit bridge's `sitofp` reads
/// it there): the join of the cell's entry value and the recurrence
/// bound of its in-loop def. Sound when every def of the cell is
/// either the in-loop recurrence def or sits in a header predecessor
/// outside the loop, and the read point is dominated by the header
/// (so every path to it entered the loop through the same entries).
pub(crate) fn cell_bound_after_loop(
    func: &Function,
    la: &LoopAnalysis,
    pe: &PointEval,
    dom: &crate::dominator::DominatorTree,
    cell: ValueId,
    point: (usize, usize),
) -> Option<NumFact> {
    // the one in-loop def `copy x`, and every other def on an entry edge
    let mut rec: Option<(ValueId, BlockId)> = None;
    let mut lp: Option<&NaturalLoop> = None;
    for block in &func.blocks {
        for inst in &block.insts {
            if inst.result != Some(cell) {
                continue;
            }
            match innermost_loop(la, block.id) {
                Some(l) => {
                    if rec.is_some() {
                        return None;
                    }
                    let InstKind::Copy(_, Operand::Value(x)) = &inst.kind else {
                        return None;
                    };
                    rec = Some((*x, block.id));
                    lp = Some(l);
                }
                None => {}
            }
        }
    }
    let (x, _) = rec?;
    let lp = lp?;
    let point_block = func.blocks[point.0].id;
    if lp.body.contains(&point_block) || !dom.dominates(lp.header, point_block) {
        return None;
    }
    for block in &func.blocks {
        if lp.body.contains(&block.id) {
            continue;
        }
        let defines = block.insts.iter().any(|i| i.result == Some(cell));
        if defines && !super::point_eval::successors(&block.term).contains(&lp.header) {
            return None;
        }
    }
    let x_def = single_def(func, x)?;
    let grown = accum_bound(func, la, pe, x, x_def)?;
    let entry = entry_fact(func, lp, pe, cell)?;
    Some(entry.join(grown))
}

/// Is `cell` a multi-def value with a `copy x` def somewhere (the
/// cheap pre-filter before the full structural walk)?
fn is_rec_cell(func: &Function, cell: ValueId, x: ValueId) -> bool {
    let mut saw_rec = false;
    let mut defs = 0u32;
    for block in &func.blocks {
        for inst in &block.insts {
            if inst.result == Some(cell) {
                defs += 1;
                if matches!(&inst.kind, InstKind::Copy(_, Operand::Value(s)) if *s == x) {
                    saw_rec = true;
                }
            }
        }
    }
    defs >= 2 && saw_rec
}

fn innermost_loop(la: &LoopAnalysis, b: BlockId) -> Option<&NaturalLoop> {
    la.loops()
        .iter()
        .filter(|l| l.body.contains(&b))
        .min_by_key(|l| l.body.len())
}

/// The loop's counting argument: cell, constant step, the header's
/// exclusive-or-inclusive bound and the entry value.
struct LoopCounter {
    cell: ValueId,
    step: i128,
    /// `K − 1` for `slt K`, `K` for `sle K`: the largest value the
    /// header lets into the body.
    last: i128,
    entry: NumFact,
}

impl LoopCounter {
    /// Max header-true outcomes per loop episode.
    fn trip(&self) -> i128 {
        let span = self.last - self.entry.lo + 1;
        (span.max(0) + self.step - 1) / self.step
    }

    /// The counter's range at any point of the body: at most `last`
    /// on entry to the body, plus the one step the latch adds.
    fn body_range(&self) -> NumFact {
        NumFact::new(self.entry.lo, self.last + self.step)
    }
}

/// Does `op` name `cell`, directly or through single-def `copy`s
/// (the post-φ form reads a loop cell through a block-tail copy)?
fn resolves_to(func: &Function, op: &Operand, cell: ValueId) -> bool {
    let mut cur = match op {
        Operand::Value(v) => *v,
        _ => return false,
    };
    for _ in 0..8 {
        if cur == cell {
            return true;
        }
        let Some((bi, ii)) = single_def(func, cur) else {
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
/// exit compare and the counter's `+s` step.
fn loop_counter(func: &Function, lp: &NaturalLoop, pe: &PointEval) -> Option<LoopCounter> {
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
    let cond_def = single_def(func, *cv)?;
    let InstKind::ICmp(pred @ (IPred::Slt | IPred::Sle), Operand::Value(counter), k_op) =
        &func.blocks[cond_def.0].insts[cond_def.1].kind
    else {
        return None;
    };
    let k = op_const_int(k_op)?;
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
            let sdef = single_def(func, *sv)?;
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
fn entry_fact(func: &Function, lp: &NaturalLoop, pe: &PointEval, cell: ValueId) -> Option<NumFact> {
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

fn single_def(func: &Function, v: ValueId) -> Option<(usize, usize)> {
    let mut found: Option<(usize, usize)> = None;
    for (bi, block) in func.blocks.iter().enumerate() {
        for (ii, inst) in block.insts.iter().enumerate() {
            if inst.result == Some(v) {
                if found.is_some() {
                    return None;
                }
                found = Some((bi, ii));
            }
        }
    }
    found
}
