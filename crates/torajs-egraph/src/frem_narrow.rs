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
//! * **Truncate (R2)** — `fptosi(frem(x̂, ŷ))` over a single-use frem:
//!   the i64 sink already collapses -0 to 0, so the float detour
//!   computes the same i64. The frem retypes to srem in place and the
//!   fptosi collapses to a Copy of it.
//!
//! A constant-zero divisor never narrows (frem → NaN is the JS `a % 0`
//! answer; srem would be UB). A *runtime* zero divisor in the narrowed
//! form is UB-parity with the pre-W3 int path (known debt, ssa_lower
//! Mod arm comment).
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
    /// fptosi-over-frem chains collapsed to srem.
    pub truncs_narrowed: u32,
    /// Orphaned sitofp insts swept after the rewrites.
    pub dead_converts_removed: u32,
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

fn narrow_in_function(func: &mut Function, stats: &mut FremNarrowStats) {
    // def summaries for single-def values; multi-def values (post-φ
    // Copy cells) are conservatively unknown.
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

    // value → use count, across every inst operand and terminator.
    let mut uses: HashMap<ValueId, u32> = HashMap::new();
    for block in &func.blocks {
        for inst in &block.insts {
            visit_value_operands(&inst.kind, |v| {
                *uses.entry(v).or_default() += 1;
            });
        }
        match &block.term {
            Terminator::Ret(Some(Operand::Value(v))) => {
                *uses.entry(*v).or_default() += 1;
            }
            Terminator::CondBr {
                cond: Operand::Value(v),
                ..
            } => {
                *uses.entry(*v).or_default() += 1;
            }
            _ => {}
        }
    }

    // A single-use single-def frem whose operands peel to int form and
    // whose divisor is not a compile-time zero.
    let narrowable = |v: &ValueId| -> Option<(Operand, Operand)> {
        if multi.contains(v) || uses.get(v).copied().unwrap_or(0) != 1 {
            return None;
        }
        let InstKind::BinOp(BinOp::FRem, a, b) = defs.get(v)? else {
            return None;
        };
        let bi = int_form(b, &defs)?;
        if matches!(bi, Operand::ConstI64(0)) {
            return None;
        }
        Some((int_form(a, &defs)?, bi))
    };

    // Decision pass — collect frem results to retype, keyed by the
    // consuming inst shape.
    let mut narrow: HashMap<ValueId, (Operand, Operand)> = HashMap::new();
    let mut cmp_results: HashSet<ValueId> = HashSet::new();
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
                    if let Some(ops) = narrowable(v) {
                        narrow.insert(*v, ops);
                        cmp_results.insert(*v);
                        stats.cmps_narrowed += 1;
                    }
                }
                InstKind::FpToSi(Operand::Value(v)) => {
                    if let Some(ops) = narrowable(v) {
                        narrow.insert(*v, ops);
                        trunc_results.insert(*v);
                        stats.truncs_narrowed += 1;
                    }
                }
                _ => {}
            }
        }
    }
    if narrow.is_empty() {
        return;
    }

    // Mutation pass — retype each chosen frem to srem in place, then
    // rewrite its single consumer.
    for block in func.blocks.iter_mut() {
        for inst in block.insts.iter_mut() {
            if let Some(r) = inst.result {
                if let Some((xi, yi)) = narrow.get(&r) {
                    if matches!(inst.kind, InstKind::BinOp(BinOp::FRem, _, _)) {
                        inst.kind = InstKind::BinOp(BinOp::SRem, *xi, *yi);
                        func.values[r.0 as usize].ty = Type::I64;
                        continue;
                    }
                }
            }
            match &inst.kind {
                InstKind::FCmp(p, a, b) => {
                    let int_side = |op: &Operand| -> Option<Operand> {
                        match op {
                            Operand::Value(v) if cmp_results.contains(v) => Some(*op),
                            Operand::Value(_) => None,
                            o => op_const_int(o).map(|c| Operand::ConstI64(c as i64)),
                        }
                    };
                    let frem_side =
                        |op: &Operand| matches!(op, Operand::Value(v) if cmp_results.contains(v));
                    if frem_side(a) || frem_side(b) {
                        if let (Some(na), Some(nb)) = (int_side(a), int_side(b)) {
                            inst.kind = InstKind::ICmp(fpred_to_ipred(*p), na, nb);
                        }
                    }
                }
                InstKind::FpToSi(Operand::Value(v)) if trunc_results.contains(v) => {
                    inst.kind = InstKind::Copy(Type::I64, Operand::Value(*v));
                }
                _ => {}
            }
        }
    }

    // DCE — sitofp insts orphaned by the operand peel.
    loop {
        let mut used: HashSet<ValueId> = HashSet::new();
        for block in &func.blocks {
            for inst in &block.insts {
                visit_value_operands(&inst.kind, |v| {
                    used.insert(v);
                });
            }
            match &block.term {
                Terminator::Ret(Some(Operand::Value(v))) => {
                    used.insert(*v);
                }
                Terminator::CondBr {
                    cond: Operand::Value(v),
                    ..
                } => {
                    used.insert(*v);
                }
                _ => {}
            }
        }
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
        // %2 = sitofp %0 ; %3 = sitofp %1 ; %4 = frem %2, %3
        // %5 = fcmp oeq %4, 0.0 ; cond_br %5
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
        assert_eq!(stats.dead_converts_removed, 2);
        let insts = &f.blocks[0].insts;
        assert_eq!(insts.len(), 2);
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
            InstKind::ICmp(IPred::Eq, Operand::Value(ValueId(4)), Operand::ConstI64(0))
        ));
        assert_eq!(f.values[4].ty, Type::I64);
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
    fn fptosi_over_frem_collapses_to_srem() {
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
        assert_eq!(stats.truncs_narrowed, 1);
        let insts = &f.blocks[0].insts;
        assert_eq!(insts.len(), 2);
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
            InstKind::Copy(Type::I64, Operand::Value(ValueId(4)))
        ));
    }
}
