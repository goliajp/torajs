//! Number printing on a proven-integral f64 — `print_f64(sitofp w)`
//! with `w` inside ±(2^53 − 1) prints exactly what `print_i64(w)`
//! prints: an integer-valued double below 1e21 renders as its digits
//! (§6.1.6.1.20 Number::toString steps 5-6), `sitofp` never mints
//! `-0`, and 2^53 sits well below 1e21. The rewrite drops the f64
//! printer — dtoa plus the 10 KB grisu power table — from every
//! program whose only float is a demoted accumulator's exit bridge
//! (`let total = 0; for (…) total += i; console.log(total)`: the
//! float_demote fast loop is i64, the guard folds, and this is the
//! last f64 use standing — r506, the c11 probe's second page).
//!
//! The bound comes from branch_fold's evaluators: point evaluation
//! for straight-line values, the add-recurrence argument for a loop
//! cell read after its loop (`accum::cell_bound_after_loop`). Runs
//! right after `branch_fold` so a folded guard's dead slow version is
//! already swept and the bridge cell has one def.
//! `TORAJS_PRINT_NARROW_OFF=1` skips, `_STATS=1` dumps counters.

use std::collections::HashMap;

use torajs_core::ssa::{FuncId, Function, InstKind, Module, Operand, Type, ValueId};

use crate::branch_fold::accum::cell_bound_after_loop;
use crate::branch_fold::point_eval::PointEval;
use crate::branch_fold::single_def;
use crate::dominator::DominatorTree;
use crate::interval::transfer::NumFact;
use crate::loop_analysis::LoopAnalysis;

/// The f64 printers and their i64 twins, by intrinsic name.
const PAIRS: [(&str, &str); 2] = [
    ("print_f64", "print_i64"),
    ("print_f64_inline", "print_i64_inline"),
];
const MAX_EXACT: i128 = (1i128 << 53) - 1;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PrintNarrowStats {
    /// f64 print calls rewritten to the i64 printer.
    pub narrowed: u32,
    /// `sitofp` bridges left with no use and removed.
    pub bridges_removed: u32,
}

pub fn narrow_prints(module: &mut Module) -> PrintNarrowStats {
    let mut stats = PrintNarrowStats::default();
    let mut pairs: Vec<(FuncId, FuncId)> = Vec::new();
    for (f64_name, i64_name) in PAIRS {
        let find = |n: &str| {
            module
                .funcs
                .iter()
                .position(|f| f.name == n)
                .map(|i| FuncId(i as u32))
        };
        if let (Some(a), Some(b)) = (find(f64_name), find(i64_name)) {
            pairs.push((a, b));
        }
    }
    if pairs.is_empty() {
        return stats;
    }
    for func in &mut module.funcs {
        if func.is_declaration() {
            continue;
        }
        narrow_in_function(func, &pairs, &mut stats);
    }
    stats
}

fn narrow_in_function(
    func: &mut Function,
    pairs: &[(FuncId, FuncId)],
    stats: &mut PrintNarrowStats,
) {
    let dom = DominatorTree::compute(func);
    let la = LoopAnalysis::compute(func, &dom);
    let pe = PointEval::new(func, &dom);
    // (block, inst) → (i64 twin, integer operand)
    let mut rewrites: Vec<((usize, usize), FuncId, ValueId)> = Vec::new();
    for (bi, block) in func.blocks.iter().enumerate() {
        for (ii, inst) in block.insts.iter().enumerate() {
            let InstKind::Call(fid, args) = &inst.kind else {
                continue;
            };
            let Some((_, twin)) = pairs.iter().find(|(f, _)| f == fid) else {
                continue;
            };
            let [Operand::Value(v)] = args.as_slice() else {
                continue;
            };
            let Some(vd) = single_def(func, *v) else {
                continue;
            };
            let InstKind::SiToFp(Operand::Value(w)) = &func.blocks[vd.0].insts[vd.1].kind else {
                continue;
            };
            if func.values[w.0 as usize].ty != Type::I64 {
                continue;
            }
            let Some(f) = bound_at(func, &la, &pe, &dom, *w, vd) else {
                continue;
            };
            if f.lo >= -MAX_EXACT && f.hi <= MAX_EXACT {
                rewrites.push(((bi, ii), *twin, *w));
            }
        }
    }
    for ((bi, ii), twin, w) in rewrites {
        func.blocks[bi].insts[ii].kind = InstKind::Call(twin, vec![Operand::Value(w)]);
        stats.narrowed += 1;
    }
    if stats.narrowed > 0 {
        stats.bridges_removed += sweep_unused_sitofp(func);
    }
}

/// The value's bound as read at `point`: point evaluation first (a
/// straight-line or uniquely-defined value), then the after-loop
/// cell bound for an accumulator read past its loop.
fn bound_at(
    func: &Function,
    la: &LoopAnalysis,
    pe: &PointEval,
    dom: &DominatorTree,
    w: ValueId,
    point: (usize, usize),
) -> Option<NumFact> {
    pe.eval_value(w, point)
        .or_else(|| cell_bound_after_loop(func, la, pe, dom, w, point))
}

/// Remove `sitofp` results nobody reads any more.
fn sweep_unused_sitofp(func: &mut Function) -> u32 {
    let mut uses: HashMap<ValueId, u32> = HashMap::new();
    for block in &func.blocks {
        for inst in &block.insts {
            torajs_core::ssa::visit_value_operands(&inst.kind, |v| {
                *uses.entry(v).or_default() += 1;
            });
        }
        if let torajs_core::ssa::Terminator::CondBr {
            cond: Operand::Value(v),
            ..
        }
        | torajs_core::ssa::Terminator::Ret(Some(Operand::Value(v))) = &block.term
        {
            *uses.entry(*v).or_default() += 1;
        }
    }
    let mut removed = 0u32;
    for block in &mut func.blocks {
        let before = block.insts.len();
        block.insts.retain(|i| {
            !(matches!(i.kind, InstKind::SiToFp(_))
                && i.result
                    .is_some_and(|r| uses.get(&r).copied().unwrap_or(0) == 0))
        });
        removed += (before - block.insts.len()) as u32;
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{BinOp, Block, BlockId, IPred, Inst, Terminator, ValueInfo};

    fn inst(result: u32, kind: InstKind) -> Inst {
        Inst {
            result: Some(ValueId(result)),
            kind,
            origin: None,
        }
    }

    fn void(kind: InstKind) -> Inst {
        Inst {
            result: None,
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

    fn decl(name: &str, param: Type) -> Function {
        let mut f = Function::new(name, Type::Void);
        f.add_param(param, "a0");
        f
    }

    /// `let s = 0; for (let i = 0; i < K; i++) s += i; print(s)` in
    /// the post-float_demote shape: i64 fast loop, `sitofp` bridge on
    /// the exit edge, f64 print.
    fn sum_loop(k: i64) -> Module {
        let mut m = Module::default();
        m.funcs.push(decl("print_f64", Type::F64)); // 0
        m.funcs.push(decl("print_i64", Type::I64)); // 1
        let values: Vec<Type> = vec![
            Type::I64,  // %0 sum cell
            Type::I64,  // %1 counter cell
            Type::Bool, // %2 cmp
            Type::I64,  // %3 sum + i
            Type::I64,  // %4 i + 1
            Type::F64,  // %5 bridge
        ];
        let blocks = vec![
            Block {
                id: BlockId(0),
                insts: vec![
                    inst(0, InstKind::Copy(Type::I64, c(0))),
                    inst(1, InstKind::Copy(Type::I64, c(0))),
                ],
                term: Terminator::Br(BlockId(1)),
            },
            Block {
                id: BlockId(1),
                insts: vec![inst(2, InstKind::ICmp(IPred::Slt, v(1), c(k)))],
                term: Terminator::CondBr {
                    cond: v(2),
                    then_blk: BlockId(2),
                    else_blk: BlockId(3),
                },
            },
            Block {
                id: BlockId(2),
                insts: vec![
                    inst(3, InstKind::BinOp(BinOp::Add, v(0), v(1))),
                    inst(4, InstKind::BinOp(BinOp::Add, v(1), c(1))),
                    inst(0, InstKind::Copy(Type::I64, v(3))),
                    inst(1, InstKind::Copy(Type::I64, v(4))),
                ],
                term: Terminator::Br(BlockId(1)),
            },
            Block {
                id: BlockId(3),
                insts: vec![
                    inst(5, InstKind::SiToFp(v(0))),
                    void(InstKind::Call(FuncId(0), vec![v(5)])),
                ],
                term: Terminator::Ret(None),
            },
        ];
        m.funcs.push(Function {
            name: "main".into(),
            params: vec![],
            ret: Type::Void,
            blocks,
            values: values
                .into_iter()
                .map(|ty| ValueInfo { ty, name: None })
                .collect(),
            current_origin: None,
        });
        m
    }

    #[test]
    fn bounded_sum_prints_through_the_i64_printer() {
        let mut m = sum_loop(1000);
        let stats = narrow_prints(&mut m);
        assert_eq!(stats.narrowed, 1);
        assert_eq!(stats.bridges_removed, 1);
        let exit = &m.funcs[2].blocks[3].insts;
        assert_eq!(exit.len(), 1);
        assert!(matches!(&exit[0].kind, InstKind::Call(FuncId(1), a) if a == &vec![v(0)]));
    }

    #[test]
    fn sum_that_may_pass_2_53_keeps_the_f64_printer() {
        // trip 2^30 × inc up to 2^30 = 2^60 > 2^53
        let mut m = sum_loop(1 << 30);
        let stats = narrow_prints(&mut m);
        assert_eq!(stats.narrowed, 0);
        assert!(matches!(
            &m.funcs[2].blocks[3].insts[1].kind,
            InstKind::Call(FuncId(0), _)
        ));
    }
}
