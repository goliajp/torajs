//! Wide integer-constant loop hoisting — the integer twin of
//! [`crate::fconst_hoist`], from the rotation 623 fill-loop reading.
//!
//! `icmp x, #imm` encodes only 0..4096 as the CMP-immediate alias;
//! any other `ConstI64` operand of a compare is rematerialized at
//! every use (compile/cmp.rs — a `movz` / `movk` pair for a
//! 10,000,000 bound). The pre-reserved fill loop's 12-instruction body
//! spends 2 of them rebuilding its trip count on every iteration,
//! and every counted loop over a wide literal bound pays the same.
//!
//! This pass gives each such constant a value: for every natural
//! loop, each distinct non-encodable `ConstI64` used by a loop-body
//! `ICmp` is minted once as `Copy(I64, ConstI64(c))` at the tail of
//! the loop header's immediate dominator, and the compares rewrite to
//! the minted value. Register allocation keeps it resident across the
//! loop; a spill decision degrades to a 1-inst `ldr`, still cheaper
//! than the pair. Only `ICmp` operands are rewritten — the one shape
//! the reading observed; a `BinOp` immediate has its own encodings
//! (`add #imm12`, shifted forms) that a blanket rewrite would lose.
//! Loops whose header has no strict dominator outside the body are
//! skipped, as in the f64 twin.

use std::collections::HashMap;

use torajs_core::ssa::{Function, Inst, InstKind, Module, Operand, Type, ValueId, ValueInfo};

use crate::dominator::DominatorTree;
use crate::loop_analysis::LoopAnalysis;

#[derive(Debug, Default)]
pub struct IconstHoistStats {
    /// Distinct (loop, constant) pairs minted as a preheader Copy.
    pub consts_hoisted: u32,
    /// ConstI64 compare operands rewritten to a minted value.
    pub uses_rewritten: u32,
}

/// The CMP-immediate alias covers exactly this range (compile/cmp.rs).
fn cmp_imm_encodes(c: i64) -> bool {
    (0..4096).contains(&c)
}

pub fn hoist_int_consts(module: &mut Module) -> IconstHoistStats {
    let mut stats = IconstHoistStats::default();
    for func in module.funcs.iter_mut() {
        if func.is_declaration() {
            continue;
        }
        hoist_fn(func, &mut stats);
    }
    stats
}

fn hoist_fn(func: &mut Function, stats: &mut IconstHoistStats) {
    let dom = DominatorTree::compute(func);
    let loops = LoopAnalysis::compute(func, &dom);
    let n = func.blocks.len();
    for lp in loops.loops().to_vec() {
        let Some(pre) = dom.immediate_dominator(lp.header) else {
            continue;
        };
        if pre == lp.header || lp.body.contains(&pre) || pre.0 as usize >= n {
            continue;
        }
        let mut seen: Vec<i64> = Vec::new();
        for &b in &lp.body {
            let bi = b.0 as usize;
            if bi >= n {
                continue;
            }
            for inst in &func.blocks[bi].insts {
                let mut kind = inst.kind.clone();
                rewrite_cmp(&mut kind, |c| {
                    if !seen.contains(&c) {
                        seen.push(c);
                    }
                    Operand::ConstI64(c)
                });
            }
        }
        if seen.is_empty() {
            continue;
        }
        let mut minted: HashMap<i64, ValueId> = HashMap::new();
        for c in &seen {
            let vid = ValueId(func.values.len() as u32);
            func.values.push(ValueInfo {
                ty: Type::I64,
                name: Some(format!("iconst_{}", vid.0)),
            });
            func.blocks[pre.0 as usize].insts.push(Inst {
                result: Some(vid),
                kind: InstKind::Copy(Type::I64, Operand::ConstI64(*c)),
                origin: None,
            });
            minted.insert(*c, vid);
            stats.consts_hoisted += 1;
        }
        for &b in &lp.body {
            let bi = b.0 as usize;
            if bi >= n {
                continue;
            }
            for inst in &mut func.blocks[bi].insts {
                stats.uses_rewritten += rewrite_cmp(&mut inst.kind, |c| Operand::Value(minted[&c]));
            }
        }
    }
}

/// Rewrite every non-encodable `ConstI64` operand of an `ICmp`
/// through `lookup`; returns how many operands were visited.
fn rewrite_cmp(kind: &mut InstKind, mut lookup: impl FnMut(i64) -> Operand) -> u32 {
    let mut hits = 0;
    if let InstKind::ICmp(_, a, b) = kind {
        for op in [a, b] {
            if let Operand::ConstI64(c) = op
                && !cmp_imm_encodes(*c)
            {
                *op = lookup(*c);
                hits += 1;
            }
        }
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{BinOp, Block, BlockId, IPred, Terminator};

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
    /// b0(entry) → b1(header: icmp %v0 < 10_000_000, CondBr b2/b3) →
    /// b2(body: %2 = add %v0 1, Br b1) / b3(exit).
    fn counted_loop(bound: i64) -> Module {
        Module {
            funcs: vec![Function {
                name: "f".into(),
                params: vec![],
                ret: Type::I64,
                values: vec![vi(Type::I64), vi(Type::Bool), vi(Type::I64)],
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
                            InstKind::ICmp(
                                IPred::Slt,
                                Operand::Value(ValueId(0)),
                                Operand::ConstI64(bound),
                            ),
                        )],
                        term: Terminator::CondBr {
                            cond: Operand::Value(ValueId(1)),
                            then_blk: BlockId(2),
                            else_blk: BlockId(3),
                        },
                    },
                    Block {
                        id: BlockId(2),
                        insts: vec![mk(
                            2,
                            InstKind::BinOp(
                                BinOp::Add,
                                Operand::Value(ValueId(0)),
                                Operand::ConstI64(1),
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
            }],
            ..Default::default()
        }
    }

    #[test]
    fn a_wide_loop_bound_hoists_to_the_dominator() {
        let mut module = counted_loop(10_000_000);
        let stats = hoist_int_consts(&mut module);
        assert_eq!(stats.consts_hoisted, 1);
        assert_eq!(stats.uses_rewritten, 1);
        let f = &module.funcs[0];
        assert_eq!(f.blocks[0].insts.len(), 1);
        let minted = f.blocks[0].insts[0].result.unwrap();
        assert!(matches!(
            f.blocks[0].insts[0].kind,
            InstKind::Copy(Type::I64, Operand::ConstI64(10_000_000))
        ));
        assert!(matches!(
            f.blocks[1].insts[0].kind,
            InstKind::ICmp(_, _, Operand::Value(v)) if v == minted
        ));
        // The body's `add 1` keeps its encodable immediate.
        assert!(matches!(
            f.blocks[2].insts[0].kind,
            InstKind::BinOp(BinOp::Add, _, Operand::ConstI64(1))
        ));
    }

    #[test]
    fn an_encodable_bound_stays_inline() {
        let mut module = counted_loop(100);
        let stats = hoist_int_consts(&mut module);
        assert_eq!(stats.consts_hoisted, 0);
        assert!(matches!(
            module.funcs[0].blocks[1].insts[0].kind,
            InstKind::ICmp(_, _, Operand::ConstI64(100))
        ));
    }
}
