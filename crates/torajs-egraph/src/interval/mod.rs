//! Integer range analysis — function-local interval lattice with
//! branch refinement and loop widening/narrowing (Cousot & Cousot
//! abstract interpretation; the lightweight analogue of LLVM
//! LVI/SCCP and the type/range half of TurboFan's representation
//! selection). Analysis only: no IR is mutated here.
//!
//! Clients (L3a item 10, RFC 20260611-int-range-float-demotion):
//!
//! * `sext_elide` R2 facts — any i64 value provably inside i32 range
//!   equals its own low-32 sign extension, which lets loop-carried
//!   cells (multi-def Copy, conservatively unknown to sext_elide's
//!   own one-pass lattice) qualify and collapses the residual
//!   popcount sext pair.
//! * `float_demote` (phase 1b) — integer-valuedness + bounds of f64
//!   hot-loop SCCs decide demotion and guard placement.
//!
//! Flow-insensitive per-value facts plus dominating-edge constraints:
//! a loop cell's fact is the join over its def sites, each evaluated
//! in its block's refined context; widening kicks in after a few
//! Kleene rounds so unbounded trajectories (collatz `3n+1`) settle at
//! a sentinel bound while keeping integer-valuedness, and a narrowing
//! sweep re-tightens bounds the refinement can recover (the
//! `i <= 1e6` loop-counter shape).

mod elem;
pub mod refine;
pub mod transfer;

pub use transfer::{AbsVal, NumFact};

use std::collections::{HashMap, HashSet};

use torajs_core::ssa::{Function, InstKind, Module, Operand, Type, ValueId};

use crate::dominator::DominatorTree;
use refine::{Constraint, Constraints};
use transfer::eval_inst;

/// Kleene rounds before unstable cells widen to sentinel bounds.
const WIDEN_ROUND: usize = 4;
/// Hard cap on ascending rounds; anything still moving loses its
/// bounds. Sized so threshold widening (one ladder step per round)
/// can climb a realistic icmp-constant ladder before the cap.
const MAX_ASCEND: usize = 32;
/// Narrowing sweeps after the ascend stabilizes.
const NARROW_ROUNDS: usize = 2;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct IntervalStats {
    /// Values with a finite (non-sentinel, non-full-i64) fact.
    pub bounded: u32,
    /// i64 values provably inside i32 range (sext_elide seeds).
    pub sext32: u32,
    /// f64-typed values proven integer-valued.
    pub f64_int_valued: u32,
}

/// Analyze every function body. Index-aligned with `module.funcs`.
pub fn analyze_module(module: &Module) -> Vec<HashMap<ValueId, NumFact>> {
    let names: Vec<String> = module.funcs.iter().map(|f| f.name.clone()).collect();
    module
        .funcs
        .iter()
        .map(|f| {
            if f.is_declaration() {
                HashMap::new()
            } else {
                analyze_function_with(f, &names)
            }
        })
        .collect()
}

/// The i64-typed values whose fact proves them sign-extended-from-32.
pub fn sext32_set(facts: &HashMap<ValueId, NumFact>, func: &Function) -> HashSet<ValueId> {
    facts
        .iter()
        .filter(|(v, f)| func.values[v.0 as usize].ty == Type::I64 && f.is_sext32())
        .map(|(v, _)| *v)
        .collect()
}

pub fn stats_for(facts: &HashMap<ValueId, NumFact>, func: &Function) -> IntervalStats {
    let mut s = IntervalStats::default();
    for (v, f) in facts {
        let ty = func.values[v.0 as usize].ty;
        if f != &NumFact::full_i64() && f.lo > transfer::NEG_INF && f.hi < transfer::POS_INF {
            s.bounded += 1;
        }
        if ty == Type::I64 && f.is_sext32() {
            s.sext32 += 1;
        }
        if ty == Type::F64 {
            s.f64_int_valued += 1;
        }
    }
    s
}

/// Analyze one function with no way to name its callees, so no array
/// element point is tracked — the facts are the same ones this pass
/// gave before [`elem`] existed, and still sound.
pub fn analyze_function(func: &Function) -> HashMap<ValueId, NumFact> {
    analyze_function_with(func, &[])
}

/// `callee_names` is index-aligned with `Module::funcs`; it is what
/// lets [`elem`] recognize the array runtime by name.
pub fn analyze_function_with(
    func: &Function,
    callee_names: &[String],
) -> HashMap<ValueId, NumFact> {
    // def shapes: single-def map for constraint pattern-matching,
    // cell def-site sets for constraint kills.
    let mut single: HashMap<ValueId, InstKind> = HashMap::new();
    let mut cells: HashSet<ValueId> = HashSet::new();
    for block in &func.blocks {
        for inst in &block.insts {
            if let Some(r) = inst.result {
                if single.insert(r, inst.kind.clone()).is_some() {
                    cells.insert(r);
                }
            }
        }
    }
    for v in &cells {
        single.remove(v);
    }
    let mut block_defs: Vec<HashSet<ValueId>> = vec![HashSet::new(); func.blocks.len()];
    for (bi, block) in func.blocks.iter().enumerate() {
        for inst in &block.insts {
            if let Some(r) = inst.result {
                if cells.contains(&r) {
                    block_defs[bi].insert(r);
                }
            }
        }
    }

    let dom = DominatorTree::compute(func);
    let constraints = Constraints::compute(func, &single);
    let thresholds = collect_thresholds(func);
    // An allocation's elements are one more multi-def cell whose defs
    // happen to be stores; it rides the same ascent below.
    let elems = elem::collect(func, callee_names);
    let mut elem_state: HashMap<ValueId, AbsVal> = HashMap::new();

    let mut state: HashMap<ValueId, AbsVal> = HashMap::new();
    for p in &func.params {
        state.insert(*p, opaque_for(func.values[p.0 as usize].ty));
    }

    let mut stable_since = None;
    for round in 0.. {
        let narrowing = stable_since.is_some();
        let mut contrib: HashMap<ValueId, AbsVal> = HashMap::new();
        let mut changed = false;

        for &bid in dom.reverse_postorder() {
            let block = &func.blocks[bid.0 as usize];
            let mut live = constraints.inherited(bid, &dom, &block_defs);
            for inst in &block.insts {
                let Some(r) = inst.result else { continue };
                let ty = func.values[r.0 as usize].ty;
                let ev = {
                    let env = |op: &Operand| lookup(op, &state, &live);
                    let is_even = |op: &Operand| {
                        matches!(op, Operand::Value(v)
                            if live.iter().any(|c| matches!(c, Constraint::Even(e) if e == v)))
                    };
                    eval_inst(&inst.kind, ty, &env, &is_even)
                };
                let ev = if ev == AbsVal::Bottom
                    && !matches!(
                        inst.kind,
                        InstKind::Copy(..)
                            | InstKind::Identity(_)
                            | InstKind::BinOp(..)
                            | InstKind::SiToFp(_)
                            | InstKind::FpToSi(_)
                    ) {
                    opaque_for(ty)
                } else {
                    ev
                };
                // The load of a tracked element answers with the
                // element point rather than the opaque default. Only
                // for numeric results — a pointer element carries no
                // interval and no consumer wants one.
                let ev = match elems.reads.get(&r) {
                    Some(root) if matches!(ty, Type::I64 | Type::F64) => {
                        elem_state.get(root).copied().unwrap_or(AbsVal::Bottom)
                    }
                    _ => ev,
                };
                if cells.contains(&r) {
                    let e = contrib.entry(r).or_insert(AbsVal::Bottom);
                    *e = e.join(ev);
                    // the cell's contents changed: refined facts about
                    // its previous contents no longer apply below.
                    live.retain(|c| c.value() != r);
                } else if state.get(&r) != Some(&ev) {
                    state.insert(r, ev);
                    changed = true;
                }
            }
        }

        let mut moved_cells: Vec<ValueId> = Vec::new();
        let mut moved_elems: Vec<ValueId> = Vec::new();
        for (root, ws) in &elems.writes {
            let new = ws
                .iter()
                .fold(AbsVal::Bottom, |acc, op| acc.join(lookup(op, &state, &[])));
            let old = elem_state.get(root).copied().unwrap_or(AbsVal::Bottom);
            let next = if narrowing {
                narrow(old, new)
            } else {
                widen(old, old.join(new), round, &thresholds)
            };
            if next != old {
                elem_state.insert(*root, next);
                changed = true;
                moved_elems.push(*root);
            }
        }
        for (&cell, &new) in &contrib {
            let old = state.get(&cell).copied().unwrap_or(AbsVal::Bottom);
            let next = if narrowing {
                narrow(old, new)
            } else {
                widen(old, old.join(new), round, &thresholds)
            };
            if next != old {
                state.insert(cell, next);
                changed = true;
                moved_cells.push(cell);
            }
        }

        match (narrowing, changed, round) {
            (false, false, _) | (false, _, MAX_ASCEND..) => {
                // ascend done (stable or capped) → start narrowing.
                // capped: cells still moving lose their bounds (the
                // join stays sound; int-valuedness survives).
                for cell in moved_cells {
                    if let Some(AbsVal::Fact(f)) = state.get_mut(&cell) {
                        *f = NumFact::new(transfer::NEG_INF, transfer::POS_INF);
                    }
                }
                for root in moved_elems {
                    if let Some(AbsVal::Fact(f)) = elem_state.get_mut(&root) {
                        *f = NumFact::new(transfer::NEG_INF, transfer::POS_INF);
                    }
                }
                stable_since = Some(round);
            }
            (true, _, _) if round >= stable_since.unwrap() + NARROW_ROUNDS => break,
            _ => {}
        }
    }

    state
        .into_iter()
        .filter_map(|(v, av)| match av {
            AbsVal::Fact(f) => Some((v, f)),
            _ => None,
        })
        .collect()
}

fn opaque_for(ty: Type) -> AbsVal {
    match ty {
        Type::I64 => AbsVal::Fact(NumFact::full_i64()),
        _ => AbsVal::Top,
    }
}

fn lookup(op: &Operand, state: &HashMap<ValueId, AbsVal>, live: &[Constraint]) -> AbsVal {
    if let Some(k) = transfer::op_const_int(op) {
        return AbsVal::Fact(NumFact::point(k));
    }
    let Operand::Value(v) = op else {
        return AbsVal::Top;
    };
    let base = state.get(v).copied().unwrap_or(AbsVal::Bottom);
    let AbsVal::Fact(mut f) = base else {
        return base;
    };
    for c in live {
        match c {
            Constraint::Range(cv, r) if cv == v => {
                if let Some(m) = f.meet(*r) {
                    f = m;
                }
            }
            // W3 C3 — nonzero on this path: trim a zero endpoint.
            // A range with zero strictly inside keeps its bounds
            // (the exclusion is not interval-expressible — sound).
            Constraint::NonZero(cv) if cv == v => {
                if f.lo == 0 && f.hi > 0 {
                    f = NumFact::new(1, f.hi);
                } else if f.hi == 0 && f.lo < 0 {
                    f = NumFact::new(f.lo, -1);
                }
            }
            _ => {}
        }
    }
    AbsVal::Fact(f)
}

/// Widening thresholds — every integer constant compared against in
/// the function (±1 for strict/inclusive edges). Standard "widening
/// with thresholds" (Astrée/Miné): a growing bound first jumps to the
/// nearest threshold, giving branch refinement a chance to pin loop
/// counters (`i < 1e7` settles at `[0, 9999999]`) before chained
/// cells (`n = i; n = n & (n-1)`) would otherwise widen to a sentinel
/// the narrowing sweep cannot recover through self-referential joins.
fn collect_thresholds(func: &Function) -> Vec<i128> {
    let mut t: Vec<i128> = Vec::new();
    for block in &func.blocks {
        for inst in &block.insts {
            if let InstKind::ICmp(_, a, b) | InstKind::FCmp(_, a, b) = &inst.kind {
                for op in [a, b] {
                    if let Some(k) = transfer::op_const_int(op) {
                        t.extend([k - 1, k, k + 1]);
                    }
                }
            }
        }
    }
    t.sort_unstable();
    t.dedup();
    t
}

/// Ascend step: monotone join, with threshold widening once the round
/// budget is spent — a bound still growing jumps to the next
/// threshold, then to a sentinel.
fn widen(old: AbsVal, joined: AbsVal, round: usize, thresholds: &[i128]) -> AbsVal {
    if round < WIDEN_ROUND {
        return joined;
    }
    let (AbsVal::Fact(o), AbsVal::Fact(j)) = (old, joined) else {
        return joined;
    };
    let hi = if j.hi > o.hi {
        thresholds
            .iter()
            .copied()
            .find(|&t| t >= j.hi)
            .unwrap_or(transfer::POS_INF)
    } else {
        j.hi
    };
    let lo = if j.lo < o.lo {
        thresholds
            .iter()
            .rev()
            .copied()
            .find(|&t| t <= j.lo)
            .unwrap_or(transfer::NEG_INF)
    } else {
        j.lo
    };
    AbsVal::Fact(NumFact::new(lo, hi))
}

/// Narrowing step: accept a recomputed fact only when it tightens.
fn narrow(old: AbsVal, new: AbsVal) -> AbsVal {
    match (old, new) {
        (AbsVal::Top, AbsVal::Fact(_)) => new,
        (AbsVal::Fact(o), AbsVal::Fact(n)) if o.contains(n) => new,
        _ => old,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{BinOp, Block, BlockId, FPred, IPred, Inst, Terminator, ValueInfo};

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

    fn func(values: Vec<Type>, blocks: Vec<Block>) -> Function {
        Function {
            name: "f".into(),
            params: vec![],
            ret: Type::Void,
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

    #[test]
    fn popcount_counter_cell_proves_sext32() {
        // bb0: %0 = copy 0            ; i counter cell
        // bb1: %1 = icmp slt %0, 1e7 ; cond_br %1, bb2, bb4
        // bb2: %2 = copy %0           ; n cell init
        //      %3 = sub %2, 1
        //      %4 = and %2, %3
        //      %2 = copy %4           ; n cell loop def
        //      %5 = add %0, 1
        //      %0 = copy %5           ; counter loop def
        //      br bb1
        // bb4: ret
        let f = func(
            vec![Type::I64; 6],
            vec![
                block(
                    0,
                    vec![inst(0, InstKind::Copy(Type::I64, c(0)))],
                    Terminator::Br(BlockId(1)),
                ),
                block(
                    1,
                    vec![inst(1, InstKind::ICmp(IPred::Slt, v(0), c(10_000_000)))],
                    Terminator::CondBr {
                        cond: v(1),
                        then_blk: BlockId(2),
                        else_blk: BlockId(3),
                    },
                ),
                block(
                    2,
                    vec![
                        inst(2, InstKind::Copy(Type::I64, v(0))),
                        inst(3, InstKind::BinOp(BinOp::Sub, v(2), c(1))),
                        inst(4, InstKind::BinOp(BinOp::And, v(2), v(3))),
                        inst(2, InstKind::Copy(Type::I64, v(4))),
                        inst(5, InstKind::BinOp(BinOp::Add, v(0), c(1))),
                        inst(0, InstKind::Copy(Type::I64, v(5))),
                    ],
                    Terminator::Br(BlockId(1)),
                ),
                block(3, vec![], Terminator::Ret(None)),
            ],
        );
        let facts = analyze_function(&f);
        // counter cell: [0, 1e7] (refined increment joins the init)
        assert_eq!(facts[&ValueId(0)], NumFact::new(0, 10_000_000));
        // n cell and the and-result stay inside i32 → sext_elide seeds
        let seeds = sext32_set(&facts, &f);
        assert!(seeds.contains(&ValueId(2)), "n cell: {facts:?}");
        assert!(seeds.contains(&ValueId(4)), "and result: {facts:?}");
    }

    #[test]
    fn collatz_f64_cell_stays_integer_valued() {
        // bb0: %0 = copy 1                 ; i cell (i64)
        // bb1: %1 = icmp sle %0, 1000000  ; cond_br bb2, bb8
        // bb2: %2 = sitofp %0
        //      %3 = copy f64 %2            ; n cell init
        //      br bb3
        // bb3: %4 = frem %3, 2
        //      %5 = fcmp oeq %4, 0 ; cond_br bb4, bb5
        // bb4: %6 = fdiv %3, 2 ; %3 = copy %6 ; br bb6
        // bb5: %7 = fmul 3, %3 ; %8 = fadd %7, 1 ; %3 = copy %8 ; br bb6
        // bb6: br bb3   (loop back; exit path elided — analysis only)
        let two = Operand::ConstF64(2.0);
        let f = func(
            vec![
                Type::I64,
                Type::Bool,
                Type::F64,
                Type::F64,
                Type::F64,
                Type::Bool,
                Type::F64,
                Type::F64,
                Type::F64,
            ],
            vec![
                block(
                    0,
                    vec![inst(0, InstKind::Copy(Type::I64, c(1)))],
                    Terminator::Br(BlockId(1)),
                ),
                block(
                    1,
                    vec![inst(1, InstKind::ICmp(IPred::Sle, v(0), c(1_000_000)))],
                    Terminator::CondBr {
                        cond: v(1),
                        then_blk: BlockId(2),
                        else_blk: BlockId(7),
                    },
                ),
                block(
                    2,
                    vec![
                        inst(2, InstKind::SiToFp(v(0))),
                        inst(3, InstKind::Copy(Type::F64, v(2))),
                    ],
                    Terminator::Br(BlockId(3)),
                ),
                block(
                    3,
                    vec![
                        inst(4, InstKind::BinOp(BinOp::FRem, v(3), two)),
                        inst(5, InstKind::FCmp(FPred::Oeq, v(4), Operand::ConstF64(0.0))),
                    ],
                    Terminator::CondBr {
                        cond: v(5),
                        then_blk: BlockId(4),
                        else_blk: BlockId(5),
                    },
                ),
                block(
                    4,
                    vec![
                        inst(6, InstKind::BinOp(BinOp::FDiv, v(3), two)),
                        inst(3, InstKind::Copy(Type::F64, v(6))),
                    ],
                    Terminator::Br(BlockId(6)),
                ),
                block(
                    5,
                    vec![
                        inst(
                            7,
                            InstKind::BinOp(BinOp::FMul, Operand::ConstF64(3.0), v(3)),
                        ),
                        inst(
                            8,
                            InstKind::BinOp(BinOp::FAdd, v(7), Operand::ConstF64(1.0)),
                        ),
                        inst(3, InstKind::Copy(Type::F64, v(8))),
                    ],
                    Terminator::Br(BlockId(6)),
                ),
                block(6, vec![], Terminator::Br(BlockId(3))),
                block(7, vec![], Terminator::Ret(None)),
            ],
        );
        let facts = analyze_function(&f);
        // the n cell is integer-valued (has a fact at all) even though
        // its upper bound widens to the sentinel — the 1b demotion
        // contract: int-ness provable, bound guarded at runtime.
        let n = facts[&ValueId(3)];
        assert!(n.lo >= 0, "n cell fact: {n:?}");
        assert!(n.hi >= transfer::POS_INF, "3n+1 must widen: {n:?}");
        // the fdiv result is integer-valued thanks to the parity guard
        assert!(
            facts.contains_key(&ValueId(6)),
            "fdiv even-proof: {facts:?}"
        );
        // the frem result is bounded
        assert_eq!(facts[&ValueId(4)], NumFact::new(0, 1));
    }

    #[test]
    fn fdiv_without_parity_guard_is_unknown() {
        // straight-line fdiv by 2 with no frem branch — must stay Top.
        let f = func(
            vec![Type::I64, Type::F64, Type::F64],
            vec![block(
                0,
                vec![
                    inst(0, InstKind::Copy(Type::I64, c(5))),
                    inst(1, InstKind::SiToFp(v(0))),
                    inst(
                        2,
                        InstKind::BinOp(BinOp::FDiv, v(1), Operand::ConstF64(2.0)),
                    ),
                ],
                Terminator::Ret(None),
            )],
        );
        let facts = analyze_function(&f);
        assert!(!facts.contains_key(&ValueId(2)));
        assert_eq!(facts[&ValueId(1)], NumFact::point(5));
    }

    #[test]
    fn refinement_requires_sole_predecessor() {
        // bb1 is reached both from the branch and from bb2 — the
        // icmp constraint must NOT attach.
        let f = func(
            vec![Type::I64, Type::Bool],
            vec![
                block(
                    0,
                    vec![
                        inst(0, InstKind::Copy(Type::I64, c(0))),
                        inst(1, InstKind::ICmp(IPred::Slt, v(0), c(10))),
                    ],
                    Terminator::CondBr {
                        cond: v(1),
                        then_blk: BlockId(1),
                        else_blk: BlockId(2),
                    },
                ),
                block(1, vec![], Terminator::Ret(None)),
                block(2, vec![], Terminator::Br(BlockId(1))),
            ],
        );
        // would panic on inconsistency if constraints attached wrongly;
        // the cell-free function must still produce the plain fact.
        let facts = analyze_function(&f);
        assert_eq!(facts[&ValueId(0)], NumFact::point(0));
    }
}
