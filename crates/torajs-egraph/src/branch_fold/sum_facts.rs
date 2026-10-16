//! Loop sums bounded before float demotion: an `add` / `fadd` whose
//! lattice fact the cell's own join widened past 2^53 gets the
//! add-recurrence bound `accum` proves (`s = 0; while (d * d <= n) {
//! if (n % d === 0) s += d; d++ }` sums at most `trip * max(d)`). A
//! sum the bound keeps inside ±2^53 then demotes to i64 with no
//! per-iteration window check and no slow copy of the loop.
//!
//! The bound is a fact like any other the lattice states: it holds at
//! every execution of the def, so it is met into the fact the def
//! already has. `TORAJS_SUM_FACTS_OFF=1` skips.

use crate::id_hash::HashMap;

use torajs_core::ssa::dominator::DominatorTree;
use torajs_core::ssa::loop_analysis::LoopAnalysis;
use torajs_core::ssa::{BinOp, InstKind, Module, ValueId};

use super::accum::accum_bound;
use super::point_eval::PointEval;
use crate::interval::FactCache;
use crate::interval::transfer::NumFact;

const F64_EXACT: i128 = 1 << 53;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SumFactStats {
    /// sums whose fact the recurrence bound narrowed
    pub bounded: u32,
}

/// `cache` holds the lattice facts as analyzed, before this pass
/// tightens `facts`.
pub fn bound_sums(
    module: &Module,
    facts: &mut [HashMap<ValueId, NumFact>],
    cache: Option<&FactCache>,
) -> SumFactStats {
    let (cache, names) = FactCache::for_module(cache, module);
    let mut stats = SumFactStats::default();
    for (fi, (func, fm)) in module.funcs.iter().zip(facts.iter_mut()).enumerate() {
        if func.is_declaration() {
            continue;
        }
        let open = |v: ValueId| {
            fm.get(&v)
                .is_some_and(|f| f.hi >= F64_EXACT || f.lo <= -F64_EXACT)
        };
        let mut sums: Vec<(ValueId, (usize, usize))> = Vec::new();
        for (bi, block) in func.blocks.iter().enumerate() {
            for (ii, inst) in block.insts.iter().enumerate() {
                if let (Some(r), InstKind::BinOp(BinOp::Add | BinOp::FAdd, ..)) =
                    (inst.result, &inst.kind)
                    && open(r)
                {
                    sums.push((r, (bi, ii)));
                }
            }
        }
        if sums.is_empty() {
            continue;
        }
        let dom = DominatorTree::compute(func);
        let la = LoopAnalysis::compute(func, &dom);
        let pe = PointEval::new(func, &dom, &la, &names, cache.and_then(|c| c.get(fi, func)));
        for (x, at) in sums {
            if pe.single_def(x).is_none() {
                continue;
            }
            if let Some(bound) = accum_bound(func, &la, &pe, x, at)
                && let Some(f) = fm.get_mut(&x)
                && let Some(met) = f.meet(bound)
            {
                *f = met;
                stats.bounded += 1;
            }
        }
    }
    stats
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{
        Block, BlockId, Function, IPred, Inst, Operand, Terminator, Type, ValueInfo,
    };

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

    fn block(id: u32, insts: Vec<Inst>, term: Terminator) -> Block {
        Block {
            id: BlockId(id),
            insts,
            term,
        }
    }

    fn cond(c: u32, t: u32, e: u32) -> Terminator {
        Terminator::CondBr {
            cond: v(c),
            then_blk: BlockId(t),
            else_blk: BlockId(e),
        }
    }

    /// `s = 0.0; for (d = 1; d * d <= n; d++) if (n % d === 0) s += d;
    /// return s` before float demotion: `s` an f64 cell, `d` an i64
    /// counter, `%0` n.
    ///
    ///   b0: n; s=0.0; d=1              b3: t=sitofp d; x=s+t; m=x → b5
    ///   b1: d*d <= n ? b2 : b6         b4: m=s → b5
    ///   b2: n%d == 0 ? b3 : b4         b5: d++; s=m → b1     b6: ret s
    fn divisor_sum(n: i64) -> Module {
        let i64c = |k: i64| Operand::ConstI64(k);
        let blocks = vec![
            block(
                0,
                vec![
                    inst(0, InstKind::Copy(Type::I64, i64c(n))),
                    inst(1, InstKind::Copy(Type::F64, Operand::ConstF64(0.0))),
                    inst(2, InstKind::Copy(Type::I64, i64c(1))),
                ],
                Terminator::Br(BlockId(1)),
            ),
            block(
                1,
                vec![
                    inst(3, InstKind::BinOp(BinOp::Mul, v(2), v(2))),
                    inst(4, InstKind::ICmp(IPred::Sle, v(3), v(0))),
                ],
                cond(4, 2, 6),
            ),
            block(
                2,
                vec![
                    inst(5, InstKind::BinOp(BinOp::SRem, v(0), v(2))),
                    inst(6, InstKind::ICmp(IPred::Eq, v(5), i64c(0))),
                ],
                cond(6, 3, 4),
            ),
            block(
                3,
                vec![
                    inst(7, InstKind::SiToFp(v(2))),
                    inst(8, InstKind::BinOp(BinOp::FAdd, v(1), v(7))),
                    inst(9, InstKind::Copy(Type::F64, v(8))),
                ],
                Terminator::Br(BlockId(5)),
            ),
            block(
                4,
                vec![inst(9, InstKind::Copy(Type::F64, v(1)))],
                Terminator::Br(BlockId(5)),
            ),
            block(
                5,
                vec![
                    inst(10, InstKind::BinOp(BinOp::Add, v(2), i64c(1))),
                    inst(1, InstKind::Copy(Type::F64, v(9))),
                    inst(2, InstKind::Copy(Type::I64, v(10))),
                ],
                Terminator::Br(BlockId(1)),
            ),
            block(6, vec![], Terminator::Ret(Some(v(1)))),
        ];
        let f64s = [1, 7, 8, 9];
        let values = (0..11)
            .map(|i| ValueInfo {
                ty: if f64s.contains(&i) {
                    Type::F64
                } else {
                    Type::I64
                },
                name: None,
            })
            .collect();
        Module {
            funcs: vec![Function {
                name: "main".into(),
                params: vec![],
                ret: Type::F64,
                blocks,
                values,
                current_origin: None,
            }],
            ..Default::default()
        }
    }

    fn sum_fact(n: i64) -> (NumFact, SumFactStats) {
        let m = divisor_sum(n);
        let mut facts = crate::interval::analyze_module(&m);
        assert!(
            facts[0][&ValueId(8)].hi >= F64_EXACT,
            "the lattice leaves the sum open"
        );
        let stats = bound_sums(&m, &mut facts, None);
        (facts[0][&ValueId(8)], stats)
    }

    #[test]
    fn a_divisor_sum_is_bounded_by_its_trips() {
        // at most 1001 trips (d from 1 to 1001 at the header), each
        // adding at most 1000 (d * d <= 10^6 in the body)
        let (f, stats) = sum_fact(1_000_000);
        assert_eq!(stats.bounded, 1);
        assert_eq!(f, NumFact::new(1, 1001 * 1000));
    }

    #[test]
    fn an_f64_sum_past_2_pow_53_stays_open() {
        // 2^30 + 1 trips of up to 2^30 each pass 2^53
        let (f, stats) = sum_fact(1 << 60);
        assert_eq!(stats.bounded, 0);
        assert!(f.hi >= F64_EXACT);
    }
}
