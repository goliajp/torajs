//! Interval-decided compare absorption — a boolean `and` / `or` one of
//! whose operands is an integer compare the interval facts already
//! decide (`true` under `and`, `false` under `or`) collapses onto its
//! other operand, when that operand is a single-use compare: the
//! compare moves into the `and` / `or` slot and both originals go, as
//! does an add or sub only the decided compare read (the shifted root
//! of a square's range guard).
//!
//! The shape comes from `frem_narrow`'s fused zero guard: `n % i ===
//! 0` over a variable divisor narrows to `and(icmp ne i, 0, icmp eq
//! srem(n, i), 0)`, because srem-by-zero on aarch64 hands the
//! dividend back where JS answers NaN. frem_narrow runs before the
//! interval analysis and cannot see that a trial-division counter
//! starting at 2 is never 0; after the analysis the guard is two
//! dead compares plus an `and` in the hottest loop of the program
//! (prime_count: 5 of the 13 inner-loop instructions). Absorbing it
//! leaves the remainder compare adjacent to the `cond_br`, which is
//! what codegen's compare/branch fuse needs (`cbz`).
//!
//! Facts are flow-insensitive joins over every def site, so a decided
//! compare is decided at every program point.
//!
//! Runs right after float_demote (the facts are still id-valid there)
//! and before srem_parity. `TORAJS_CMP_FACT_FOLD_OFF=1` skips,
//! `TORAJS_CMP_FACT_FOLD_STATS=1` dumps counters.

use std::collections::HashMap;

use torajs_core::ssa::{
    BinOp, Function, IPred, InstKind, Module, Operand, Terminator, Type, ValueId,
};

use crate::interval::NumFact;
use crate::interval::transfer::op_const_int;
use crate::rc_peephole::visit_value_operands;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CmpFactFoldStats {
    /// `and` / `or` defs that absorbed a decided compare.
    pub absorbed: u32,
}

pub fn fold_decided_cmps(
    module: &mut Module,
    facts: &[HashMap<ValueId, NumFact>],
) -> CmpFactFoldStats {
    let mut stats = CmpFactFoldStats::default();
    for (func, fm) in module.funcs.iter_mut().zip(facts) {
        if func.is_declaration() {
            continue;
        }
        fold_in_function(func, fm, &mut stats);
    }
    stats
}

/// The compare's answer at every program point, if the facts fix it.
fn decide(pred: IPred, a: NumFact, b: NumFact) -> Option<bool> {
    let lt = |x: NumFact, y: NumFact| {
        if x.hi < y.lo {
            Some(true)
        } else if x.lo >= y.hi {
            Some(false)
        } else {
            None
        }
    };
    let le = |x: NumFact, y: NumFact| {
        if x.hi <= y.lo {
            Some(true)
        } else if x.lo > y.hi {
            Some(false)
        } else {
            None
        }
    };
    let eq = if a.hi < b.lo || a.lo > b.hi {
        Some(false)
    } else if a.lo == a.hi && b.lo == b.hi {
        Some(true)
    } else {
        None
    };
    match pred {
        IPred::Eq => eq,
        IPred::Ne => eq.map(|e| !e),
        IPred::Slt => lt(a, b),
        IPred::Sle => le(a, b),
        IPred::Sgt => lt(b, a),
        IPred::Sge => le(b, a),
        // unsigned order agrees with signed only when both sides are
        // known non-negative
        IPred::Ult if a.lo >= 0 && b.lo >= 0 => lt(a, b),
        IPred::Uge if a.lo >= 0 && b.lo >= 0 => le(b, a),
        IPred::Ult | IPred::Uge => None,
    }
}

fn operand_fact(
    func: &Function,
    facts: &HashMap<ValueId, NumFact>,
    op: &Operand,
) -> Option<NumFact> {
    match op {
        Operand::Value(v) if func.values[v.0 as usize].ty == Type::I64 => facts.get(v).copied(),
        Operand::Value(_) => None,
        _ => op_const_int(op).map(NumFact::point),
    }
}

/// Where each value is defined, and how many def sites / uses it has.
struct UseDef {
    site: HashMap<ValueId, (usize, usize)>,
    defs: HashMap<ValueId, u32>,
    uses: HashMap<ValueId, u32>,
}

fn use_def(func: &Function) -> UseDef {
    let mut ud = UseDef {
        site: HashMap::new(),
        defs: HashMap::new(),
        uses: HashMap::new(),
    };
    for (bi, block) in func.blocks.iter().enumerate() {
        for (ii, inst) in block.insts.iter().enumerate() {
            if let Some(r) = inst.result {
                ud.site.insert(r, (bi, ii));
                *ud.defs.entry(r).or_default() += 1;
            }
            visit_value_operands(&inst.kind, |v| *ud.uses.entry(v).or_default() += 1);
        }
        match &block.term {
            Terminator::CondBr {
                cond: Operand::Value(v),
                ..
            }
            | Terminator::Ret(Some(Operand::Value(v))) => *ud.uses.entry(*v).or_default() += 1,
            _ => {}
        }
    }
    ud
}

impl UseDef {
    /// Single-def, single-use value defined by a compare; its site.
    fn lone_cmp(&self, func: &Function, v: &Operand) -> Option<(ValueId, usize, usize)> {
        let Operand::Value(v) = v else {
            return None;
        };
        if self.defs.get(v) != Some(&1) || self.uses.get(v) != Some(&1) {
            return None;
        }
        let (bi, ii) = self.site[v];
        matches!(func.blocks[bi].insts[ii].kind, InstKind::ICmp(..)).then_some((*v, bi, ii))
    }

    /// Site of a single-def, single-use integer add or sub (a guard's
    /// shifted operand).
    fn lone_arith(&self, func: &Function, v: &Operand) -> Option<(usize, usize)> {
        let Operand::Value(v) = v else {
            return None;
        };
        if self.defs.get(v) != Some(&1) || self.uses.get(v) != Some(&1) {
            return None;
        }
        let (bi, ii) = self.site[v];
        matches!(
            func.blocks[bi].insts[ii].kind,
            InstKind::BinOp(BinOp::Add | BinOp::Sub, ..)
        )
        .then_some((bi, ii))
    }
}

fn fold_in_function(
    func: &mut Function,
    facts: &HashMap<ValueId, NumFact>,
    stats: &mut CmpFactFoldStats,
) {
    let ud = use_def(func);
    // (block, and/or index, decided cmp site, kept cmp index)
    let mut plans: Vec<(usize, usize, (usize, usize), usize)> = Vec::new();
    // what only a decided compare read: an operand it alone used
    let mut feeds: Vec<(usize, usize)> = Vec::new();
    for (bi, block) in func.blocks.iter().enumerate() {
        for (ii, inst) in block.insts.iter().enumerate() {
            let (Some(r), InstKind::BinOp(op @ (BinOp::And | BinOp::Or), x, y)) =
                (inst.result, &inst.kind)
            else {
                continue;
            };
            if func.values[r.0 as usize].ty != Type::Bool || ud.defs.get(&r) != Some(&1) {
                continue;
            }
            let absorbing = *op == BinOp::And;
            for (dec, keep) in [(x, y), (y, x)] {
                let Some((_, dbi, dii)) = ud.lone_cmp(func, dec) else {
                    continue;
                };
                let InstKind::ICmp(pred, a, b) = &func.blocks[dbi].insts[dii].kind else {
                    unreachable!("lone_cmp checked the kind");
                };
                let (Some(fa), Some(fb)) =
                    (operand_fact(func, facts, a), operand_fact(func, facts, b))
                else {
                    continue;
                };
                if decide(*pred, fa, fb) != Some(absorbing) {
                    continue;
                }
                let Some((_, kbi, kii)) = ud.lone_cmp(func, keep) else {
                    continue;
                };
                if kbi != bi || kii >= ii || !operands_stable(func, bi, kii, ii) {
                    continue;
                }
                plans.push((bi, ii, (dbi, dii), kii));
                feeds.extend([a, b].into_iter().filter_map(|o| ud.lone_arith(func, o)));
                break;
            }
        }
    }
    let mut dead: Vec<(usize, usize)> = feeds;
    for (bi, ii, dsite, kii) in plans {
        let kept = func.blocks[bi].insts[kii].kind.clone();
        func.blocks[bi].insts[ii].kind = kept;
        dead.push(dsite);
        dead.push((bi, kii));
        stats.absorbed += 1;
    }
    dead.sort_unstable();
    dead.dedup();
    for (bi, ii) in dead.into_iter().rev() {
        func.blocks[bi].insts.remove(ii);
    }
}

/// No inst strictly between `from` and `to` redefines an operand of
/// the compare at `from` (multi-def Copy cells can be reassigned).
fn operands_stable(func: &Function, bi: usize, from: usize, to: usize) -> bool {
    let insts = &func.blocks[bi].insts;
    let mut ok = true;
    visit_value_operands(&insts[from].kind, |v| {
        if insts[from + 1..to].iter().any(|i| i.result == Some(v)) {
            ok = false;
        }
    });
    ok
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{Block, BlockId, Inst, ValueInfo};

    fn inst(r: u32, kind: InstKind) -> Inst {
        Inst {
            result: Some(ValueId(r)),
            kind,
            origin: None,
        }
    }
    fn v(id: u32) -> Operand {
        Operand::Value(ValueId(id))
    }

    /// `%0 = n` `%1 = i` · `%2 = srem %0,%1` · `%3 = icmp ne %1,0` ·
    /// `%4 = icmp eq %2,0` · `%5 = and %3,%4` · `cond_br %5`.
    fn guard_fn() -> Function {
        let ty = |t| ValueInfo { ty: t, name: None };
        Function {
            name: "f".into(),
            params: vec![ValueId(0), ValueId(1)],
            ret: Type::I64,
            blocks: vec![Block {
                id: BlockId(0),
                insts: vec![
                    inst(2, InstKind::BinOp(BinOp::SRem, v(0), v(1))),
                    inst(3, InstKind::ICmp(IPred::Ne, v(1), Operand::ConstI64(0))),
                    inst(4, InstKind::ICmp(IPred::Eq, v(2), Operand::ConstI64(0))),
                    inst(5, InstKind::BinOp(BinOp::And, v(3), v(4))),
                ],
                term: Terminator::CondBr {
                    cond: v(5),
                    then_blk: BlockId(0),
                    else_blk: BlockId(0),
                },
            }],
            values: vec![
                ty(Type::I64),
                ty(Type::I64),
                ty(Type::I64),
                ty(Type::Bool),
                ty(Type::Bool),
                ty(Type::Bool),
            ],
            current_origin: None,
        }
    }

    #[test]
    fn nonzero_divisor_guard_is_absorbed() {
        let mut f = guard_fn();
        let facts = HashMap::from([(ValueId(1), NumFact::new(2, 1 << 70))]);
        let mut stats = CmpFactFoldStats::default();
        fold_in_function(&mut f, &facts, &mut stats);
        assert_eq!(stats.absorbed, 1);
        let insts = &f.blocks[0].insts;
        assert_eq!(insts.len(), 2);
        assert_eq!(insts[1].result, Some(ValueId(5)));
        assert_eq!(
            insts[1].kind,
            InstKind::ICmp(IPred::Eq, v(2), Operand::ConstI64(0))
        );
    }

    #[test]
    fn divisor_that_may_be_zero_keeps_the_guard() {
        let mut f = guard_fn();
        let facts = HashMap::from([(ValueId(1), NumFact::new(0, 10))]);
        let mut stats = CmpFactFoldStats::default();
        fold_in_function(&mut f, &facts, &mut stats);
        assert_eq!(stats.absorbed, 0);
        assert_eq!(f.blocks[0].insts.len(), 4);
    }

    /// A square's range guard: `%2 = mul %1,%1` · `%3 = icmp sle %2,%0` ·
    /// `%4 = add %1, R` · `%5 = icmp ult %4, 2R+1` · `%6 = and %5,%3`.
    #[test]
    fn a_decided_range_guard_goes_with_its_shifted_root() {
        const R: i64 = 3_037_000_499;
        let mut f = guard_fn();
        f.blocks[0].insts = vec![
            inst(2, InstKind::BinOp(BinOp::Mul, v(1), v(1))),
            inst(3, InstKind::ICmp(IPred::Sle, v(2), v(0))),
            inst(4, InstKind::BinOp(BinOp::Add, v(1), Operand::ConstI64(R))),
            inst(
                5,
                InstKind::ICmp(IPred::Ult, v(4), Operand::ConstI64(2 * R + 1)),
            ),
            inst(6, InstKind::BinOp(BinOp::And, v(5), v(3))),
        ];
        f.blocks[0].term = Terminator::CondBr {
            cond: v(6),
            then_blk: BlockId(0),
            else_blk: BlockId(0),
        };
        f.values.push(ValueInfo {
            ty: Type::Bool,
            name: None,
        });
        f.values[4].ty = Type::I64;
        let facts = HashMap::from([
            (ValueId(1), NumFact::new(1, 46_341)),
            (
                ValueId(4),
                NumFact::new(i128::from(R) + 1, i128::from(R) + 46_341),
            ),
        ]);
        let mut stats = CmpFactFoldStats::default();
        fold_in_function(&mut f, &facts, &mut stats);
        assert_eq!(stats.absorbed, 1);
        let kinds: Vec<&InstKind> = f.blocks[0].insts.iter().map(|i| &i.kind).collect();
        assert_eq!(
            kinds,
            [
                &InstKind::BinOp(BinOp::Mul, v(1), v(1)),
                &InstKind::ICmp(IPred::Sle, v(2), v(0)),
            ]
        );
    }

    #[test]
    fn decide_orders_and_equality() {
        let p = NumFact::point;
        assert_eq!(decide(IPred::Ne, NumFact::new(1, 5), p(0)), Some(true));
        assert_eq!(decide(IPred::Eq, p(3), p(3)), Some(true));
        assert_eq!(decide(IPred::Slt, NumFact::new(0, 4), p(5)), Some(true));
        assert_eq!(decide(IPred::Sge, NumFact::new(0, 4), p(4)), None);
        assert_eq!(decide(IPred::Sgt, NumFact::new(5, 9), p(4)), Some(true));
        assert_eq!(decide(IPred::Sle, NumFact::new(5, 9), p(4)), Some(false));
    }
}
