//! Versioning feasibility planning (phase 1b-ii) — pick the regions
//! that can host guarded defs. Read-only over the pre-mutation shape.
//!
//! Region shape: a natural loop hosting in-set defs plus the
//! pre-chain — loop-external blocks holding the component's remaining
//! defs (the `sitofp` entry and cell init copies), each dominating
//! the header with all successors inside the region. Renamed clone
//! values (in-set values and single-def intermediates) must not
//! escape the region; the region keeps a single external entry.
//! Guarded values that no region hosts are evicted and the demotion
//! fixpoint re-runs without them — the pass never fires partially.

use std::collections::{HashMap, HashSet};

use torajs_core::ssa::{BlockId, Function, Operand, ValueId};

use crate::dominator::DominatorTree;
use crate::loop_analysis::LoopAnalysis;
use crate::rc_peephole::visit_value_operands;

use super::cfg::{
    collect_value_blocks, innermost_loop, multi_def_values, pred_map, successors, touched_values,
};
use super::guard::{GuardCheck, GuardSite};

/// One versionable region, planned against the pre-mutation shape.
pub(crate) struct RegionPlan {
    /// Region blocks (loop body + pre-chain), sorted.
    pub(super) region: Vec<BlockId>,
    /// The natural-loop body alone (no pre-chain) — the per-iteration
    /// blocks the profitability gate weighs checks against.
    pub(super) body: Vec<BlockId>,
    /// The single region block with external predecessors.
    pub(super) entry: BlockId,
    /// Growth sites inside the region.
    pub(super) growth: Vec<GuardSite>,
    /// Entry checks (`sitofp` operands not provably exact); checked
    /// once on the region entry edge.
    pub(super) entry_guard: Vec<GuardCheck>,
    /// In-set cells read outside the region (the `console.log(sum)` /
    /// `return sum` accumulator shape), sorted. Their external uses
    /// read a fresh f64 cell written on every region exit edge — the
    /// merge bridge — instead of evicting the component.
    pub(super) escapes: Vec<ValueId>,
}

impl RegionPlan {
    /// Pre-mutation positions of the growth sites (for the mutation
    /// walk to track through rewriting).
    pub(crate) fn growth_keys(&self) -> impl Iterator<Item = (BlockId, usize)> + '_ {
        self.growth.iter().map(|s| (s.block, s.inst_idx))
    }
}

/// Why planning failed: evict the named values and re-run the
/// fixpoint, or split the named pre-chain block before the given
/// inst index first (a mixed preheader — see `plan_one`) and re-plan.
pub(crate) enum PlanFail {
    Evict(HashSet<ValueId>),
    Split(BlockId, usize),
}

/// One header's planning outcome.
enum PlanOutcome {
    Plan(RegionPlan),
    Fail,
    Split(BlockId, usize),
}

/// Plan versioning regions. Candidate regions grow from the innermost
/// natural loop of every in-set def; every guarded site must land in
/// some region. `Err` carries the values whose guarded defs cannot be
/// hosted (caller evicts them from the demotion set and re-runs the
/// fixpoint) or a pre-chain split request (caller splits and
/// re-plans).
pub(crate) fn plan_regions(
    func: &Function,
    set: &HashSet<ValueId>,
    growth_sites: Vec<GuardSite>,
    entry_sites: Vec<GuardSite>,
) -> Result<Vec<RegionPlan>, PlanFail> {
    let dom = DominatorTree::compute(func);
    let loops = LoopAnalysis::compute(func, &dom);
    let value_blocks = collect_value_blocks(func);
    let preds = pred_map(func);

    // candidate headers: innermost loop of every in-set def block,
    // innermost-first
    let mut headers: Vec<(usize, BlockId)> = Vec::new();
    for (v, (defs, _)) in &value_blocks {
        if !set.contains(v) {
            continue;
        }
        for d in defs {
            if let Some((len, h)) = innermost_loop(&loops, *d) {
                if !headers.iter().any(|(_, eh)| *eh == h) {
                    headers.push((len, h));
                }
            }
        }
    }
    headers.sort_by_key(|(len, h)| (*len, h.0));

    let mut plans: Vec<RegionPlan> = Vec::new();
    let mut used: HashSet<BlockId> = HashSet::new();
    for (_, header) in headers {
        let body = loops
            .loops()
            .iter()
            .find(|l| l.header == header)
            .unwrap()
            .body
            .clone();
        match plan_one(
            func,
            set,
            &dom,
            &preds,
            &value_blocks,
            body,
            header,
            &growth_sites,
            &entry_sites,
            &used,
        ) {
            PlanOutcome::Plan(plan) => {
                if plan.growth.is_empty() && plan.entry_guard.is_empty() {
                    continue; // fully exact component — no versioning needed
                }
                used.extend(plan.region.iter().copied());
                plans.push(plan);
            }
            PlanOutcome::Fail => {}
            PlanOutcome::Split(b, p) => return Err(PlanFail::Split(b, p)),
        }
    }

    // every guarded site must be hosted; an unhosted one has no slow
    // path to fall into
    let mut evict: HashSet<ValueId> = HashSet::new();
    for s in &growth_sites {
        if !plans.iter().any(|p| p.region.contains(&s.block)) {
            evict.insert(s.result);
        }
    }
    for s in &entry_sites {
        if !plans.iter().any(|p| p.region.contains(&s.block)) {
            evict.insert(s.result);
        }
    }
    if evict.is_empty() {
        Ok(plans)
    } else {
        Err(PlanFail::Evict(evict))
    }
}

#[expect(clippy::too_many_arguments, reason = "internal planning step")]
fn plan_one(
    func: &Function,
    set: &HashSet<ValueId>,
    dom: &DominatorTree,
    preds: &[Vec<BlockId>],
    value_blocks: &HashMap<ValueId, (Vec<BlockId>, Vec<BlockId>)>,
    body: Vec<BlockId>,
    header: BlockId,
    growth_sites: &[GuardSite],
    entry_sites: &[GuardSite],
    used: &HashSet<BlockId>,
) -> PlanOutcome {
    let mut region: HashSet<BlockId> = body.iter().copied().collect();

    // grow the region until it hosts every touched in-set value's
    // defs and uses (pre-chain absorption). A def block that cannot
    // absorb fails the region; a use block that cannot absorb marks
    // its value escaped instead — every external use of an escaped
    // value reads the merge-bridge cell, so nothing else is absorbed
    // for it.
    let mut escapes: HashSet<ValueId> = HashSet::new();
    loop {
        let touched = touched_values(func, set, &region);
        let mut missing: Vec<(BlockId, ValueId, bool)> = Vec::new();
        for v in &touched {
            let Some((defs, uses)) = value_blocks.get(v) else {
                return PlanOutcome::Fail;
            };
            for b in defs {
                if !region.contains(b) {
                    missing.push((*b, *v, true));
                }
            }
            if !escapes.contains(v) {
                for b in uses {
                    if !region.contains(b) {
                        missing.push((*b, *v, false));
                    }
                }
            }
        }
        if missing.is_empty() {
            break;
        }
        for (b, v, is_def) in missing {
            if region.contains(&b) {
                continue; // absorbed earlier this round
            }
            // a pre-chain block dominates the header and flows only
            // into the region
            if dom.dominates(b, header)
                && successors(&func.blocks[b.0 as usize].term).all(|s| region.contains(&s))
            {
                region.insert(b);
            } else if is_def {
                return PlanOutcome::Fail;
            } else {
                escapes.insert(v);
            }
        }
    }
    // drop escape marks whose external uses were all absorbed after
    // the mark (the block also held another value's def)
    escapes.retain(|v| {
        value_blocks
            .get(v)
            .is_some_and(|(_, uses)| uses.iter().any(|b| !region.contains(b)))
    });

    if region.iter().any(|b| used.contains(b)) {
        return PlanOutcome::Fail;
    }

    // single entry: exactly one region block with external preds (or
    // the function entry), every other block's preds stay inside
    let mut entry = None;
    for &b in &region {
        let ext = preds[b.0 as usize].iter().any(|p| !region.contains(p));
        if ext || b.0 == 0 {
            if entry.is_some() {
                return PlanOutcome::Fail;
            }
            entry = Some(b);
        }
    }
    let Some(entry) = entry else {
        return PlanOutcome::Fail;
    };

    // renamed clone values must not escape: any value the clone will
    // rename (in-set or single-def intermediate defined inside) is
    // used only inside — except an in-set value marked escaped, whose
    // external uses read the merge-bridge cell on both versions. A
    // violation inside an absorbed pre-chain block is a mixed
    // preheader (the `loopSum` env-object setup sharing the sum-init
    // block): request a tail split so the unrelated defs leave the
    // region, and re-plan.
    let cells = multi_def_values(func);
    // Scan in value order. This loop returns on its first violation, so
    // `value_blocks`' hash order picked which one was reported — and
    // with it whether a split happened at all, since a candidate whose
    // `tail_split_point` answers None falls through to Fail while a
    // later one might have answered Some. The split is the only thing
    // this pass leaves behind when planning then fails, so the same
    // input produced two different modules with identical (all-zero)
    // demotion stats.
    let mut scan: Vec<ValueId> = value_blocks.keys().copied().collect();
    scan.sort_by_key(|v| v.0);
    for v in &scan {
        let (defs, uses) = &value_blocks[v];
        if !defs.iter().any(|b| region.contains(b)) {
            continue;
        }
        let renamed = set.contains(v) || !cells.contains(v);
        if renamed && !escapes.contains(v) && uses.iter().any(|b| !region.contains(b)) {
            if let Some(&d) = defs.iter().find(|b| region.contains(b)) {
                if let Some(p) =
                    tail_split_point(func, set, &cells, value_blocks, &region, &body, d)
                {
                    return PlanOutcome::Split(d, p);
                }
            }
            return PlanOutcome::Fail;
        }
    }

    // an external use that is itself an in-set def (a cross-region
    // demotion chain) cannot read the f64 bridge cell — bail out
    for v in &escapes {
        let Some((_, uses)) = value_blocks.get(v) else {
            return PlanOutcome::Fail;
        };
        for b in uses {
            if region.contains(b) {
                continue;
            }
            for inst in &func.blocks[b.0 as usize].insts {
                if !inst.result.is_some_and(|r| set.contains(&r)) {
                    continue;
                }
                let mut reads_v = false;
                visit_value_operands(&inst.kind, |o| reads_v |= o == *v);
                if reads_v {
                    return PlanOutcome::Fail;
                }
            }
        }
    }

    let mut growth: Vec<GuardSite> = growth_sites
        .iter()
        .filter(|s| region.contains(&s.block))
        .cloned()
        .collect();

    // entry checks for hosted guarded sitofp defs. An operand defined
    // outside the region is available at the entry edge — checked
    // once there; one defined inside (the loop-body `sitofp` of an
    // accumulator increment) re-classifies as a per-iteration growth
    // site instead. A function-entry region has no edge to guard.
    let mut entry_guard: Vec<GuardCheck> = Vec::new();
    for s in entry_sites {
        if !region.contains(&s.block) {
            continue;
        }
        let inside = s.checks.iter().any(|c| match c.operand {
            Operand::Value(cv) => value_blocks
                .get(&cv)
                .is_some_and(|(defs, _)| defs.iter().any(|d| region.contains(d))),
            _ => false,
        });
        if inside {
            growth.push(s.clone());
        } else {
            entry_guard.extend(s.checks.iter().copied());
        }
    }
    if !entry_guard.is_empty() && entry.0 == 0 {
        return PlanOutcome::Fail;
    }

    let mut region: Vec<BlockId> = region.into_iter().collect();
    region.sort_by_key(|b| b.0);
    let mut escapes: Vec<ValueId> = escapes.into_iter().collect();
    escapes.sort_by_key(|v| v.0);
    PlanOutcome::Plan(RegionPlan {
        region,
        body,
        entry,
        growth,
        entry_guard,
        escapes,
    })
}

/// Per-iteration profitability: a region fires only when the checks
/// it pays per iteration are covered by the f64 ops it removes.
/// Weights are coarse op-cost classes: `frem`/`fdiv` rewrite away a
/// libcall / hard division (4), `sitofp` a domain crossing (2), the
/// remaining f64 arithmetic 1 each; every per-iteration check costs
/// 1. Only loop-body defs count — pre-chain rewrites and entry
/// guards run once. An unprofitable region (the unbounded-load
/// accumulator that swaps two f64 ops for four checks) evicts its
/// guarded values: the f64 baseline IS the cheaper correct form.
pub(crate) fn profitable(func: &Function, plan: &RegionPlan, set: &HashSet<ValueId>) -> bool {
    use torajs_core::ssa::{BinOp, InstKind};
    let checks: usize = plan.growth.iter().map(|s| s.checks.len()).sum();
    let mut savings = 0usize;
    for b in &plan.body {
        for inst in &func.blocks[b.0 as usize].insts {
            if !inst.result.is_some_and(|r| set.contains(&r)) {
                continue;
            }
            savings += match &inst.kind {
                InstKind::BinOp(BinOp::FRem | BinOp::FDiv, _, _) => 4,
                InstKind::SiToFp(_) => 2,
                InstKind::BinOp(BinOp::FAdd | BinOp::FSub | BinOp::FMul, _, _) => 1,
                _ => 0,
            };
        }
    }
    checks <= savings
}

/// The split point of a mixed pre-chain block: the position of its
/// first in-set def. Valid when the block is absorbed pre-chain (not
/// loop body), something precedes the point, and every def at or
/// after it is region-clean — in-set (bridged when escaping), a
/// shared multi-def cell, or used only inside the region and this
/// block. The head segment then leaves the region on re-plan.
fn tail_split_point(
    func: &Function,
    set: &HashSet<ValueId>,
    cells: &HashSet<ValueId>,
    value_blocks: &HashMap<ValueId, (Vec<BlockId>, Vec<BlockId>)>,
    region: &HashSet<BlockId>,
    body: &[BlockId],
    d: BlockId,
) -> Option<usize> {
    if !region.contains(&d) || body.contains(&d) {
        return None;
    }
    let insts = &func.blocks[d.0 as usize].insts;
    let p = insts
        .iter()
        .position(|i| i.result.is_some_and(|r| set.contains(&r)))?;
    if p == 0 {
        return None;
    }
    for inst in &insts[p..] {
        let Some(r) = inst.result else { continue };
        if set.contains(&r) || cells.contains(&r) {
            continue;
        }
        let clean = value_blocks
            .get(&r)
            .is_none_or(|(_, uses)| uses.iter().all(|b| region.contains(b) || *b == d));
        if !clean {
            return None;
        }
    }
    Some(p)
}

#[cfg(test)]
mod tests {
    use super::super::{FloatDemoteStats, demote_in_function};
    use crate::interval;
    use torajs_core::ssa::{
        BinOp, Block, BlockId, Function, Inst, InstKind, Module, Operand, Terminator, Type,
        ValueId, ValueInfo,
    };

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

    fn func(ret: Type, values: Vec<Type>, blocks: Vec<Block>) -> Function {
        Function {
            name: "f".into(),
            params: vec![],
            ret,
            blocks,
            values: values
                .into_iter()
                .map(|ty| ValueInfo { ty, name: None })
                .collect(),
            current_origin: None,
        }
    }

    fn block(id: u32, insts: Vec<Inst>, term: Terminator) -> Block {
        Block {
            id: BlockId(id),
            insts,
            term,
        }
    }

    fn run(f: Function) -> (Module, FloatDemoteStats) {
        let mut m = Module {
            funcs: vec![f],
            ..Default::default()
        };
        let facts = interval::analyze_module(&m);
        let mut stats = FloatDemoteStats::default();
        demote_in_function(&mut m.funcs[0], &facts[0], &mut stats);
        (m, stats)
    }

    /// the accumulator loop `for (i…) sum += i` with the loop-body
    /// sitofp and a double-variable fadd, no escaping use: the
    /// in-region sitofp re-classifies as a per-iteration growth site
    /// and the whole closure demotes (the chunk-A shape — escape
    /// handling is the merge-bridge step on top of this).
    fn accumulator_loop(op: BinOp) -> Function {
        let mut f = func(
            Type::Void,
            vec![
                Type::I64,  // %0 n param
                Type::F64,  // %1 sum cell
                Type::I64,  // %2 i cell
                Type::Bool, // %3 icmp
                Type::F64,  // %4 sitofp
                Type::F64,  // %5 fadd
                Type::I64,  // %6 i + 1
            ],
            vec![
                block(
                    0,
                    vec![
                        inst(1, InstKind::Copy(Type::F64, Operand::ConstF64(0.0))),
                        inst(2, InstKind::Copy(Type::I64, Operand::ConstI64(0))),
                    ],
                    Terminator::Br(BlockId(1)),
                ),
                block(
                    1,
                    vec![inst(
                        3,
                        InstKind::ICmp(torajs_core::ssa::IPred::Slt, v(2), v(0)),
                    )],
                    Terminator::CondBr {
                        cond: v(3),
                        then_blk: BlockId(2),
                        else_blk: BlockId(3),
                    },
                ),
                block(
                    2,
                    vec![
                        inst(4, InstKind::SiToFp(v(2))),
                        inst(5, InstKind::BinOp(op, v(1), v(4))),
                        inst(1, InstKind::Copy(Type::F64, v(5))),
                        inst(6, InstKind::BinOp(BinOp::Add, v(2), Operand::ConstI64(1))),
                        inst(2, InstKind::Copy(Type::I64, v(6))),
                    ],
                    Terminator::Br(BlockId(1)),
                ),
                block(3, vec![], Terminator::Ret(None)),
            ],
        );
        f.params = vec![ValueId(0)];
        f
    }

    #[test]
    fn accumulator_loop_demotes_with_per_iter_guards() {
        let (m, stats) = run(accumulator_loop(BinOp::FAdd));
        // %1, %4, %5 demote; the in-region sitofp takes a per-iter
        // guard and the fadd one post-result check (both single-sided
        // — the non-negative facts prune the lower bounds).
        assert_eq!(stats.values_demoted, 3);
        assert_eq!(stats.loops_versioned, 1);
        assert_eq!(stats.guards_inserted, 2);
        assert_eq!(stats.entry_guards, 0);
        assert_eq!(stats.bridges_inserted, 0);
        let f = &m.funcs[0];
        // fast path rewrote the sitofp to an i64 copy (the guard
        // splits moved it into a continuation block)
        let has_copy = f.blocks.iter().flat_map(|b| &b.insts).any(|i| {
            matches!(
                i.kind,
                InstKind::Copy(Type::I64, Operand::Value(ValueId(2)))
            )
        });
        assert!(has_copy, "sitofp must rewrite to an i64 copy");
        assert_eq!(f.values[1].ty, Type::I64);
        // the slow clone preserves an fadd over fresh ids
        let fadd_count = f
            .blocks
            .iter()
            .flat_map(|b| &b.insts)
            .filter(|i| matches!(i.kind, InstKind::BinOp(BinOp::FAdd, _, _)))
            .count();
        assert_eq!(fadd_count, 1, "slow clone keeps the f64 fadd");
    }

    #[test]
    fn mixed_preheader_splits_and_fires() {
        // the loopSum shape: the sum-init block also allocates an
        // env-like slot read after the loop. The planner requests a
        // pre-chain tail split, the head segment (alloca) leaves the
        // region on re-plan, and the closure demotes with the merge
        // bridge.
        let mut f = accumulator_loop(BinOp::FAdd);
        f.ret = Type::F64;
        f.values.push(ValueInfo {
            ty: Type::Ptr,
            name: None,
        }); // %7 env-like alloca
        f.values.push(ValueInfo {
            ty: Type::I64,
            name: None,
        }); // %8 post-loop load
        f.blocks[0]
            .insts
            .insert(0, inst(7, InstKind::Alloca(Type::I64)));
        f.blocks[3]
            .insts
            .push(inst(8, InstKind::Load(Type::I64, v(7), 0)));
        f.blocks[3].term = Terminator::Ret(Some(v(1)));
        let (m, stats) = run(f);
        assert_eq!(stats.values_demoted, 3);
        assert_eq!(stats.loops_versioned, 1);
        assert_eq!(stats.merge_cells, 1);
        let f0 = &m.funcs[0];
        // the alloca was split out of the region — not cloned
        let allocas = f0
            .blocks
            .iter()
            .flat_map(|b| &b.insts)
            .filter(|i| matches!(i.kind, InstKind::Alloca(_)))
            .count();
        assert_eq!(allocas, 1, "env alloca stays out of the slow clone");
        assert_eq!(f0.values[1].ty, Type::I64);
    }

    #[test]
    fn double_variable_fmul_evicts() {
        // prod *= i (seeded 1, so the interval cannot collapse it to
        // the constant 0): the product has no operand window — the
        // growth def cannot be guarded and the closure stays f64.
        let mut f = accumulator_loop(BinOp::FMul);
        f.blocks[0].insts[0].kind = InstKind::Copy(Type::F64, Operand::ConstF64(1.0));
        let (m, stats) = run(f);
        assert_eq!(stats.values_demoted, 0);
        assert_eq!(stats.loops_versioned, 0);
        assert!(matches!(
            m.funcs[0].blocks[2].insts[1].kind,
            InstKind::BinOp(BinOp::FMul, _, _)
        ));
        assert_eq!(m.funcs[0].values[1].ty, Type::F64);
    }

    #[test]
    fn escaping_accumulator_takes_merge_bridge() {
        // same loop but the sum cell is returned after the loop: the
        // escaping use takes a merge bridge — the ret reads a fresh
        // f64 cell written on both exit edges and the closure still
        // demotes (the `return sum` accumulator shape).
        let mut f = accumulator_loop(BinOp::FAdd);
        f.ret = Type::F64;
        f.blocks[3].term = Terminator::Ret(Some(v(1)));
        let (m, stats) = run(f);
        assert_eq!(stats.values_demoted, 3);
        assert_eq!(stats.loops_versioned, 1);
        assert_eq!(stats.merge_cells, 1);
        assert_eq!(stats.bridges_inserted, 0);
        let f0 = &m.funcs[0];
        assert_eq!(f0.values[1].ty, Type::I64);
        let Terminator::Ret(Some(Operand::Value(w))) = f0.blocks[3].term else {
            panic!("expected ret of the bridge cell");
        };
        assert_ne!(w, ValueId(1));
        assert_eq!(f0.values[w.0 as usize].ty, Type::F64);
        // both versions write the bridge cell at their exit
        let writebacks = f0
            .blocks
            .iter()
            .flat_map(|b| &b.insts)
            .filter(|i| i.result == Some(w))
            .count();
        assert_eq!(writebacks, 2, "one fast + one slow exit writeback");
    }
}
