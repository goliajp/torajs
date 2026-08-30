//! Constant-branch folding over interval evidence — folds a CondBr
//! whose ICmp cond is provably constant at its program point, then
//! sweeps the dead compare chain and the now-unreachable blocks.
//!
//! Runs right after `ctpop_idiom`: the Kernighan-loop collapse turns
//! the float_demote growth guards (`count > 2^53`, `total > 2^53-1`)
//! into provably-false checks — `ctpop` is bounded by 64 and the
//! accumulator by `trip * 64` — but the flow-insensitive interval
//! lattice cannot see either (cells widen through their own joins),
//! so this pass brings two demand-driven evaluators:
//!
//! * [`point_eval`] — LVI-lite unique-last-def resolution at a
//!   concrete program point (the count-cell chain);
//! * [`accum`] — SCEV-lite add-recurrence bound over a counted loop
//!   (the total-cell guard).
//!
//! The flow-insensitive lattice is the third source, and the blind
//! spot is mutual: `point_eval` gives up on a loop counter, because
//! the preheader def and the latch def both reach the read and there
//! is no unique last one, while the lattice answers exactly that
//! shape — a cell's fact is the join over its defs, so it holds at
//! every point the value is live. `i < 0` on a counter that starts at
//! zero is the case that needs it, and it is what the guarded index
//! read leaves behind: the length guard settles the upper bound of
//! `xs[i]` and never the lower, so that compare is emitted and is
//! always false. It is recomputed here rather than reused from the
//! pipeline's earlier run — the passes in between rewrite
//! instructions, and a fact keyed by a ValueId that has since been
//! reused is evidence for the wrong value.
//!
//! Folding is generic: any always-true/false ICmp-fed CondBr folds,
//! guard or not. `TORAJS_BRANCH_FOLD_OFF=1` skips (bisect gate),
//! `TORAJS_BRANCH_FOLD_STATS=1` dumps counters.

pub(crate) mod accum;
pub(crate) mod point_eval;

use std::cell::OnceCell;
use std::collections::HashMap;

use torajs_core::ssa::{
    BlockId, Function, IPred, InstKind, Module, Operand, Terminator, ValueId, visit_value_operands,
};

use crate::dominator::DominatorTree;
use crate::interval::transfer::NumFact;
use crate::loop_analysis::LoopAnalysis;
use point_eval::{PointEval, successors};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct BranchFoldStats {
    /// CondBrs replaced with an unconditional Br.
    pub folded: u32,
    /// Dead pure instructions removed after folding.
    pub insts_removed: u32,
    /// Unreachable blocks removed after folding.
    pub blocks_removed: u32,
}

/// Fold provably-constant conditional branches in every function.
pub fn fold_branches(module: &mut Module) -> BranchFoldStats {
    let mut stats = BranchFoldStats::default();
    for func in module.funcs.iter_mut() {
        if func.is_declaration() {
            continue;
        }
        fold_in_function(func, &mut stats);
    }
    stats
}

fn fold_in_function(func: &mut Function, stats: &mut BranchFoldStats) {
    let dom = DominatorTree::compute(func);
    let la = LoopAnalysis::compute(func, &dom);
    let pe = PointEval::new(func, &dom);
    // Lazily: most functions carry no ICmp-fed CondBr at all, and the
    // lattice is a fixpoint per function.
    let flow: OnceCell<HashMap<ValueId, NumFact>> = OnceCell::new();

    // Decide every foldable CondBr against the unmutated CFG (edge
    // removal only shrinks the path set, so the evidence stays valid
    // when the folds apply together).
    let mut folds: Vec<(usize, BlockId)> = Vec::new();
    for (bi, block) in func.blocks.iter().enumerate() {
        let Terminator::CondBr {
            cond: Operand::Value(cv),
            then_blk,
            else_blk,
        } = &block.term
        else {
            continue;
        };
        let Some(cd) = single_def(func, *cv) else {
            continue;
        };
        let InstKind::ICmp(pred, a, b) = &func.blocks[cd.0].insts[cd.1].kind else {
            continue;
        };
        let fa = eval_side(func, &la, &pe, &flow, a, cd);
        let fb = eval_side(func, &la, &pe, &flow, b, cd);
        if let (Some(fa), Some(fb)) = (fa, fb)
            && let Some(taken) = decide(*pred, fa, fb)
        {
            folds.push((bi, if taken { *then_blk } else { *else_blk }));
        }
    }
    if folds.is_empty() {
        return;
    }
    for (bi, target) in folds {
        func.blocks[bi].term = Terminator::Br(target);
        stats.folded += 1;
    }
    stats.insts_removed += sweep_dead_insts(func);
    stats.blocks_removed += sweep_unreachable_blocks(func);
}

/// Evaluate one compare side against all three sources: point
/// evaluation, the add-recurrence bound for a single-def `add` over
/// an accumulator cell, and the flow-insensitive lattice. Each is
/// sound on its own, so the answer is their meet.
fn eval_side(
    func: &Function,
    la: &LoopAnalysis,
    pe: &PointEval,
    flow: &OnceCell<HashMap<ValueId, NumFact>>,
    op: &Operand,
    at: (usize, usize),
) -> Option<NumFact> {
    // Point evaluation resolves opaque i64 defs to the vacuous
    // full-i64 fact, so the other two must always get their shot;
    // all are sound → meet (disjoint cannot arise among sound facts,
    // but fall back to the other side).
    let pf = pe.eval_operand(op, at);
    let af = match op {
        Operand::Value(v) => {
            single_def(func, *v).and_then(|vd| accum::accum_bound(func, la, pe, *v, vd))
        }
        _ => None,
    };
    let ff = match op {
        Operand::Value(v) => flow
            .get_or_init(|| crate::interval::analyze_function(func))
            .get(v)
            .copied(),
        _ => None,
    };
    [pf, af, ff]
        .into_iter()
        .flatten()
        .reduce(|a, b| a.meet(b).unwrap_or(b))
}

fn decide(pred: IPred, a: NumFact, b: NumFact) -> Option<bool> {
    match pred {
        IPred::Sgt => decide_ordered(a.lo > b.hi, a.hi <= b.lo),
        IPred::Slt => decide_ordered(a.hi < b.lo, a.lo >= b.hi),
        IPred::Sge => decide_ordered(a.lo >= b.hi, a.hi < b.lo),
        IPred::Sle => decide_ordered(a.hi <= b.lo, a.lo > b.hi),
        IPred::Eq => decide_ordered(a.lo == a.hi && b.lo == b.hi && a.lo == b.lo, disjoint(a, b)),
        IPred::Ne => decide_ordered(disjoint(a, b), a.lo == a.hi && b.lo == b.hi && a.lo == b.lo),
    }
}

fn decide_ordered(always: bool, never: bool) -> Option<bool> {
    match (always, never) {
        (true, _) => Some(true),
        (_, true) => Some(false),
        _ => None,
    }
}

fn disjoint(a: NumFact, b: NumFact) -> bool {
    a.hi < b.lo || a.lo > b.hi
}

pub(crate) fn single_def(func: &Function, v: ValueId) -> Option<(usize, usize)> {
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

/// Remove pure instructions whose results are no longer read
/// anywhere (the folded compares and their operand chains). Fixpoint
/// loop — removing a compare frees its operands.
fn sweep_dead_insts(func: &mut Function) -> u32 {
    let mut removed = 0u32;
    loop {
        let mut uses: HashMap<ValueId, u32> = HashMap::new();
        let mut bump = |v: ValueId| *uses.entry(v).or_insert(0) += 1;
        for block in &func.blocks {
            for inst in &block.insts {
                visit_value_operands(&inst.kind, &mut bump);
            }
            match &block.term {
                Terminator::Ret(Some(Operand::Value(v))) => bump(*v),
                Terminator::CondBr {
                    cond: Operand::Value(v),
                    ..
                } => bump(*v),
                _ => {}
            }
        }
        let mut changed = false;
        for block in func.blocks.iter_mut() {
            block.insts.retain(|inst| {
                let dead = matches!(inst.result, Some(r) if !uses.contains_key(&r))
                    && is_removable(&inst.kind);
                if dead {
                    removed += 1;
                    changed = true;
                }
                !dead
            });
        }
        if !changed {
            return removed;
        }
    }
}

/// Side-effect-free kinds safe to drop when unused. Calls, memory
/// ops, allocas and reference mints stay.
fn is_removable(kind: &InstKind) -> bool {
    matches!(
        kind,
        InstKind::BinOp(..)
            | InstKind::ICmp(..)
            | InstKind::FCmp(..)
            | InstKind::Copy(..)
            | InstKind::Identity(_)
            | InstKind::Neg(_)
            | InstKind::Ctpop(_)
            | InstKind::Select(..)
            | InstKind::ZExtBoolToI64(_)
            | InstKind::ZExtI32ToI64(_)
            | InstKind::TruncI64ToBool(_)
            | InstKind::SiToFp(_)
            | InstKind::FpToSi(_)
            | InstKind::BitCastF64ToI64(_)
            | InstKind::BitCastI64ToF64(_)
    )
}

/// Drop blocks no longer reachable from the entry block and renumber
/// the survivors (the `BlockId.0 == position` invariant the whole
/// pipeline relies on).
///
/// Sound as a plain terminator remap because the IR has no phi nodes:
/// a removed block's id appears only in `Terminator`s, all of which
/// are rewritten here. `block_layout` calls it once more at the end
/// of the pipeline — folding is not the only way a block loses its
/// last predecessor (507-04).
pub(crate) fn sweep_unreachable_blocks(func: &mut Function) -> u32 {
    let n = func.blocks.len();
    let mut reachable = vec![false; n];
    let mut stack = vec![0usize];
    while let Some(b) = stack.pop() {
        if reachable[b] {
            continue;
        }
        reachable[b] = true;
        for t in successors(&func.blocks[b].term) {
            stack.push(t.0 as usize);
        }
    }
    if reachable.iter().all(|r| *r) {
        return 0;
    }
    let mut remap: HashMap<u32, u32> = HashMap::new();
    let mut next = 0u32;
    for (i, r) in reachable.iter().enumerate() {
        if *r {
            remap.insert(func.blocks[i].id.0, next);
            next += 1;
        }
    }
    let mut removed = 0u32;
    let mut i = 0usize;
    func.blocks.retain(|_| {
        let keep = reachable[i];
        i += 1;
        if !keep {
            removed += 1;
        }
        keep
    });
    for block in func.blocks.iter_mut() {
        block.id = BlockId(remap[&block.id.0]);
        match &mut block.term {
            Terminator::Br(t) => t.0 = remap[&t.0],
            Terminator::CondBr {
                then_blk, else_blk, ..
            } => {
                then_blk.0 = remap[&then_blk.0];
                else_blk.0 = remap[&else_blk.0];
            }
            _ => {}
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{BinOp, Block, Inst, Type, ValueInfo};

    fn inst(result: u32, kind: InstKind) -> Inst {
        Inst {
            result: Some(ValueId(result)),
            kind,
            origin: None,
        }
    }

    fn v(id: u32) -> Operand {
        Operand::Value(ValueId(id))
    }

    fn c(n: i64) -> Operand {
        Operand::ConstI64(n)
    }

    fn block(id: u32, insts: Vec<Inst>, term: Terminator) -> Block {
        Block {
            id: BlockId(id),
            insts,
            term,
        }
    }

    /// The post-ctpop popcount shape: outer counted loop, per-
    /// iteration count guard (`count > 2^53`) and accumulator guard
    /// (`total > 2^53-1`), both feeding f64 side exits. Both guards
    /// must fold and the side exits must sweep away.
    ///
    ///   b0: total=0; i=0                 b4: g1 = count > 2^53 ? b8 : b5
    ///   b1: i < 1e7 ? b2 : b7            b5: x = total+count; g2 ? b9 : b6
    ///   b2: n=i; count=0                 b6: i++; total=x → b1
    ///   b3: pc=ctpop n; count=0+pc       b7: ret   b8/b9: side exits
    #[test]
    fn popcount_guards_fold_and_side_exits_sweep() {
        // v0 total cell, v1 i cell, v2 n cell, v3 count cell,
        // v4 header cmp, v5 pc, v6 sum, v7 g1, v8 t1, v9 x, v10 g2,
        // v11 i2
        let mut f = Function {
            name: "f".into(),
            params: vec![],
            ret: Type::Void,
            values: vec![
                ValueInfo {
                    ty: Type::I64,
                    name: None
                };
                12
            ],
            blocks: vec![
                block(
                    0,
                    vec![
                        inst(0, InstKind::Copy(Type::I64, c(0))),
                        inst(1, InstKind::Copy(Type::I64, c(0))),
                    ],
                    Terminator::Br(BlockId(1)),
                ),
                block(
                    1,
                    vec![inst(4, InstKind::ICmp(IPred::Slt, v(1), c(10_000_000)))],
                    Terminator::CondBr {
                        cond: v(4),
                        then_blk: BlockId(2),
                        else_blk: BlockId(7),
                    },
                ),
                block(
                    2,
                    vec![
                        inst(2, InstKind::Copy(Type::I64, v(1))),
                        inst(3, InstKind::Copy(Type::I64, c(0))),
                    ],
                    Terminator::Br(BlockId(3)),
                ),
                block(
                    3,
                    vec![
                        inst(5, InstKind::Ctpop(v(2))),
                        inst(6, InstKind::BinOp(BinOp::Add, v(3), v(5))),
                        inst(3, InstKind::Copy(Type::I64, v(6))),
                        inst(2, InstKind::Copy(Type::I64, c(0))),
                    ],
                    Terminator::Br(BlockId(4)),
                ),
                block(
                    4,
                    vec![inst(
                        7,
                        InstKind::ICmp(IPred::Sgt, v(3), c(9007199254740992)),
                    )],
                    Terminator::CondBr {
                        cond: v(7),
                        then_blk: BlockId(8),
                        else_blk: BlockId(5),
                    },
                ),
                block(
                    5,
                    vec![
                        inst(8, InstKind::Copy(Type::I64, v(3))),
                        inst(9, InstKind::BinOp(BinOp::Add, v(0), v(8))),
                        inst(10, InstKind::ICmp(IPred::Sgt, v(9), c(9007199254740991))),
                    ],
                    Terminator::CondBr {
                        cond: v(10),
                        then_blk: BlockId(9),
                        else_blk: BlockId(6),
                    },
                ),
                block(
                    6,
                    vec![
                        inst(11, InstKind::BinOp(BinOp::Add, v(1), c(1))),
                        inst(0, InstKind::Copy(Type::I64, v(9))),
                        inst(1, InstKind::Copy(Type::I64, v(11))),
                    ],
                    Terminator::Br(BlockId(1)),
                ),
                block(7, vec![], Terminator::Ret(None)),
                block(8, vec![], Terminator::Br(BlockId(7))),
                block(9, vec![], Terminator::Br(BlockId(7))),
            ],
            current_origin: None,
        };
        let mut stats = BranchFoldStats::default();
        fold_in_function(&mut f, &mut stats);
        assert_eq!(stats.folded, 2, "both guards fold: {stats:?}");
        assert_eq!(stats.blocks_removed, 2, "side exits sweep: {stats:?}");
        assert_eq!(f.blocks.len(), 8);
        // renumbering keeps the position invariant
        for (i, b) in f.blocks.iter().enumerate() {
            assert_eq!(b.id.0 as usize, i);
        }
        // no CondBr guards remain besides the loop header
        let cond_count = f
            .blocks
            .iter()
            .filter(|b| matches!(b.term, Terminator::CondBr { .. }))
            .count();
        assert_eq!(cond_count, 1);
        // the guard compares are gone
        for b in &f.blocks {
            for i2 in &b.insts {
                assert!(
                    !matches!(&i2.kind, InstKind::ICmp(IPred::Sgt, ..)),
                    "guard icmp must be swept"
                );
            }
        }
    }

    /// A branch whose cond depends on an opaque call must not fold.
    #[test]
    fn opaque_cond_does_not_fold() {
        let mut f = Function {
            name: "f".into(),
            params: vec![],
            ret: Type::Void,
            values: vec![
                ValueInfo {
                    ty: Type::I64,
                    name: None
                };
                2
            ],
            blocks: vec![
                block(
                    0,
                    vec![
                        inst(0, InstKind::Call(torajs_core::ssa::FuncId(0), vec![])),
                        inst(1, InstKind::ICmp(IPred::Sgt, v(0), c(5))),
                    ],
                    Terminator::CondBr {
                        cond: v(1),
                        then_blk: BlockId(1),
                        else_blk: BlockId(2),
                    },
                ),
                block(1, vec![], Terminator::Ret(None)),
                block(2, vec![], Terminator::Ret(None)),
            ],
            current_origin: None,
        };
        let mut stats = BranchFoldStats::default();
        fold_in_function(&mut f, &mut stats);
        assert_eq!(stats, BranchFoldStats::default());
    }
}
