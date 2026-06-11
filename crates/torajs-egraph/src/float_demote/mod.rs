//! Float demotion — representation selection for integer-valued f64
//! flows (RFC 20260611-int-range-float-demotion phases 1b-i/1b-ii;
//! the AOT analogue of TurboFan simplified lowering's int
//! representation choice, with explicit guards + loop versioning
//! standing in for deopt). Consumes the interval analysis: an f64
//! value with a fact is integer-valued, and a fact bounded inside
//! ±2^53 means every op that produced it was exact — the i64
//! computation produces the bit-identical trajectory.
//!
//! Demotion set: the largest set of f64 values where every def either
//! rewrites exactly or accepts a runtime guard (`fit` holds the
//! per-def judgments, `guard` the window math):
//!
//! * `sitofp x` with `x`'s fact inside ±2^53 (conversion exact) —
//!   becomes `copy x`; an unbounded `x` takes an entry guard on the
//!   hosting region's entry edge (phase 1b-ii)
//! * `frem a, C` (C ≠ 0 integer const, a in set) — `srem` (truncated
//!   remainder, dividend-sign: identical to fmod on integers)
//! * `fdiv a, 2` whose result carries a fact — the interval transfer
//!   only proves fdiv under the frem-parity branch (even dividend ⇒
//!   exact) — becomes `sdiv`
//! * `fadd`/`fsub`/`fmul` whose result fact is finite (clamp53 left
//!   bounds < 2^53 ⇒ no rounding) — integer add/sub/mul; a growth
//!   site (bound widened to a sentinel — the collatz `3n+1` shape)
//!   over an in-set value and an integer constant takes a
//!   per-iteration window check + side exit instead (phase 1b-ii,
//!   planned by `plan`, installed by `apply`)
//! * `copy` of an in-set value (loop cells: every def must qualify)
//!
//! Values failing every path drop out and the removal propagates;
//! guarded values whose sites cannot be hosted by a versionable
//! region (no natural loop, escaping uses, multi-entry) are evicted
//! the same way — the pass never fires partially.
//!
//! Uses are patched in place: an `fcmp` whose operands are all in-set
//! or integer consts becomes the matching `icmp` (the set is NaN-free
//! so ordered/unordered collapse); any other f64 consumer gets a
//! `sitofp` bridge inserted before it. A bridged value must be
//! provably never `-0.0` (i64 has no negative zero, so the bridge
//! would launder `-0` into `+0` — observable through `1/x` or
//! `Object.is` downstream); a value needing a bridge without the
//! `-0`-free proof leaves the set before anything is mutated.
//!
//! Runs between the interval analysis and sext_elide — demoted values
//! keep their ValueId and fact, so in-i32-range demoted cells seed
//! sext_elide for free. `TORAJS_FLOAT_DEMOTE_OFF=1` skips,
//! `TORAJS_FLOAT_DEMOTE_STATS=1` dumps counters.

mod apply;
mod cfg;
mod fit;
mod guard;
mod merge;
mod plan;

use std::collections::{HashMap, HashSet};

use torajs_core::ssa::{
    BlockId, Function, Inst, InstKind, Module, Operand, Terminator, Type, ValueId, ValueInfo,
};

use crate::interval::transfer::NumFact;
use crate::rc_peephole::visit_value_operands;
use crate::slot_forward::rewrite_operands;

use fit::Fit;
use guard::GuardSite;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct FloatDemoteStats {
    /// f64 values rewritten to i64.
    pub values_demoted: u32,
    /// fcmp instructions converted to icmp.
    pub fcmp_converted: u32,
    /// sitofp bridges inserted at out-of-set f64 uses.
    pub bridges_inserted: u32,
    /// regions cloned into fast/slow versions (1b-ii).
    pub loops_versioned: u32,
    /// per-iteration growth checks inserted (1b-ii).
    pub guards_inserted: u32,
    /// entry-edge checks inserted (1b-ii).
    pub entry_guards: u32,
    /// escaped accumulator cells bridged at region exits (item 13).
    pub merge_cells: u32,
}

pub fn demote_floats(module: &mut Module, facts: &[HashMap<ValueId, NumFact>]) -> FloatDemoteStats {
    let mut stats = FloatDemoteStats::default();
    let empty = HashMap::new();
    for (fi, func) in module.funcs.iter_mut().enumerate() {
        if func.is_declaration() {
            continue;
        }
        demote_in_function(func, facts.get(fi).unwrap_or(&empty), &mut stats);
    }
    stats
}

fn demote_in_function(
    func: &mut Function,
    facts: &HashMap<ValueId, NumFact>,
    stats: &mut FloatDemoteStats,
) {
    let Some((set, plans)) = compute_demote_set(func, facts) else {
        return;
    };
    // mint merge-bridge cells and re-point external uses first —
    // before cloning (only original blocks exist) and before mutation
    // (the rewritten uses must not take per-use sitofp bridges)
    let bridges: Vec<merge::MergeBridge> = plans
        .iter()
        .map(|p| merge::rewrite_escapes(func, p))
        .collect();
    // clone the slow (f64) regions before any mutation
    let clones: Vec<apply::RegionClone> = plans
        .iter()
        .map(|p| apply::clone_region(func, p, &set))
        .collect();
    let track: HashSet<(BlockId, usize)> = plans.iter().flat_map(|p| p.growth_keys()).collect();
    let fast_pos = mutate_function(func, &set, &track, stats);
    for ((plan, clone), bridge) in plans.iter().zip(&clones).zip(&bridges) {
        merge::install_exits(func, plan, clone, bridge);
        stats.merge_cells += bridge.len() as u32;
        let (g, eg) = apply::install_guards(func, plan, clone, &set, &fast_pos);
        stats.guards_inserted += g;
        stats.entry_guards += eg;
    }
    stats.loops_versioned += plans.len() as u32;
    stats.values_demoted += set.len() as u32;
}

/// The demotion set plus the versioning plans its guarded defs need.
/// `None` = nothing demotes in this function. Mutates only on a
/// planner split request (mixed preheader normalization — a pure
/// control-flow split, semantics-preserving even if nothing fires).
fn compute_demote_set(
    func: &mut Function,
    facts: &HashMap<ValueId, NumFact>,
) -> Option<(HashSet<ValueId>, Vec<plan::RegionPlan>)> {
    // def sites per value (loop cells have several).
    let mut defs: HashMap<ValueId, Vec<InstKind>> = HashMap::new();
    for block in &func.blocks {
        for inst in &block.insts {
            if let Some(r) = inst.result {
                defs.entry(r).or_default().push(inst.kind.clone());
            }
        }
    }

    // candidate set: integer-valued f64 values, shrunk until every
    // def fits AND every bridged escape is -0-free AND every guarded
    // def is hosted by a versionable region.
    let mut set: HashSet<ValueId> = facts
        .keys()
        .filter(|v| func.values[v.0 as usize].ty == Type::F64)
        .copied()
        .collect();
    loop {
        // def-fit fixpoint (greatest: shrink from the full candidate
        // set so loop cells can certify through themselves)
        loop {
            let snapshot = set.clone();
            set.retain(|v| {
                defs.get(v).is_some_and(|dl| {
                    !dl.is_empty()
                        && dl
                            .iter()
                            .all(|d| fit::def_fit(d, v, facts, &snapshot) != Fit::No)
                })
            });
            if set.len() == snapshot.len() {
                break;
            }
        }
        if set.is_empty() {
            return None;
        }
        // -0-free closure — coinductive (greatest fixpoint): assume
        // every in-set value clean, evict anything whose def could
        // mint a -0 under the current assumption, repeat. A least
        // fixpoint would never admit loop cells (cell ↔ op cycles);
        // the survivors here provably have no -0 source on any path.
        let mut nz: HashSet<ValueId> = set.clone();
        loop {
            let snapshot = nz.clone();
            nz.retain(|v| {
                defs[v]
                    .iter()
                    .all(|d| fit::def_no_neg_zero(d, facts, &snapshot))
            });
            if nz.len() == snapshot.len() {
                break;
            }
        }
        // classify every use: an in-set def operand or a convertible
        // fcmp operand is a useful (i64-native) use; anything else
        // needs a sitofp bridge. A bridged escape must be -0-free,
        // and a value with no useful use at all leaves the set — a
        // demote whose every consumer bridges back is a net loss
        // (copy + sitofp where a single sitofp stood).
        let mut bad: HashSet<ValueId> = HashSet::new();
        let mut useful: HashSet<ValueId> = HashSet::new();
        for block in &func.blocks {
            for inst in &block.insts {
                if inst.result.is_some_and(|r| set.contains(&r)) {
                    visit_value_operands(&inst.kind, |v| {
                        if set.contains(&v) {
                            useful.insert(v);
                        }
                    });
                    continue;
                }
                if let InstKind::FCmp(_, a, b) = &inst.kind {
                    if fit::fcmp_convertible(a, b, &set) {
                        for op in [a, b] {
                            if let Operand::Value(v) = op {
                                useful.insert(*v);
                            }
                        }
                        continue;
                    }
                }
                visit_value_operands(&inst.kind, |v| {
                    if set.contains(&v) && !nz.contains(&v) {
                        bad.insert(v);
                    }
                });
            }
            if let Terminator::Ret(Some(Operand::Value(v))) = &block.term {
                if set.contains(v) && !nz.contains(v) {
                    bad.insert(*v);
                }
            }
        }
        for v in &set {
            if !useful.contains(v) {
                bad.insert(*v);
            }
        }
        if !bad.is_empty() {
            for v in bad {
                set.remove(&v);
            }
            continue;
        }
        // guarded defs need a hosting region (1b-ii)
        let (growth, entries) = collect_guard_sites(func, &set, facts);
        if growth.is_empty() && entries.is_empty() {
            return Some((set, Vec::new()));
        }
        match plan::plan_regions(func, &set, growth, entries) {
            Ok(plans) => {
                // profitability gate: an unprofitable region's guarded
                // values evict like an infeasible one — the f64
                // baseline is the cheaper correct form there
                let costly: HashSet<ValueId> = plans
                    .iter()
                    .filter(|p| !plan::profitable(func, p, &set))
                    .flat_map(|p| p.growth.iter().map(|s| s.result))
                    .collect();
                if costly.is_empty() {
                    return Some((set, plans));
                }
                for v in costly {
                    set.remove(&v);
                }
            }
            Err(plan::PlanFail::Evict(evict)) => {
                for v in evict {
                    set.remove(&v);
                }
            }
            Err(plan::PlanFail::Split(b, p)) => {
                // sites re-collect next round on the split shape
                apply::split_block_at(func, b, &[p]);
            }
        }
    }
}

/// The non-exact (guarded) def sites of the final set, split into
/// growth sites and sitofp entries. Entry sites carry their inst
/// position too: one whose operand is defined inside the hosting
/// region re-classifies as a per-iteration growth site during
/// planning (the `sum += xs[j]` loop-body sitofp shape).
fn collect_guard_sites(
    func: &Function,
    set: &HashSet<ValueId>,
    facts: &HashMap<ValueId, NumFact>,
) -> (Vec<GuardSite>, Vec<GuardSite>) {
    let mut growth: Vec<GuardSite> = Vec::new();
    let mut entries: Vec<GuardSite> = Vec::new();
    for block in &func.blocks {
        for (i, inst) in block.insts.iter().enumerate() {
            let Some(r) = inst.result else { continue };
            if !set.contains(&r) || fit::def_exact(&inst.kind, &r, facts, set) {
                continue;
            }
            match &inst.kind {
                InstKind::SiToFp(x) => {
                    if let Some(checks) = guard::entry_checks(x, facts) {
                        entries.push(GuardSite {
                            block: block.id,
                            inst_idx: i,
                            result: r,
                            checks,
                            post: false,
                        });
                    }
                }
                _ => {
                    if let Some((checks, post)) = guard::growth_checks(&inst.kind, r, facts) {
                        growth.push(GuardSite {
                            block: block.id,
                            inst_idx: i,
                            result: r,
                            checks,
                            post,
                        });
                    }
                }
            }
        }
    }
    (growth, entries)
}

/// Rewrite the function in place. Single walk — every inst takes
/// exactly one of three branches (demoted def rewrite / fcmp→icmp
/// conversion / bridge insertion), so a freshly converted icmp's i64
/// operands are never mistaken for f64 uses needing a bridge. The
/// cloned slow blocks reference only fresh / shared / external ids,
/// none in the set, so the walk leaves them untouched. Returns the
/// post-mutation positions of the tracked (guarded) defs.
fn mutate_function(
    func: &mut Function,
    set: &HashSet<ValueId>,
    track: &HashSet<(BlockId, usize)>,
    stats: &mut FloatDemoteStats,
) -> HashMap<(BlockId, usize), usize> {
    let mut fast_pos: HashMap<(BlockId, usize), usize> = HashMap::new();
    let Function {
        blocks,
        values,
        ret,
        ..
    } = func;
    for block in blocks.iter_mut() {
        let old = std::mem::take(&mut block.insts);
        let mut out: Vec<Inst> = Vec::with_capacity(old.len());
        for (i, mut inst) in old.into_iter().enumerate() {
            if inst.result.is_some_and(|r| set.contains(&r)) {
                if track.contains(&(block.id, i)) {
                    fast_pos.insert((block.id, i), out.len());
                }
                inst.kind = fit::rewrite_def(&inst.kind);
                out.push(inst);
                continue;
            }
            if let InstKind::FCmp(p, a, b) = &inst.kind {
                if fit::fcmp_convertible(a, b, set) {
                    inst.kind =
                        InstKind::ICmp(fit::fpred_to_ipred(*p), fit::int_op(a), fit::int_op(b));
                    stats.fcmp_converted += 1;
                    out.push(inst);
                    continue;
                }
            }
            let mut bridge_ops: Vec<ValueId> = Vec::new();
            visit_value_operands(&inst.kind, |v| {
                if set.contains(&v) && !bridge_ops.contains(&v) {
                    bridge_ops.push(v);
                }
            });
            for v in bridge_ops {
                let b = push_value(values, Type::F64);
                out.push(Inst {
                    result: Some(b),
                    kind: InstKind::SiToFp(Operand::Value(v)),
                    origin: inst.origin,
                });
                let mut map = HashMap::new();
                map.insert(v, Operand::Value(b));
                rewrite_operands(&mut inst.kind, &map);
                stats.bridges_inserted += 1;
            }
            out.push(inst);
        }
        if let Terminator::Ret(Some(Operand::Value(v))) = &block.term {
            if set.contains(v) && *ret == Type::F64 {
                let b = push_value(values, Type::F64);
                out.push(Inst {
                    result: Some(b),
                    kind: InstKind::SiToFp(Operand::Value(*v)),
                    origin: None,
                });
                block.term = Terminator::Ret(Some(Operand::Value(b)));
                stats.bridges_inserted += 1;
            }
        }
        block.insts = out;
    }

    for v in set {
        values[v.0 as usize].ty = Type::I64;
    }
    fast_pos
}

pub(crate) fn push_value(values: &mut Vec<ValueInfo>, ty: Type) -> ValueId {
    let id = ValueId(values.len() as u32);
    values.push(ValueInfo { ty, name: None });
    id
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interval;
    use torajs_core::ssa::{BinOp, Block, BlockId, FPred, IPred, Module};

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

    fn cf(x: f64) -> Operand {
        Operand::ConstF64(x)
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

    /// halve loop: sitofp entry, frem/fcmp parity guard, fdiv-by-2 in
    /// the even arm, f64 ret (bridge). The full closure demotes.
    fn halve_loop() -> Function {
        func(
            Type::F64,
            vec![
                Type::I64,  // %0 entry const
                Type::F64,  // %1 sitofp
                Type::F64,  // %2 n cell
                Type::F64,  // %3 frem
                Type::Bool, // %4 fcmp
                Type::F64,  // %5 fdiv
            ],
            vec![
                block(
                    0,
                    vec![
                        inst(0, InstKind::Copy(Type::I64, Operand::ConstI64(100000))),
                        inst(1, InstKind::SiToFp(v(0))),
                        inst(2, InstKind::Copy(Type::F64, v(1))),
                    ],
                    Terminator::Br(BlockId(1)),
                ),
                block(
                    1,
                    vec![
                        inst(3, InstKind::BinOp(BinOp::FRem, v(2), cf(2.0))),
                        inst(4, InstKind::FCmp(FPred::Oeq, v(3), cf(0.0))),
                    ],
                    Terminator::CondBr {
                        cond: v(4),
                        then_blk: BlockId(2),
                        else_blk: BlockId(3),
                    },
                ),
                block(
                    2,
                    vec![
                        inst(5, InstKind::BinOp(BinOp::FDiv, v(2), cf(2.0))),
                        inst(2, InstKind::Copy(Type::F64, v(5))),
                    ],
                    Terminator::Br(BlockId(1)),
                ),
                block(3, vec![], Terminator::Ret(Some(v(2)))),
            ],
        )
    }

    #[test]
    fn halve_loop_demotes_fully() {
        let (m, stats) = run(halve_loop());
        // %1, %2, %3, %5 demote; the fcmp converts; ret bridges.
        assert_eq!(stats.values_demoted, 4);
        assert_eq!(stats.fcmp_converted, 1);
        assert_eq!(stats.bridges_inserted, 1);
        assert_eq!(stats.loops_versioned, 0);
        let f = &m.funcs[0];
        assert!(matches!(
            f.blocks[1].insts[0].kind,
            InstKind::BinOp(BinOp::SRem, _, Operand::ConstI64(2))
        ));
        assert!(matches!(
            f.blocks[1].insts[1].kind,
            InstKind::ICmp(IPred::Eq, _, Operand::ConstI64(0))
        ));
        assert!(matches!(
            f.blocks[2].insts[0].kind,
            InstKind::BinOp(BinOp::SDiv, _, Operand::ConstI64(2))
        ));
        // sitofp entry became a copy of the i64 source
        assert!(matches!(
            f.blocks[0].insts[1].kind,
            InstKind::Copy(Type::I64, Operand::Value(ValueId(0)))
        ));
        // ret goes through a fresh sitofp bridge
        let Terminator::Ret(Some(Operand::Value(rb))) = f.blocks[3].term else {
            panic!("expected ret of bridge");
        };
        assert!(matches!(
            f.blocks[3].insts.last().unwrap().kind,
            InstKind::SiToFp(Operand::Value(ValueId(2)))
        ));
        assert_eq!(f.blocks[3].insts.last().unwrap().result, Some(rb));
        // demoted cells are i64-typed now
        assert_eq!(f.values[2].ty, Type::I64);
    }

    #[test]
    fn folded_const_entry_rewrites_to_int_const() {
        // a GVN-folded f64 const entry (Copy(F64, ConstF64)) must
        // rewrite its operand to ConstI64 — the baseline tier's GPR
        // materialization rejects f64 constants (param-width-widen-001
        // jit regression, steps(1) const call site).
        let mut f = halve_loop();
        f.blocks[0].insts[1].kind = InstKind::Copy(Type::F64, cf(100000.0));
        let (m, stats) = run(f);
        assert_eq!(stats.values_demoted, 4);
        assert!(matches!(
            m.funcs[0].blocks[0].insts[1].kind,
            InstKind::Copy(Type::I64, Operand::ConstI64(100000))
        ));
    }

    #[test]
    fn growth_loop_with_escaping_ret_takes_merge_bridge() {
        // collatz odd arm with the SCC cell returned after the loop:
        // the guarded fmul/fadd need versioning and the ret use
        // escapes the region — the escape takes a merge bridge (item
        // 13): the ret reads a fresh f64 cell written on both exit
        // edges, and the closure demotes instead of evicting.
        let f = func(
            Type::F64,
            vec![
                Type::I64,  // %0
                Type::F64,  // %1 sitofp
                Type::F64,  // %2 n cell
                Type::F64,  // %3 fmul
                Type::F64,  // %4 fadd
                Type::Bool, // %5 fcmp
            ],
            vec![
                block(
                    0,
                    vec![
                        inst(0, InstKind::Copy(Type::I64, Operand::ConstI64(7))),
                        inst(1, InstKind::SiToFp(v(0))),
                        inst(2, InstKind::Copy(Type::F64, v(1))),
                    ],
                    Terminator::Br(BlockId(1)),
                ),
                block(
                    1,
                    vec![
                        inst(3, InstKind::BinOp(BinOp::FMul, cf(3.0), v(2))),
                        inst(4, InstKind::BinOp(BinOp::FAdd, v(3), cf(1.0))),
                        inst(2, InstKind::Copy(Type::F64, v(4))),
                        inst(5, InstKind::FCmp(FPred::Une, v(2), cf(1.0))),
                    ],
                    Terminator::CondBr {
                        cond: v(5),
                        then_blk: BlockId(1),
                        else_blk: BlockId(2),
                    },
                ),
                block(2, vec![], Terminator::Ret(Some(v(2)))),
            ],
        );
        let (m, stats) = run(f);
        // %1, %2, %3, %4 demote; the n cell bridges at the exits
        assert_eq!(stats.values_demoted, 4);
        assert_eq!(stats.loops_versioned, 1);
        assert_eq!(stats.merge_cells, 1);
        let f0 = &m.funcs[0];
        // the ret reads the bridge cell, not the demoted i64 cell
        let Terminator::Ret(Some(Operand::Value(w))) = f0.blocks[2].term else {
            panic!("expected ret of the bridge cell");
        };
        assert_ne!(w, ValueId(2));
        assert_eq!(f0.values[w.0 as usize].ty, Type::F64);
        assert_eq!(f0.values[2].ty, Type::I64);
        // the fast exit writes it via sitofp(n), the slow exit copies
        // the cloned f64 cell
        let fast_wb = f0.blocks.iter().flat_map(|b| &b.insts).any(|i| {
            i.result == Some(w) && matches!(i.kind, InstKind::SiToFp(Operand::Value(ValueId(2))))
        });
        let slow_wb = f0
            .blocks
            .iter()
            .flat_map(|b| &b.insts)
            .any(|i| i.result == Some(w) && matches!(i.kind, InstKind::Copy(Type::F64, _)));
        assert!(fast_wb, "fast exit bridges through sitofp");
        assert!(slow_wb, "slow exit copies the cloned cell");
    }

    #[test]
    fn non_integer_entry_stays_f64() {
        let f = func(
            Type::F64,
            vec![Type::F64, Type::F64],
            vec![block(
                0,
                vec![
                    inst(0, InstKind::Copy(Type::F64, cf(0.5))),
                    inst(1, InstKind::BinOp(BinOp::FAdd, v(0), cf(1.0))),
                ],
                Terminator::Ret(Some(v(1))),
            )],
        );
        let (_, stats) = run(f);
        assert_eq!(stats.values_demoted, 0);
    }

    #[test]
    fn negative_dividend_frem_not_demoted() {
        // frem over a possibly-negative dividend can yield -0, so its
        // result may not bridge; the sitofp entry then has no useful
        // i64 use left (a bridge-only demote is a net loss) and the
        // whole chain stays f64.
        let f = func(
            Type::F64,
            vec![Type::I64, Type::F64, Type::F64],
            vec![block(
                0,
                vec![
                    inst(0, InstKind::Copy(Type::I64, Operand::ConstI64(-8))),
                    inst(1, InstKind::SiToFp(v(0))),
                    inst(2, InstKind::BinOp(BinOp::FRem, v(1), cf(2.0))),
                ],
                Terminator::Ret(Some(v(2))),
            )],
        );
        let (m, stats) = run(f);
        assert_eq!(stats, FloatDemoteStats::default());
        let f0 = &m.funcs[0];
        assert!(
            f0.blocks[0]
                .insts
                .iter()
                .any(|i| matches!(i.kind, InstKind::BinOp(BinOp::FRem, _, _)))
        );
        assert!(matches!(f0.blocks[0].insts[1].kind, InstKind::SiToFp(_)));
        assert_eq!(f0.values[1].ty, Type::F64);
        assert_eq!(f0.values[2].ty, Type::F64);
    }

    #[test]
    fn fdiv_without_parity_proof_not_demoted() {
        let f = func(
            Type::F64,
            vec![Type::I64, Type::F64, Type::F64],
            vec![block(
                0,
                vec![
                    inst(0, InstKind::Copy(Type::I64, Operand::ConstI64(5))),
                    inst(1, InstKind::SiToFp(v(0))),
                    inst(2, InstKind::BinOp(BinOp::FDiv, v(1), cf(2.0))),
                ],
                Terminator::Ret(Some(v(2))),
            )],
        );
        let (m, stats) = run(f);
        // 5 / 2 = 2.5: no parity proof, the fdiv stays f64; the
        // entry's only use would bridge, so nothing fires.
        assert_eq!(stats, FloatDemoteStats::default());
        let f0 = &m.funcs[0];
        assert!(
            f0.blocks[0]
                .insts
                .iter()
                .any(|i| matches!(i.kind, InstKind::BinOp(BinOp::FDiv, _, _)))
        );
        assert_eq!(f0.values[1].ty, Type::F64);
        assert_eq!(f0.values[2].ty, Type::F64);
    }

    /// canonical collatz: the count cell exits the loop, the f64 SCC
    /// does not. The full closure demotes with two growth guards + a
    /// versioned slow loop (1b-ii).
    fn collatz_loop() -> Function {
        func(
            Type::I64,
            vec![
                Type::I64,  // %0 x init
                Type::F64,  // %1 sitofp
                Type::F64,  // %2 n cell
                Type::Bool, // %3 fcmp une
                Type::F64,  // %4 frem
                Type::Bool, // %5 fcmp oeq
                Type::I64,  // %6 count cell
                Type::F64,  // %7 fdiv
                Type::F64,  // %8 fmul
                Type::F64,  // %9 fadd
                Type::I64,  // %10 count add
            ],
            vec![
                block(
                    0,
                    vec![
                        inst(0, InstKind::Copy(Type::I64, c(1000000))),
                        inst(6, InstKind::Copy(Type::I64, c(0))),
                    ],
                    Terminator::Br(BlockId(1)),
                ),
                block(
                    1,
                    vec![inst(1, InstKind::SiToFp(v(0)))],
                    Terminator::Br(BlockId(2)),
                ),
                block(
                    2,
                    vec![inst(2, InstKind::Copy(Type::F64, v(1)))],
                    Terminator::Br(BlockId(3)),
                ),
                block(
                    3,
                    vec![inst(3, InstKind::FCmp(FPred::Une, v(2), cf(1.0)))],
                    Terminator::CondBr {
                        cond: v(3),
                        then_blk: BlockId(4),
                        else_blk: BlockId(8),
                    },
                ),
                block(
                    4,
                    vec![
                        inst(4, InstKind::BinOp(BinOp::FRem, v(2), cf(2.0))),
                        inst(5, InstKind::FCmp(FPred::Oeq, v(4), cf(0.0))),
                    ],
                    Terminator::CondBr {
                        cond: v(5),
                        then_blk: BlockId(5),
                        else_blk: BlockId(6),
                    },
                ),
                block(
                    5,
                    vec![
                        inst(7, InstKind::BinOp(BinOp::FDiv, v(2), cf(2.0))),
                        inst(2, InstKind::Copy(Type::F64, v(7))),
                    ],
                    Terminator::Br(BlockId(7)),
                ),
                block(
                    6,
                    vec![
                        inst(8, InstKind::BinOp(BinOp::FMul, cf(3.0), v(2))),
                        inst(9, InstKind::BinOp(BinOp::FAdd, v(8), cf(1.0))),
                        inst(2, InstKind::Copy(Type::F64, v(9))),
                    ],
                    Terminator::Br(BlockId(7)),
                ),
                block(
                    7,
                    vec![
                        inst(10, InstKind::BinOp(BinOp::Add, v(6), c(1))),
                        inst(6, InstKind::Copy(Type::I64, v(10))),
                    ],
                    Terminator::Br(BlockId(3)),
                ),
                block(8, vec![], Terminator::Ret(Some(v(6)))),
            ],
        )
    }

    #[test]
    fn collatz_versions_with_growth_guards() {
        let (m, stats) = run(collatz_loop());
        // %1, %2, %4, %7, %8, %9 demote; both fcmps convert; the 3n
        // and +1 sites each take one upper-bound guard; the region
        // clones once; the count cell needs no bridge.
        assert_eq!(stats.values_demoted, 6);
        assert_eq!(stats.fcmp_converted, 2);
        assert_eq!(stats.bridges_inserted, 0);
        assert_eq!(stats.loops_versioned, 1);
        assert_eq!(stats.guards_inserted, 2);
        assert_eq!(stats.entry_guards, 0);

        let f = &m.funcs[0];
        // fast path rewrote the loop ops to integer forms
        assert!(matches!(
            f.blocks[4].insts[0].kind,
            InstKind::BinOp(BinOp::SRem, _, Operand::ConstI64(2))
        ));
        assert!(matches!(
            f.blocks[5].insts[0].kind,
            InstKind::BinOp(BinOp::SDiv, _, Operand::ConstI64(2))
        ));
        // the fmul guard checks n > (2^53-1)/3 (lower side proven by
        // the non-negative fact)
        let has_mul_guard = f.blocks.iter().any(|b| {
            b.insts.iter().any(|i| {
                matches!(
                    i.kind,
                    InstKind::ICmp(IPred::Sgt, _, Operand::ConstI64(3_002_399_751_580_330))
                )
            })
        });
        assert!(has_mul_guard, "missing 3n window check");
        // the slow clone preserves an frem
        let frem_count = f
            .blocks
            .iter()
            .flat_map(|b| &b.insts)
            .filter(|i| matches!(i.kind, InstKind::BinOp(BinOp::FRem, _, _)))
            .count();
        assert_eq!(frem_count, 1, "slow clone keeps the f64 frem");
        // a side exit bridges the n cell back to f64
        let has_writeback = f
            .blocks
            .iter()
            .flat_map(|b| &b.insts)
            .any(|i| matches!(i.kind, InstKind::SiToFp(Operand::Value(ValueId(2)))));
        assert!(has_writeback, "side exit writes the n cell back");
        // demoted cell is i64-typed now
        assert_eq!(f.values[2].ty, Type::I64);
    }

    #[test]
    fn unbounded_entry_takes_entry_guard() {
        // halve loop fed by a param-shaped unbounded i64: the sitofp
        // can't prove |x| ≤ 2^53 statically, so the region takes a
        // two-sided entry guard into the preserved f64 loop.
        let mut f = func(
            Type::Void,
            vec![
                Type::I64,  // %0 param
                Type::F64,  // %1 sitofp
                Type::F64,  // %2 n cell
                Type::F64,  // %3 frem
                Type::Bool, // %4 fcmp
                Type::F64,  // %5 fdiv
            ],
            vec![
                block(0, vec![], Terminator::Br(BlockId(1))),
                block(
                    1,
                    vec![
                        inst(1, InstKind::SiToFp(v(0))),
                        inst(2, InstKind::Copy(Type::F64, v(1))),
                    ],
                    Terminator::Br(BlockId(2)),
                ),
                block(
                    2,
                    vec![
                        inst(3, InstKind::BinOp(BinOp::FRem, v(2), cf(2.0))),
                        inst(4, InstKind::FCmp(FPred::Oeq, v(3), cf(0.0))),
                    ],
                    Terminator::CondBr {
                        cond: v(4),
                        then_blk: BlockId(3),
                        else_blk: BlockId(4),
                    },
                ),
                block(
                    3,
                    vec![
                        inst(5, InstKind::BinOp(BinOp::FDiv, v(2), cf(2.0))),
                        inst(2, InstKind::Copy(Type::F64, v(5))),
                    ],
                    Terminator::Br(BlockId(2)),
                ),
                block(4, vec![], Terminator::Ret(None)),
            ],
        );
        f.params = vec![ValueId(0)];
        let (m, stats) = run(f);
        assert_eq!(stats.values_demoted, 4);
        assert_eq!(stats.loops_versioned, 1);
        assert_eq!(stats.guards_inserted, 0);
        assert_eq!(stats.entry_guards, 2);
        // bb0 re-routes through the check chain, not into bb1
        let f0 = &m.funcs[0];
        let Terminator::Br(t) = f0.blocks[0].term else {
            panic!("entry must stay a Br");
        };
        assert_ne!(t, BlockId(1), "entry edge re-routed through guard");
        // the chain checks both sides of ±2^53
        let bound = 1i64 << 53;
        let has_upper = f0.blocks.iter().any(|b| {
            b.insts.iter().any(
                |i| matches!(i.kind, InstKind::ICmp(IPred::Sgt, _, Operand::ConstI64(k)) if k == bound),
            )
        });
        let has_lower = f0.blocks.iter().any(|b| {
            b.insts.iter().any(
                |i| matches!(i.kind, InstKind::ICmp(IPred::Slt, _, Operand::ConstI64(k)) if k == -bound),
            )
        });
        assert!(has_upper && has_lower, "two-sided entry guard");
    }
}
