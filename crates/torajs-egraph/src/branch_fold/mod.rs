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
//! always false. The facts are the pipeline's own run of the analysis
//! for a body no pass has changed since; a changed body is analyzed
//! again, because the passes in between can tighten it (the guards
//! float_demote inserts, the loops ctpop collapses) and the values they
//! minted have no fact of their own (`interval::FactCache`).
//!
//! The fourth is the branch-edge constraints the lattice itself uses
//! for instruction results but never applies to a value's own fact: a
//! single-def value's range on every path into the compare's block
//! (`while (xs.length > 0)` settles the pop's `len != 0` once late GVN
//! has made the two length loads one value).
//!
//! Folding is generic: any always-true/false ICmp-fed CondBr folds,
//! guard or not. `TORAJS_BRANCH_FOLD_OFF=1` skips (bisect gate),
//! `TORAJS_BRANCH_FOLD_STATS=1` dumps counters.

pub(crate) mod accum;
mod counter;
mod decide;
pub(crate) mod point_eval;
pub mod sum_facts;

use crate::id_hash::{HashMap, HashSet};
use std::cell::OnceCell;

use torajs_core::ssa::{
    BinOp, BlockId, Function, InstKind, Module, Operand, Terminator, ValueId, visit_value_operands,
};

use crate::interval::refine::{Constraint, Constraints};
use crate::interval::transfer::{NumFact, POS_INF, op_const_int};
use crate::interval::{FactCache, Seed};
use decide::decide;
use point_eval::{PointEval, successors};
use torajs_core::ssa::dominator::DominatorTree;
use torajs_core::ssa::loop_analysis::LoopAnalysis;

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
/// `facts` is the pipeline's interval analysis, index-aligned with
/// `module.funcs`, and `cache` tells which bodies it still describes.
pub fn fold_branches(
    module: &mut Module,
    facts: Option<&[HashMap<ValueId, NumFact>]>,
    cache: Option<&FactCache>,
) -> BranchFoldStats {
    let (cache, names) = FactCache::for_module(cache, module);
    let mut stats = BranchFoldStats::default();
    for (fi, func) in module.funcs.iter_mut().enumerate() {
        if func.is_declaration() {
            continue;
        }
        let seed = cache.and_then(|c| c.get(facts, fi, func));
        fold_in_function(func, &names, seed, &mut stats);
    }
    stats
}

fn fold_in_function(func: &mut Function, fns: &[String], seed: Seed, stats: &mut BranchFoldStats) {
    let dom = DominatorTree::compute(func);
    let la = LoopAnalysis::compute(func, &dom);
    let pe = PointEval::new(func, &dom, &la, fns, seed);
    // Lazily: most functions carry no ICmp-fed CondBr at all.
    let edges: OnceCell<Constraints> = OnceCell::new();
    let src = Sources {
        func,
        la: &la,
        pe: &pe,
        edges: &edges,
        dom: &dom,
    };

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
        let Some(cd) = pe.single_def(*cv) else {
            continue;
        };
        let InstKind::ICmp(pred, a, b) = &func.blocks[cd.0].insts[cd.1].kind else {
            continue;
        };
        // both sides read the same value at the same instant, so the
        // compare is reflexive whatever the value is; sext_elide's
        // round-trip collapse leaves `icmp ne x, x` behind
        if let (Operand::Value(va), Operand::Value(vb)) = (a, b)
            && va == vb
        {
            let taken = decide::reflexive(*pred);
            folds.push((bi, if taken { *then_blk } else { *else_blk }));
            continue;
        }
        let fa = eval_side(&src, a, cd);
        let fb = eval_side(&src, b, cd);
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

/// What [`eval_side`] consults; the edge constraints are built on
/// first use, the lattice lives in the point evaluator.
struct Sources<'a> {
    func: &'a Function,
    la: &'a LoopAnalysis,
    pe: &'a PointEval<'a>,
    edges: &'a OnceCell<Constraints>,
    dom: &'a DominatorTree,
}

/// Evaluate one compare side against all four sources: point
/// evaluation, the add-recurrence bound for a single-def `add` over
/// an accumulator cell, the flow-insensitive lattice, and the ranges
/// the branch edges into the compare's block put on a single-def
/// value. Each is sound on its own, so the answer is their meet.
fn eval_side(src: &Sources<'_>, op: &Operand, at: (usize, usize)) -> Option<NumFact> {
    let Sources { func, la, pe, .. } = *src;
    // Point evaluation resolves opaque i64 defs to the vacuous
    // full-i64 fact, so the other two must always get their shot;
    // all are sound → meet (disjoint cannot arise among sound facts,
    // but fall back to the other side).
    let pf = pe.eval_operand(op, at);
    let af = match op {
        Operand::Value(v) => pe
            .single_def(*v)
            .and_then(|vd| accum::accum_bound(func, la, pe, *v, vd)),
        _ => None,
    };
    let ff = match op {
        Operand::Value(v) => pe.flow_fact(*v),
        _ => None,
    };
    let ef = match op {
        Operand::Value(v) => edge_range(src, *v, at.0),
        _ => None,
    };
    let sf = match op {
        Operand::Value(v) => pe.single_def(*v).and_then(|vd| shifted_bound(src, vd)),
        _ => None,
    };
    [pf, af, ff, ef, sf]
        .into_iter()
        .flatten()
        .reduce(|a, b| a.meet(b).unwrap_or(b))
}

/// A single-def `lshr v, k` at `vd` carries every bound the sources
/// put on `v` there, shifted — the float_demote window test reads its
/// operand through such a shift.
fn shifted_bound(src: &Sources<'_>, vd: (usize, usize)) -> Option<NumFact> {
    let InstKind::BinOp(BinOp::LShr, inner, k) = &src.func.blocks[vd.0].insts[vd.1].kind else {
        return None;
    };
    let k = op_const_int(k).filter(|k| (0..64).contains(k))? as u32;
    let f = eval_side(src, inner, vd)?;
    let hi = if f.hi >= POS_INF { POS_INF } else { f.hi >> k };
    (f.lo >= 0).then(|| NumFact::new(f.lo >> k, hi))
}

/// The range the branch edges dominating block `bi` put on `v`, when
/// `v` has a single def (a multi-def cell may be redefined between the
/// edge and the compare).
fn edge_range(src: &Sources<'_>, v: ValueId, bi: usize) -> Option<NumFact> {
    let func = src.func;
    src.pe.single_def(v)?;
    let edges = src.edges.get_or_init(|| {
        let mut defs: HashMap<ValueId, InstKind> = HashMap::default();
        let mut multi: HashSet<ValueId> = HashSet::default();
        for inst in func.blocks.iter().flat_map(|b| &b.insts) {
            if let Some(r) = inst.result
                && defs.insert(r, inst.kind.clone()).is_some()
            {
                multi.insert(r);
            }
        }
        defs.retain(|r, _| !multi.contains(r));
        Constraints::compute(func, &defs)
    });
    // a single-def value is never redefined below its edge
    edges
        .inherited(func.blocks[bi].id, src.dom)
        .into_iter()
        .filter_map(|c| match c {
            Constraint::Range(w, f) if w == v => Some(f),
            _ => None,
        })
        .reduce(|a, b| a.meet(b).unwrap_or(b))
}

/// Remove pure instructions whose results are no longer read
/// anywhere (the folded compares and their operand chains). Fixpoint
/// loop — removing a compare frees its operands.
fn sweep_dead_insts(func: &mut Function) -> u32 {
    let mut removed = 0u32;
    loop {
        let mut uses: HashMap<ValueId, u32> = HashMap::default();
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
            | InstKind::Cttz(_)
            | InstKind::Select(..)
            | InstKind::ZExtBoolToI64(_)
            | InstKind::ZExtI32ToI64(_)
            | InstKind::TruncI64ToBool(_)
            | InstKind::SiToFp(_)
            | InstKind::FpToSi(_)
            | InstKind::FpToInt32(_)
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
    let mut remap = vec![u32::MAX; n]; // new id by old id, which is the position
    let mut next = 0u32;
    for (i, r) in reachable.iter().enumerate() {
        if *r {
            remap[func.blocks[i].id.0 as usize] = next;
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
        block.id = BlockId(remap[block.id.0 as usize]);
        match &mut block.term {
            Terminator::Br(t) => t.0 = remap[t.0 as usize],
            Terminator::CondBr {
                then_blk, else_blk, ..
            } => {
                then_blk.0 = remap[then_blk.0 as usize];
                else_blk.0 = remap[else_blk.0 as usize];
            }
            _ => {}
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{BinOp, Block, IPred, Inst, Type, ValueInfo};

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
    fn popcount_guarded(shift: bool) -> Function {
        // v0 total cell, v1 i cell, v2 n cell, v3 count cell,
        // v4 header cmp, v5 pc, v6 sum, v7 g1, v8 t1, v9 x, v10 g2,
        // v11 i2, v12 / v13 the shifted guard reads
        let guard = |cond: u32, x: u32, shifted: u32, bound: i64| -> Vec<Inst> {
            if shift {
                vec![
                    inst(shifted, InstKind::BinOp(BinOp::LShr, v(x), c(53))),
                    inst(cond, InstKind::ICmp(IPred::Ne, v(shifted), c(0))),
                ]
            } else {
                vec![inst(cond, InstKind::ICmp(IPred::Sgt, v(x), c(bound)))]
            }
        };
        let mut accumulate = vec![
            inst(8, InstKind::Copy(Type::I64, v(3))),
            inst(9, InstKind::BinOp(BinOp::Add, v(0), v(8))),
        ];
        accumulate.extend(guard(10, 9, 13, 9007199254740991));
        Function {
            name: "f".into(),
            params: vec![],
            ret: Type::Void,
            values: vec![
                ValueInfo {
                    ty: Type::I64,
                    name: None
                };
                14
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
                    guard(7, 3, 12, 9007199254740992),
                    Terminator::CondBr {
                        cond: v(7),
                        then_blk: BlockId(8),
                        else_blk: BlockId(5),
                    },
                ),
                block(
                    5,
                    accumulate,
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
        }
    }

    /// The accumulator bound reaches the guard through the shift of
    /// the power-of-two window form, and the shift sweeps with it.
    #[test]
    fn popcount_shift_guards_fold_and_sweep() {
        let mut f = popcount_guarded(true);
        let mut stats = BranchFoldStats::default();
        fold_in_function(&mut f, &[], None, &mut stats);
        assert_eq!(stats.folded, 2, "both guards fold: {stats:?}");
        assert_eq!(stats.blocks_removed, 2, "side exits sweep: {stats:?}");
        let shifts = f
            .blocks
            .iter()
            .flat_map(|b| &b.insts)
            .filter(|i| matches!(i.kind, InstKind::BinOp(BinOp::LShr, ..)))
            .count();
        assert_eq!(shifts, 0, "the guard shifts are dead and swept");
    }

    #[test]
    fn popcount_guards_fold_and_side_exits_sweep() {
        let mut f = popcount_guarded(false);
        let mut stats = BranchFoldStats::default();
        fold_in_function(&mut f, &[], None, &mut stats);
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
        fold_in_function(&mut f, &[], None, &mut stats);
        assert_eq!(stats, BranchFoldStats::default());
    }

    /// `icmp ne x, x` over an opaque value is false: the arm it guards
    /// goes, even though no range is known for `x`.
    #[test]
    fn reflexive_compare_folds() {
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
                        inst(1, InstKind::ICmp(IPred::Ne, v(0), v(0))),
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
        fold_in_function(&mut f, &[], None, &mut stats);
        assert_eq!(stats.folded, 1, "{stats:?}");
        assert_eq!(stats.blocks_removed, 1, "{stats:?}");
        assert!(matches!(f.blocks[0].term, Terminator::Br(BlockId(1))));
    }
}
