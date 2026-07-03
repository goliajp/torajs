//! W3 C2 (rfc 20260611-ann-width-unification §5.3) — frem truncation
//! recovery over the post-φ canonical shape. C1 lowers every `%` with
//! a non-provably-non-negative dividend through the float path for -0
//! correctness, which turns previously-int comparison loops
//! (`n % i === 0`) into sitofp + frem + fcmp. Two -0-insensitive use
//! shapes narrow back to the srem form C1 retired:
//!
//! * **Compare (R1)** — `fcmp(P, frem(x̂, ŷ), ĉ)` where x̂ / ŷ each
//!   resolve to `sitofp` of an int value or an integral constant, the
//!   frem result's only use is this fcmp, and ĉ is an integral
//!   constant → `icmp(P', srem(x, y), c)`. frem over integral
//!   operands is exact (no rounding: |result| < |divisor|), and ±0
//!   compare equal under every fcmp predicate, so the -0 that srem
//!   cannot represent is unobservable here.
//! * **Truncate (R2, tree-shaped since W3 chunk ②)** — `fptosi` over a
//!   whole fadd / fsub / fmul / frem expression tree whose leaves all
//!   peel to int form: the i64 sink already collapses -0 to 0, and
//!   interior ±0 / rounding-past-2^53 differences are wrap-parity
//!   with the pre-W3 int path (known debt, unchanged), so the float
//!   detour computes the same i64. Every node retypes to its int op
//!   in place (`fptosi(fadd(fmul(x̂,ŷ), fmul(ẑ,ŵ)))` → the add/mul
//!   tree in i64) and the fptosi collapses to a Copy. This is the AOT
//!   shape of V8's Truncation::Word64 propagating through kNumberAdd /
//!   kNumberMultiply. The single-op shapes (frem / fmul / S8's
//!   -0.0-LHS fsub for `-x` on a runtime int — the -0.0 constant
//!   peels to 0 via `int_form`) are the depth-1 trees. fadd / fsub /
//!   fmul qualify only under this sink — an fcmp would observe the
//!   rounding the int form wraps (W3 C4).
//!
//! An SRem recovery requires a PROVABLY non-zero divisor — a non-zero
//! integral constant (srem runtime-0, ann-width §5.3 follow-up close).
//! A variable divisor that is 0 at runtime must keep the frem: frem →
//! NaN is the JS `a % 0` answer, while aarch64 sdiv-by-zero hands the
//! dividend back (`(7 % b) | 0` printed 7 where bun prints NaN|0 = 0).
//!
//! Runs after phi_promote, before the interval analysis — the
//! recovered int loops feed interval facts and sext_elide seeds as if
//! C1 had never floated them. `TORAJS_FREM_NARROW_OFF=1` skips,
//! `TORAJS_FREM_NARROW_STATS=1` dumps counters.

use std::collections::{HashMap, HashSet};

use torajs_core::ssa::{BinOp, Function, InstKind, Module, Operand, Terminator, Type, ValueId};

use crate::float_demote::fit::fpred_to_ipred;
use crate::interval::transfer::op_const_int;
use crate::rc_peephole::visit_value_operands;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct FremNarrowStats {
    /// fcmp-over-frem chains narrowed to icmp-over-srem.
    pub cmps_narrowed: u32,
    /// fptosi sinks whose float-op tree collapsed to int form
    /// (counts the root; interior nodes land in
    /// `tree_interior_narrowed`).
    pub truncs_narrowed: u32,
    /// Interior float-op nodes retyped as part of a trunc tree
    /// (0 for the single-op shapes).
    pub tree_interior_narrowed: u32,
    /// Orphaned sitofp insts swept after the rewrites.
    pub dead_converts_removed: u32,
    /// Narrowed compares whose variable divisor needed the fused
    /// zero guard (subset of `cmps_narrowed`).
    pub cmps_guarded: u32,
}

pub fn narrow_frems(module: &mut Module) -> FremNarrowStats {
    let mut stats = FremNarrowStats::default();
    for func in module.funcs.iter_mut() {
        if func.is_declaration() {
            continue;
        }
        narrow_in_function(func, &mut stats);
    }
    stats
}

/// A frem operand in int form: `sitofp x` peels to `x`, an integral
/// constant becomes ConstI64. `None` = not provably integral.
fn int_form(op: &Operand, defs: &HashMap<ValueId, InstKind>) -> Option<Operand> {
    if let Operand::Value(v) = op {
        if let Some(InstKind::SiToFp(inner)) = defs.get(v) {
            return Some(*inner);
        }
        return None;
    }
    op_const_int(op).map(|c| Operand::ConstI64(c as i64))
}

/// Truncation propagation through a float-op tree (W3 chunk ② —
/// the AOT shape of V8's Truncation::Word64 flowing through
/// kNumberAdd / kNumberSubtract / kNumberMultiply): an entire
/// fadd / fsub / fmul / frem expression tree rooted at an fptosi
/// sink retypes to int ops when every leaf peels to int form.
/// The i64 sink collapses ±0, and interior ±0 / rounding-past-2^53
/// differences are wrap-parity with the pre-W3 int path (W4 /
/// overflow known debt, unchanged). Every interior node must be
/// single-def single-use (its only consumer is its tree parent or
/// the sink); a leaf `sitofp` may stay multi-use — the peel doesn't
/// disturb it and DCE only sweeps orphans. An frem divisor that is
/// a compile-time zero rejects the whole tree (frem → NaN is the JS
/// `a % 0` answer; srem would be UB).
///
/// On success `out` holds the int rewrite for every tree node;
/// interior edges keep their `Operand::Value` identity (the nodes
/// retype in place, so references stay valid).
fn collect_trunc_tree(
    v: &ValueId,
    defs: &HashMap<ValueId, InstKind>,
    multi: &HashSet<ValueId>,
    uses: &HashMap<ValueId, u32>,
    out: &mut HashMap<ValueId, (BinOp, Operand, Operand)>,
) -> bool {
    if multi.contains(v) || uses.get(v).copied().unwrap_or(0) != 1 {
        return false;
    }
    let Some(InstKind::BinOp(fop @ (BinOp::FAdd | BinOp::FSub | BinOp::FMul | BinOp::FRem), a, b)) =
        defs.get(v)
    else {
        return false;
    };
    let iop = match fop {
        BinOp::FAdd => BinOp::Add,
        BinOp::FSub => BinOp::Sub,
        BinOp::FMul => BinOp::Mul,
        BinOp::FRem => BinOp::SRem,
        _ => unreachable!(),
    };
    let resolve =
        |op: &Operand, out: &mut HashMap<ValueId, (BinOp, Operand, Operand)>| -> Option<Operand> {
            if let Some(leaf) = int_form(op, defs) {
                return Some(leaf);
            }
            if let Operand::Value(c) = op
                && collect_trunc_tree(c, defs, multi, uses, out)
            {
                return Some(*op);
            }
            None
        };
    let Some(ai) = resolve(a, out) else {
        return false;
    };
    let Some(bi) = resolve(b, out) else {
        return false;
    };
    // srem runtime-0 (ann-width §5.3 follow-up close) — an SRem
    // recovery needs a PROVABLY non-zero divisor, not just "not the
    // constant zero": a variable divisor that is 0 at runtime must
    // keep the frem (NaN per JS `a % 0`; aarch64 sdiv-by-zero hands
    // the dividend back — `(7 % b) | 0` printed 7 where bun prints
    // NaN|0 = 0).
    if iop == BinOp::SRem && !matches!(bi, Operand::ConstI64(d) if d != 0) {
        return false;
    }
    out.insert(*v, (iop, ai, bi));
    true
}

fn narrow_in_function(func: &mut Function, stats: &mut FremNarrowStats) {
    let (defs, multi) = collect_defs(func);

    // value → use count, across every inst operand and terminator.
    let mut uses: HashMap<ValueId, u32> = HashMap::new();
    visit_func_uses(func, |v| {
        *uses.entry(v).or_default() += 1;
    });

    let plan = plan_narrows(func, &defs, &multi, &uses, stats);
    if plan.narrow.is_empty() {
        return;
    }
    rewrite_blocks(func, &plan);
    sweep_orphan_sitofps(func, stats);
}

/// Def summaries for single-def values; multi-def values (post-φ
/// Copy cells) are conservatively unknown.
fn collect_defs(func: &Function) -> (HashMap<ValueId, InstKind>, HashSet<ValueId>) {
    let mut defs: HashMap<ValueId, InstKind> = HashMap::new();
    let mut multi: HashSet<ValueId> = HashSet::new();
    for block in &func.blocks {
        for inst in &block.insts {
            if let Some(r) = inst.result {
                if defs.insert(r, inst.kind.clone()).is_some() {
                    multi.insert(r);
                }
            }
        }
    }
    for v in &multi {
        defs.remove(v);
    }
    (defs, multi)
}

/// Every value use in the function — inst operands plus Ret /
/// CondBr terminator operands (shared by the use count and the
/// DCE liveness sweep).
fn visit_func_uses(func: &Function, mut f: impl FnMut(ValueId)) {
    for block in &func.blocks {
        for inst in &block.insts {
            visit_value_operands(&inst.kind, &mut f);
        }
        match &block.term {
            Terminator::Ret(Some(Operand::Value(v))) => f(*v),
            Terminator::CondBr {
                cond: Operand::Value(v),
                ..
            } => f(*v),
            _ => {}
        }
    }
}

/// Compare sink — a single-use single-def frem whose operands peel
/// to int form narrows under an fcmp (±0 compare equal, frem over
/// integral operands is exact). fadd / fsub / fmul stay out of the
/// fcmp shape: an fcmp would observe the f64 rounding past 2^53
/// that the int form wraps instead.
///
/// srem runtime-0 — a VARIABLE divisor narrows too, but with a
/// fused zero guard (`Some(divisor)` in the result): frem(a, 0)
/// is NaN and every ordered predicate answers false (Une: true),
/// while srem-by-zero on aarch64 hands the dividend back — `0 % 0
/// === 0` would wrongly take the branch. The guard is two ALU
/// insts (icmp + and/or) against the multi-cycle frem the narrow
/// saves: `n % i === 0` trial-division loops (prime_count
/// +2640% under the reject-only fix) keep the int path. A
/// compile-time zero never narrows (constant NaN shape).
fn narrow_cmp(
    v: &ValueId,
    defs: &HashMap<ValueId, InstKind>,
    multi: &HashSet<ValueId>,
    uses: &HashMap<ValueId, u32>,
) -> Option<(BinOp, Operand, Operand, Option<Operand>)> {
    if multi.contains(v) || uses.get(v).copied().unwrap_or(0) != 1 {
        return None;
    }
    let InstKind::BinOp(BinOp::FRem, a, b) = defs.get(v)? else {
        return None;
    };
    let ai = int_form(a, defs)?;
    let bi = int_form(b, defs)?;
    if matches!(bi, Operand::ConstI64(0)) {
        return None;
    }
    let guard = if matches!(bi, Operand::ConstI64(_)) {
        None
    } else {
        Some(bi)
    };
    Some((BinOp::SRem, ai, bi, guard))
}

/// Decision-pass output: frem results to retype, keyed by the
/// consuming inst shape.
struct NarrowPlan {
    narrow: HashMap<ValueId, (BinOp, Operand, Operand)>,
    cmp_results: HashMap<ValueId, Option<Operand>>,
    trunc_results: HashSet<ValueId>,
}

fn plan_narrows(
    func: &Function,
    defs: &HashMap<ValueId, InstKind>,
    multi: &HashSet<ValueId>,
    uses: &HashMap<ValueId, u32>,
    stats: &mut FremNarrowStats,
) -> NarrowPlan {
    let mut narrow: HashMap<ValueId, (BinOp, Operand, Operand)> = HashMap::new();
    let mut cmp_results: HashMap<ValueId, Option<Operand>> = HashMap::new();
    let mut trunc_results: HashSet<ValueId> = HashSet::new();
    for block in &func.blocks {
        for inst in &block.insts {
            match &inst.kind {
                InstKind::FCmp(_, a, b) => {
                    // exactly one side a narrowable frem value, the
                    // other an integral constant.
                    let (v, other) = match (a, b) {
                        (Operand::Value(v), o) if !matches!(o, Operand::Value(_)) => (v, o),
                        (o, Operand::Value(v)) if !matches!(o, Operand::Value(_)) => (v, o),
                        _ => continue,
                    };
                    if op_const_int(other).is_none() {
                        continue;
                    }
                    if let Some((iop, ai, bi, guard)) = narrow_cmp(v, defs, multi, uses) {
                        narrow.insert(*v, (iop, ai, bi));
                        if guard.is_some() {
                            stats.cmps_guarded += 1;
                        }
                        cmp_results.insert(*v, guard);
                        stats.cmps_narrowed += 1;
                    }
                }
                InstKind::FpToSi(Operand::Value(v)) => {
                    let mut tree: HashMap<ValueId, (BinOp, Operand, Operand)> = HashMap::new();
                    if collect_trunc_tree(v, defs, multi, uses, &mut tree) {
                        stats.tree_interior_narrowed += (tree.len() as u32) - 1;
                        narrow.extend(tree);
                        trunc_results.insert(*v);
                        stats.truncs_narrowed += 1;
                    }
                }
                _ => {}
            }
        }
    }
    NarrowPlan {
        narrow,
        cmp_results,
        trunc_results,
    }
}

/// Mutation pass — retype each chosen frem to srem in place, then
/// rewrite its single consumer. Guarded compares (variable
/// divisor) splice two insts ahead of the consumer, so blocks
/// rebuild instead of mutating in place.
fn rewrite_blocks(func: &mut Function, plan: &NarrowPlan) {
    let Function { blocks, values, .. } = func;
    for block in blocks.iter_mut() {
        let old_insts = std::mem::take(&mut block.insts);
        let mut new_insts = Vec::with_capacity(old_insts.len() + 2);
        for mut inst in old_insts {
            if let Some(r) = inst.result {
                if let Some((iop, xi, yi)) = plan.narrow.get(&r) {
                    if matches!(
                        inst.kind,
                        InstKind::BinOp(
                            BinOp::FRem | BinOp::FMul | BinOp::FSub | BinOp::FAdd,
                            _,
                            _
                        )
                    ) {
                        inst.kind = InstKind::BinOp(*iop, *xi, *yi);
                        values[r.0 as usize].ty = Type::I64;
                        new_insts.push(inst);
                        continue;
                    }
                }
            }
            match &inst.kind {
                InstKind::FCmp(..) => {
                    rewrite_fcmp(&mut inst, &plan.cmp_results, values, &mut new_insts);
                }
                InstKind::FpToSi(Operand::Value(v)) if plan.trunc_results.contains(v) => {
                    inst.kind = InstKind::Copy(Type::I64, Operand::Value(*v));
                }
                _ => {}
            }
            new_insts.push(inst);
        }
        block.insts = new_insts;
    }
}

/// Rewrite an fcmp whose frem side narrowed: a plain icmp for a
/// provably non-zero constant divisor, or the fused divisor zero
/// guard for a variable one (two fresh bool insts spliced ahead of
/// the compare).
fn rewrite_fcmp(
    inst: &mut torajs_core::ssa::Inst,
    cmp_results: &HashMap<ValueId, Option<Operand>>,
    values: &mut Vec<torajs_core::ssa::ValueInfo>,
    new_insts: &mut Vec<torajs_core::ssa::Inst>,
) {
    let InstKind::FCmp(p, a, b) = &inst.kind else {
        return;
    };
    let (p, a, b) = (*p, *a, *b);
    let int_side = |op: &Operand| -> Option<Operand> {
        match op {
            Operand::Value(v) if cmp_results.contains_key(v) => Some(*op),
            Operand::Value(_) => None,
            o => op_const_int(o).map(|c| Operand::ConstI64(c as i64)),
        }
    };
    let guard_of = |op: &Operand| -> Option<Operand> {
        match op {
            Operand::Value(v) => cmp_results.get(v).copied().flatten(),
            _ => None,
        }
    };
    let frem_side = |op: &Operand| matches!(op, Operand::Value(v) if cmp_results.contains_key(v));
    if !(frem_side(&a) || frem_side(&b)) {
        return;
    }
    let (Some(na), Some(nb)) = (int_side(&a), int_side(&b)) else {
        return;
    };
    let ipred = fpred_to_ipred(p);
    match guard_of(&a).or_else(|| guard_of(&b)) {
        // srem runtime-0 — fuse the divisor
        // zero guard: frem(x, 0) is NaN, which
        // answers false under every ordered
        // predicate (Une: true), while the
        // srem hands the dividend back. Two
        // fresh bool values: the divisor
        // check and the relocated compare;
        // the original result id becomes the
        // and/or so consumers stay valid.
        Some(divisor) => {
            let nan_true = matches!(p, torajs_core::ssa::FPred::Une);
            let g = ValueId(values.len() as u32);
            values.push(torajs_core::ssa::ValueInfo {
                ty: Type::Bool,
                name: None,
            });
            let c = ValueId(values.len() as u32);
            values.push(torajs_core::ssa::ValueInfo {
                ty: Type::Bool,
                name: None,
            });
            let gpred = if nan_true {
                torajs_core::ssa::IPred::Eq
            } else {
                torajs_core::ssa::IPred::Ne
            };
            new_insts.push(torajs_core::ssa::Inst {
                result: Some(g),
                kind: InstKind::ICmp(gpred, divisor, Operand::ConstI64(0)),
                origin: inst.origin,
            });
            new_insts.push(torajs_core::ssa::Inst {
                result: Some(c),
                kind: InstKind::ICmp(ipred, na, nb),
                origin: inst.origin,
            });
            let join = if nan_true { BinOp::Or } else { BinOp::And };
            inst.kind = InstKind::BinOp(join, Operand::Value(g), Operand::Value(c));
        }
        None => {
            inst.kind = InstKind::ICmp(ipred, na, nb);
        }
    }
}

/// DCE — sitofp insts orphaned by the operand peel.
fn sweep_orphan_sitofps(func: &mut Function, stats: &mut FremNarrowStats) {
    loop {
        let mut used: HashSet<ValueId> = HashSet::new();
        visit_func_uses(func, |v| {
            used.insert(v);
        });
        let mut removed = 0u32;
        for block in func.blocks.iter_mut() {
            block.insts.retain(|inst| {
                let dead = matches!(
                    (&inst.result, &inst.kind),
                    (Some(r), InstKind::SiToFp(_)) if !used.contains(r)
                );
                if dead {
                    removed += 1;
                }
                !dead
            });
        }
        if removed == 0 {
            break;
        }
        stats.dead_converts_removed += removed;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{Block, BlockId, FPred, IPred, Inst, ValueInfo};

    fn v(id: u32) -> Operand {
        Operand::Value(ValueId(id))
    }

    fn cf(x: f64) -> Operand {
        Operand::ConstF64(x)
    }

    /// One-block function over `n_values` pre-registered f64 values.
    fn func_with(insts: Vec<Inst>, n_values: u32, term: Terminator) -> Function {
        Function {
            name: "t".into(),
            params: vec![],
            ret: Type::I64,
            blocks: vec![Block {
                id: BlockId(0),
                insts,
                term,
            }],
            values: (0..n_values)
                .map(|_| ValueInfo {
                    ty: Type::F64,
                    name: None,
                })
                .collect(),
            current_origin: None,
        }
    }

    fn inst(result: u32, kind: InstKind) -> Inst {
        Inst {
            result: Some(ValueId(result)),
            kind,
            origin: None,
        }
    }

    fn run(f: &mut Function) -> FremNarrowStats {
        let mut stats = FremNarrowStats::default();
        narrow_in_function(f, &mut stats);
        stats
    }

    #[test]
    fn cmp_over_frem_narrows_to_icmp_srem() {
        // %2 = sitofp %0 ; %4 = frem %2, 2.0
        // %5 = fcmp oeq %4, 0.0 ; cond_br %5
        // Constant non-zero divisor — the provable srem case.
        let mut f = func_with(
            vec![
                inst(2, InstKind::SiToFp(v(0))),
                inst(4, InstKind::BinOp(BinOp::FRem, v(2), cf(2.0))),
                inst(5, InstKind::FCmp(FPred::Oeq, v(4), cf(0.0))),
            ],
            6,
            Terminator::CondBr {
                cond: v(5),
                then_blk: BlockId(0),
                else_blk: BlockId(0),
            },
        );
        let stats = run(&mut f);
        assert_eq!(stats.cmps_narrowed, 1);
        assert_eq!(stats.dead_converts_removed, 1);
        let insts = &f.blocks[0].insts;
        assert_eq!(insts.len(), 2);
        assert!(matches!(
            insts[0].kind,
            InstKind::BinOp(
                BinOp::SRem,
                Operand::Value(ValueId(0)),
                Operand::ConstI64(2)
            )
        ));
        assert!(matches!(
            insts[1].kind,
            InstKind::ICmp(IPred::Eq, Operand::Value(ValueId(4)), Operand::ConstI64(0))
        ));
        assert_eq!(f.values[4].ty, Type::I64);
    }

    #[test]
    fn cmp_over_frem_variable_divisor_narrows_with_zero_guard() {
        // srem runtime-0 guard: a VARIABLE divisor can be 0 at
        // runtime (frem → NaN, NaN == 0 is false; srem-by-zero on
        // aarch64 hands the dividend back — `0 % 0 == 0` would
        // wrongly take the branch). The narrow keeps the srem int
        // path (`n % i === 0` trial-division loops) and fuses a
        // divisor zero check: `and(icmp_ne(y, 0), icmp_eq(srem, 0))`.
        let mut f = func_with(
            vec![
                inst(2, InstKind::SiToFp(v(0))),
                inst(3, InstKind::SiToFp(v(1))),
                inst(4, InstKind::BinOp(BinOp::FRem, v(2), v(3))),
                inst(5, InstKind::FCmp(FPred::Oeq, v(4), cf(0.0))),
            ],
            6,
            Terminator::CondBr {
                cond: v(5),
                then_blk: BlockId(0),
                else_blk: BlockId(0),
            },
        );
        let stats = run(&mut f);
        assert_eq!(stats.cmps_narrowed, 1);
        assert_eq!(stats.cmps_guarded, 1);
        let insts = &f.blocks[0].insts;
        // srem retyped in place; two spliced bools; fcmp → and.
        assert!(matches!(
            insts[0].kind,
            InstKind::BinOp(
                BinOp::SRem,
                Operand::Value(ValueId(0)),
                Operand::Value(ValueId(1))
            )
        ));
        assert!(matches!(
            insts[1].kind,
            InstKind::ICmp(IPred::Ne, Operand::Value(ValueId(1)), Operand::ConstI64(0))
        ));
        assert!(matches!(
            insts[2].kind,
            InstKind::ICmp(IPred::Eq, Operand::Value(ValueId(4)), Operand::ConstI64(0))
        ));
        assert!(matches!(
            insts[3].kind,
            InstKind::BinOp(BinOp::And, Operand::Value(_), Operand::Value(_))
        ));
        assert_eq!(f.values[4].ty, Type::I64);
    }

    #[test]
    fn cmp_over_frem_variable_divisor_une_guards_with_or() {
        // Une is the one NaN-true predicate: frem(a, 0) !== c must
        // answer true, so the guard joins with OR over icmp_eq(y, 0).
        let mut f = func_with(
            vec![
                inst(2, InstKind::SiToFp(v(0))),
                inst(3, InstKind::SiToFp(v(1))),
                inst(4, InstKind::BinOp(BinOp::FRem, v(2), v(3))),
                inst(5, InstKind::FCmp(FPred::Une, v(4), cf(0.0))),
            ],
            6,
            Terminator::CondBr {
                cond: v(5),
                then_blk: BlockId(0),
                else_blk: BlockId(0),
            },
        );
        let stats = run(&mut f);
        assert_eq!(stats.cmps_narrowed, 1);
        assert_eq!(stats.cmps_guarded, 1);
        let insts = &f.blocks[0].insts;
        assert!(matches!(
            insts[1].kind,
            InstKind::ICmp(IPred::Eq, Operand::Value(ValueId(1)), Operand::ConstI64(0))
        ));
        assert!(matches!(
            insts[3].kind,
            InstKind::BinOp(BinOp::Or, Operand::Value(_), Operand::Value(_))
        ));
    }

    #[test]
    fn multi_use_frem_stays_float() {
        // frem feeds both the fcmp and a ret — -0 escapes, no narrow.
        let mut f = func_with(
            vec![
                inst(2, InstKind::SiToFp(v(0))),
                inst(3, InstKind::SiToFp(v(1))),
                inst(4, InstKind::BinOp(BinOp::FRem, v(2), v(3))),
                inst(5, InstKind::FCmp(FPred::Oeq, v(4), cf(0.0))),
            ],
            6,
            Terminator::Ret(Some(v(4))),
        );
        let stats = run(&mut f);
        assert_eq!(stats.cmps_narrowed, 0);
        assert!(matches!(
            f.blocks[0].insts[2].kind,
            InstKind::BinOp(BinOp::FRem, _, _)
        ));
    }

    #[test]
    fn const_zero_divisor_stays_float() {
        let mut f = func_with(
            vec![
                inst(1, InstKind::SiToFp(v(0))),
                inst(2, InstKind::BinOp(BinOp::FRem, v(1), cf(0.0))),
                inst(3, InstKind::FCmp(FPred::Oeq, v(2), cf(0.0))),
            ],
            4,
            Terminator::CondBr {
                cond: v(3),
                then_blk: BlockId(0),
                else_blk: BlockId(0),
            },
        );
        let stats = run(&mut f);
        assert_eq!(stats.cmps_narrowed, 0);
    }

    #[test]
    fn fractional_const_compare_stays_float() {
        let mut f = func_with(
            vec![
                inst(2, InstKind::SiToFp(v(0))),
                inst(3, InstKind::SiToFp(v(1))),
                inst(4, InstKind::BinOp(BinOp::FRem, v(2), v(3))),
                inst(5, InstKind::FCmp(FPred::Oeq, v(4), cf(0.5))),
            ],
            6,
            Terminator::CondBr {
                cond: v(5),
                then_blk: BlockId(0),
                else_blk: BlockId(0),
            },
        );
        let stats = run(&mut f);
        assert_eq!(stats.cmps_narrowed, 0);
    }

    #[test]
    fn fptosi_over_fmul_collapses_to_mul() {
        // W3 C4 — `x * -1` floated by lowering, recovered at the i64
        // sink.
        let mut f = func_with(
            vec![
                inst(1, InstKind::SiToFp(v(0))),
                inst(2, InstKind::BinOp(BinOp::FMul, v(1), cf(-1.0))),
                inst(3, InstKind::FpToSi(v(2))),
            ],
            4,
            Terminator::Ret(Some(v(3))),
        );
        let stats = run(&mut f);
        assert_eq!(stats.truncs_narrowed, 1);
        assert!(matches!(
            f.blocks[0].insts[0].kind,
            InstKind::BinOp(
                BinOp::Mul,
                Operand::Value(ValueId(0)),
                Operand::ConstI64(-1)
            )
        ));
    }

    #[test]
    fn fcmp_over_fmul_stays_float() {
        // rounding-observable: fcmp never narrows an fmul.
        let mut f = func_with(
            vec![
                inst(2, InstKind::SiToFp(v(0))),
                inst(3, InstKind::SiToFp(v(1))),
                inst(4, InstKind::BinOp(BinOp::FMul, v(2), v(3))),
                inst(5, InstKind::FCmp(FPred::Oeq, v(4), cf(0.0))),
            ],
            6,
            Terminator::CondBr {
                cond: v(5),
                then_blk: BlockId(0),
                else_blk: BlockId(0),
            },
        );
        let stats = run(&mut f);
        assert_eq!(stats.cmps_narrowed, 0);
    }

    #[test]
    fn fptosi_over_neg_shaped_fsub_collapses_to_sub() {
        // W3 S8 — `-x` on a runtime int floated by lowering as
        // fsub(-0.0, sitofp x), recovered at the i64 sink.
        let mut f = func_with(
            vec![
                inst(1, InstKind::SiToFp(v(0))),
                inst(2, InstKind::BinOp(BinOp::FSub, cf(-0.0), v(1))),
                inst(3, InstKind::FpToSi(v(2))),
            ],
            4,
            Terminator::Ret(Some(v(3))),
        );
        let stats = run(&mut f);
        assert_eq!(stats.truncs_narrowed, 1);
        assert_eq!(stats.dead_converts_removed, 1);
        let insts = &f.blocks[0].insts;
        assert_eq!(insts.len(), 2);
        assert!(matches!(
            insts[0].kind,
            InstKind::BinOp(BinOp::Sub, Operand::ConstI64(0), Operand::Value(ValueId(0)))
        ));
        assert!(matches!(
            insts[1].kind,
            InstKind::Copy(Type::I64, Operand::Value(ValueId(2)))
        ));
        assert_eq!(f.values[2].ty, Type::I64);
    }

    #[test]
    fn fptosi_over_plain_fsub_collapses_to_sub() {
        // chunk ② — an ordinary subtraction with an integral-const
        // LHS narrows under the i64 sink (wrap-parity with the int
        // path), not just S8's -0.0 negation marker.
        let mut f = func_with(
            vec![
                inst(1, InstKind::SiToFp(v(0))),
                inst(2, InstKind::BinOp(BinOp::FSub, cf(0.0), v(1))),
                inst(3, InstKind::FpToSi(v(2))),
            ],
            4,
            Terminator::Ret(Some(v(3))),
        );
        let stats = run(&mut f);
        assert_eq!(stats.truncs_narrowed, 1);
        assert!(matches!(
            f.blocks[0].insts[0].kind,
            InstKind::BinOp(BinOp::Sub, Operand::ConstI64(0), Operand::Value(ValueId(0)))
        ));
    }

    #[test]
    fn fptosi_over_fop_tree_collapses_to_int_tree() {
        // chunk ② — fptosi(fadd(fmul(â,b̂), fmul(ĉ,d̂))) retypes the
        // whole tree: both fmuls → mul, the fadd → add, sink → Copy.
        let mut f = func_with(
            vec![
                inst(4, InstKind::SiToFp(v(0))),
                inst(5, InstKind::SiToFp(v(1))),
                inst(6, InstKind::SiToFp(v(2))),
                inst(7, InstKind::SiToFp(v(3))),
                inst(8, InstKind::BinOp(BinOp::FMul, v(4), v(5))),
                inst(9, InstKind::BinOp(BinOp::FMul, v(6), v(7))),
                inst(10, InstKind::BinOp(BinOp::FAdd, v(8), v(9))),
                inst(11, InstKind::FpToSi(v(10))),
            ],
            12,
            Terminator::Ret(Some(v(11))),
        );
        let stats = run(&mut f);
        assert_eq!(stats.truncs_narrowed, 1);
        assert_eq!(stats.tree_interior_narrowed, 2);
        assert_eq!(stats.dead_converts_removed, 4);
        // orphaned sitofps swept; the three binops retyped in place.
        let kinds: Vec<_> = f.blocks[0].insts.iter().map(|i| &i.kind).collect();
        assert!(matches!(
            kinds[0],
            InstKind::BinOp(
                BinOp::Mul,
                Operand::Value(ValueId(0)),
                Operand::Value(ValueId(1))
            )
        ));
        assert!(matches!(
            kinds[1],
            InstKind::BinOp(
                BinOp::Mul,
                Operand::Value(ValueId(2)),
                Operand::Value(ValueId(3))
            )
        ));
        assert!(matches!(
            kinds[2],
            InstKind::BinOp(
                BinOp::Add,
                Operand::Value(ValueId(8)),
                Operand::Value(ValueId(9))
            )
        ));
        assert!(matches!(
            kinds[3],
            InstKind::Copy(Type::I64, Operand::Value(ValueId(10)))
        ));
    }

    #[test]
    fn tree_with_multi_use_interior_stays_float() {
        // chunk ② — an interior node consumed outside the tree keeps
        // the whole tree float (retyping it would break the other
        // f64 consumer).
        let mut f = func_with(
            vec![
                inst(2, InstKind::SiToFp(v(0))),
                inst(3, InstKind::SiToFp(v(1))),
                inst(4, InstKind::BinOp(BinOp::FMul, v(2), v(3))),
                inst(5, InstKind::BinOp(BinOp::FAdd, v(4), cf(1.0))),
                inst(6, InstKind::FpToSi(v(5))),
                // second consumer of the fmul result
                inst(7, InstKind::BinOp(BinOp::FAdd, v(4), cf(2.0))),
            ],
            8,
            Terminator::Ret(Some(v(6))),
        );
        let stats = run(&mut f);
        assert_eq!(stats.truncs_narrowed, 0);
        assert!(matches!(
            f.blocks[0].insts[2].kind,
            InstKind::BinOp(BinOp::FMul, _, _)
        ));
    }

    #[test]
    fn tree_with_const_zero_frem_divisor_stays_float() {
        // chunk ② — a const-0 frem divisor anywhere in the tree
        // rejects the whole tree (frem → NaN is the JS answer; srem
        // would be UB).
        let mut f = func_with(
            vec![
                inst(1, InstKind::SiToFp(v(0))),
                inst(2, InstKind::BinOp(BinOp::FRem, v(1), cf(0.0))),
                inst(3, InstKind::BinOp(BinOp::FAdd, v(2), cf(1.0))),
                inst(4, InstKind::FpToSi(v(3))),
            ],
            5,
            Terminator::Ret(Some(v(4))),
        );
        let stats = run(&mut f);
        assert_eq!(stats.truncs_narrowed, 0);
        assert!(matches!(
            f.blocks[0].insts[1].kind,
            InstKind::BinOp(BinOp::FRem, _, _)
        ));
    }

    #[test]
    fn fcmp_over_neg_shaped_fsub_stays_float() {
        // rounding-observable past 2^53 — the negation shape narrows
        // only at the truncating sink, never into a compare.
        let mut f = func_with(
            vec![
                inst(1, InstKind::SiToFp(v(0))),
                inst(2, InstKind::BinOp(BinOp::FSub, cf(-0.0), v(1))),
                inst(3, InstKind::FCmp(FPred::Oeq, v(2), cf(0.0))),
            ],
            4,
            Terminator::CondBr {
                cond: v(3),
                then_blk: BlockId(0),
                else_blk: BlockId(0),
            },
        );
        let stats = run(&mut f);
        assert_eq!(stats.cmps_narrowed, 0);
        assert!(matches!(
            f.blocks[0].insts[1].kind,
            InstKind::BinOp(BinOp::FSub, _, _)
        ));
    }

    #[test]
    fn fptosi_over_frem_collapses_to_srem() {
        // Constant non-zero divisor — the provable srem case.
        let mut f = func_with(
            vec![
                inst(2, InstKind::SiToFp(v(0))),
                inst(4, InstKind::BinOp(BinOp::FRem, v(2), cf(2.0))),
                inst(5, InstKind::FpToSi(v(4))),
            ],
            6,
            Terminator::Ret(Some(v(5))),
        );
        let stats = run(&mut f);
        assert_eq!(stats.truncs_narrowed, 1);
        let insts = &f.blocks[0].insts;
        assert_eq!(insts.len(), 2);
        assert!(matches!(
            insts[0].kind,
            InstKind::BinOp(
                BinOp::SRem,
                Operand::Value(ValueId(0)),
                Operand::ConstI64(2)
            )
        ));
        assert!(matches!(
            insts[1].kind,
            InstKind::Copy(Type::I64, Operand::Value(ValueId(4)))
        ));
    }

    #[test]
    fn fptosi_over_frem_variable_divisor_stays_float() {
        // srem runtime-0 guard: `(a % b) | 0` with b == 0 is
        // NaN → fptosi → 0; an srem recovery would hand back the
        // dividend (printed 7 where bun prints 0). No narrow.
        let mut f = func_with(
            vec![
                inst(2, InstKind::SiToFp(v(0))),
                inst(3, InstKind::SiToFp(v(1))),
                inst(4, InstKind::BinOp(BinOp::FRem, v(2), v(3))),
                inst(5, InstKind::FpToSi(v(4))),
            ],
            6,
            Terminator::Ret(Some(v(5))),
        );
        let stats = run(&mut f);
        assert_eq!(stats.truncs_narrowed, 0);
        assert!(matches!(
            f.blocks[0].insts[2].kind,
            InstKind::BinOp(BinOp::FRem, _, _)
        ));
    }
}
