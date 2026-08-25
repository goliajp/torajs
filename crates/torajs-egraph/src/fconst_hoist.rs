//! FP-constant loop hoisting — mandelbrot decomposition D1.
//!
//! Codegen materializes a `ConstF64` operand at every use: a 3-inst
//! `mov/movk/fmov` sequence plus a GPR→FPR domain crossing
//! (compile/operand.rs `materialize_operand_fpr`). Inside a loop that
//! is a per-iteration tax — the mandelbrot inner loop paid 6
//! instructions per iteration re-building `4.0` and `2.0` while
//! rustc keeps both in registers hoisted above the loop.
//!
//! This pass gives each in-loop f64 constant a value: for every
//! natural loop, each distinct constant bit-pattern used by a loop
//! body instruction is minted once as `Copy(F64, ConstF64(c))` at the
//! tail of the loop header's immediate dominator (which dominates the
//! whole loop), and the body operands rewrite to the minted value.
//! Register allocation then keeps it resident in an FPR across the
//! loop — and even a spill decision degrades to a 1-inst `ldr`,
//! still cheaper than the 3-inst rematerialization.
//!
//! Conservative by construction: only the pure computation kinds
//! listed in `rewrite_kind` are rewritten (BinOp / FCmp / Select /
//! Neg / Copy — every shape the decomposition observed); any other
//! kind keeps its inline constant, which costs optimality, never
//! correctness. Loops whose header has no strict dominator outside
//! the loop body (entry-header or irreducible-adjacent shapes) are
//! skipped.

use std::collections::HashMap;

use torajs_core::ssa::{Function, Inst, InstKind, Module, Operand, Type, ValueId, ValueInfo};

use crate::dominator::DominatorTree;
use crate::loop_analysis::LoopAnalysis;

#[derive(Debug, Default)]
pub struct FconstHoistStats {
    /// Distinct (loop, constant) pairs minted as a preheader Copy.
    pub consts_hoisted: u32,
    /// ConstF64 operands rewritten to a minted value.
    pub uses_rewritten: u32,
}

pub fn hoist_fp_consts(module: &mut Module) -> FconstHoistStats {
    let mut stats = FconstHoistStats::default();
    for func in module.funcs.iter_mut() {
        if func.is_declaration() {
            continue;
        }
        hoist_fn(func, &mut stats);
    }
    stats
}

fn hoist_fn(func: &mut Function, stats: &mut FconstHoistStats) {
    let dom = DominatorTree::compute(func);
    let loops = LoopAnalysis::compute(func, &dom);
    let n = func.blocks.len();
    for lp in loops.loops().to_vec() {
        let Some(pre) = dom.immediate_dominator(lp.header) else {
            continue;
        };
        // The CHK convention makes the entry its own idom; that and
        // any dominator inside the loop body cannot host the mint
        // (the Copy must run exactly once before the loop).
        if pre == lp.header || lp.body.contains(&pre) || pre.0 as usize >= n {
            continue;
        }
        // Pass 1 (read-only): collect the distinct constants the loop
        // body uses in rewritable positions.
        let mut seen: Vec<f64> = Vec::new();
        for &b in &lp.body {
            let bi = b.0 as usize;
            if bi >= n {
                continue;
            }
            for inst in &func.blocks[bi].insts {
                let mut kind = inst.kind.clone();
                rewrite_kind(&mut kind, |c| {
                    if !seen.iter().any(|s| s.to_bits() == c.to_bits()) {
                        seen.push(c);
                    }
                    Operand::ConstF64(c)
                });
            }
        }
        if seen.is_empty() {
            continue;
        }
        // Mint one Copy per constant at the dominator's tail.
        let mut minted: HashMap<u64, ValueId> = HashMap::new();
        for c in &seen {
            let vid = ValueId(func.values.len() as u32);
            func.values.push(ValueInfo {
                ty: Type::F64,
                name: Some(format!("fconst_{}", vid.0)),
            });
            func.blocks[pre.0 as usize].insts.push(Inst {
                result: Some(vid),
                kind: InstKind::Copy(Type::F64, Operand::ConstF64(*c)),
                origin: None,
            });
            minted.insert(c.to_bits(), vid);
            stats.consts_hoisted += 1;
        }
        // Pass 2 (mutating): rewrite the body operands.
        for &b in &lp.body {
            let bi = b.0 as usize;
            if bi >= n {
                continue;
            }
            for inst in &mut func.blocks[bi].insts {
                stats.uses_rewritten +=
                    rewrite_kind(&mut inst.kind, |c| Operand::Value(minted[&c.to_bits()]));
            }
        }
    }
}

/// Rewrite every `ConstF64` operand of the listed pure computation
/// kinds through `lookup`; returns how many operands were visited.
/// Kinds outside the list keep their inline constants — a coverage
/// gap here costs a rematerialization, never correctness.
fn rewrite_kind(kind: &mut InstKind, mut lookup: impl FnMut(f64) -> Operand) -> u32 {
    let mut hits = 0;
    let mut rw = |op: &mut Operand, hits: &mut u32| {
        if let Operand::ConstF64(c) = op {
            *op = lookup(*c);
            *hits += 1;
        }
    };
    match kind {
        InstKind::BinOp(_, a, b) | InstKind::FCmp(_, a, b) => {
            rw(a, &mut hits);
            rw(b, &mut hits);
        }
        InstKind::Select(_, c, t, e) => {
            rw(c, &mut hits);
            rw(t, &mut hits);
            rw(e, &mut hits);
        }
        InstKind::Neg(a) | InstKind::Copy(_, a) => rw(a, &mut hits),
        _ => {}
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{BinOp, Block, BlockId, FPred, Terminator};

    fn vi(ty: Type) -> ValueInfo {
        ValueInfo { ty, name: None }
    }
    fn mk(result: u32, kind: InstKind) -> Inst {
        Inst {
            result: Some(ValueId(result)),
            kind,
            origin: None,
        }
    }

    /// b0(entry) → b1(header: fcmp %v0 > 4.0, CondBr b2/b3) →
    /// b2(body: %2 = fmul %v0 2.0, Br b1) / b3(exit).
    #[test]
    fn in_loop_f64_consts_hoist_to_the_dominator() {
        let func = Function {
            name: "f".into(),
            params: vec![],
            ret: Type::I64,
            values: vec![vi(Type::F64), vi(Type::Bool), vi(Type::F64)],
            blocks: vec![
                Block {
                    id: BlockId(0),
                    insts: vec![],
                    term: Terminator::Br(BlockId(1)),
                },
                Block {
                    id: BlockId(1),
                    insts: vec![mk(
                        1,
                        InstKind::FCmp(
                            FPred::Ogt,
                            Operand::Value(ValueId(0)),
                            Operand::ConstF64(4.0),
                        ),
                    )],
                    term: Terminator::CondBr {
                        cond: Operand::Value(ValueId(1)),
                        then_blk: BlockId(3),
                        else_blk: BlockId(2),
                    },
                },
                Block {
                    id: BlockId(2),
                    insts: vec![mk(
                        2,
                        InstKind::BinOp(
                            BinOp::FMul,
                            Operand::Value(ValueId(0)),
                            Operand::ConstF64(4.0),
                        ),
                    )],
                    term: Terminator::Br(BlockId(1)),
                },
                Block {
                    id: BlockId(3),
                    insts: vec![],
                    term: Terminator::Ret(None),
                },
            ],
            current_origin: None,
        };
        let mut module = Module {
            funcs: vec![func],
            ..Default::default()
        };
        let stats = hoist_fp_consts(&mut module);
        // One distinct constant (4.0, shared by both uses), two uses.
        assert_eq!(stats.consts_hoisted, 1);
        assert_eq!(stats.uses_rewritten, 2);
        let f = &module.funcs[0];
        // The mint sits in b0 (idom of the header), not in the loop.
        assert_eq!(f.blocks[0].insts.len(), 1);
        let minted = f.blocks[0].insts[0].result.unwrap();
        assert!(matches!(
            f.blocks[0].insts[0].kind,
            InstKind::Copy(Type::F64, Operand::ConstF64(c)) if c == 4.0
        ));
        // Both in-loop uses now read the minted value.
        assert!(matches!(
            f.blocks[1].insts[0].kind,
            InstKind::FCmp(_, _, Operand::Value(v)) if v == minted
        ));
        assert!(matches!(
            f.blocks[2].insts[0].kind,
            InstKind::BinOp(BinOp::FMul, _, Operand::Value(v)) if v == minted
        ));
    }

    /// Straight-line code keeps inline constants — no loop, no mint.
    #[test]
    fn straight_line_consts_stay_inline() {
        let mut module = Module {
            funcs: vec![Function {
                name: "f".into(),
                params: vec![],
                ret: Type::I64,
                values: vec![vi(Type::F64), vi(Type::F64)],
                blocks: vec![Block {
                    id: BlockId(0),
                    insts: vec![mk(
                        1,
                        InstKind::BinOp(
                            BinOp::FAdd,
                            Operand::Value(ValueId(0)),
                            Operand::ConstF64(1.5),
                        ),
                    )],
                    term: Terminator::Ret(None),
                }],
                current_origin: None,
            }],
            ..Default::default()
        };
        let stats = hoist_fp_consts(&mut module);
        assert_eq!(stats.consts_hoisted, 0);
        assert!(matches!(
            module.funcs[0].blocks[0].insts[0].kind,
            InstKind::BinOp(_, _, Operand::ConstF64(_))
        ));
    }
}
