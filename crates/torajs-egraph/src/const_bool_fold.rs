//! Boolean constants folded through their readers.
//!
//! A pass that answers a compare with a constant (`merge_sroa`: a stack
//! cell is never null) leaves the readers of that answer in place: the
//! `or` of two such answers, and the conditional that branches on it.
//! The egraph ran before those passes and nothing after it folds a
//! boolean operation of constants, so the loop that dropped a merged
//! step object still computed `false || false` on every iteration and
//! branched on it (objlit-optional-field-1m: four instructions and a
//! taken branch per iteration). Substitute the constants into their
//! readers, fold the all-constant boolean operations that result, turn
//! a conditional on a constant into a plain branch, and drop the
//! blocks nothing reaches any more.

use crate::id_hash::HashMap;

use torajs_core::ssa::{BinOp, Function, InstKind, Operand, Terminator, ValueId};

use crate::branch_fold::sweep_unreachable_blocks;
use crate::slot_forward::{resolve, rewrite_operands};

/// What folding `replace` through `func` removed.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ConstBoolFold {
    /// Boolean operations of two constants.
    pub ops: u32,
    /// Conditionals on a constant, now plain branches.
    pub branches: u32,
    /// Blocks only the folded-away arms reached.
    pub blocks: u32,
}

/// Every read of a value in `replace` reads its constant instead; a
/// boolean operation of two constants becomes a constant for its own
/// readers, until nothing more folds.
pub(crate) fn fold_constant_bools(
    func: &mut Function,
    mut replace: HashMap<ValueId, Operand>,
) -> ConstBoolFold {
    let mut stats = ConstBoolFold::default();
    while !replace.is_empty() {
        let mut next: HashMap<ValueId, Operand> = HashMap::default();
        for block in func.blocks.iter_mut() {
            block.insts.retain_mut(|inst| {
                rewrite_operands(&mut inst.kind, &replace);
                let (InstKind::BinOp(op, Operand::ConstBool(a), Operand::ConstBool(b)), Some(r)) =
                    (&inst.kind, inst.result)
                else {
                    return true;
                };
                let v = match op {
                    BinOp::And => a & b,
                    BinOp::Or => a | b,
                    BinOp::Xor => a ^ b,
                    _ => return true,
                };
                next.insert(r, Operand::ConstBool(v));
                stats.ops += 1;
                false
            });
            match &mut block.term {
                Terminator::CondBr { cond, .. } => *cond = resolve(cond, &replace),
                Terminator::Ret(Some(op)) => *op = resolve(op, &replace),
                _ => {}
            }
        }
        replace = next;
    }
    for block in func.blocks.iter_mut() {
        if let Terminator::CondBr {
            cond: Operand::ConstBool(c),
            then_blk,
            else_blk,
        } = block.term
        {
            block.term = Terminator::Br(if c { then_blk } else { else_blk });
            stats.branches += 1;
        }
    }
    if stats.branches > 0 {
        stats.blocks = sweep_unreachable_blocks(func);
    }
    stats
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{Block, BlockId, Inst, Type, ValueInfo};

    fn inst(result: u32, kind: InstKind) -> Inst {
        Inst {
            result: Some(ValueId(result)),
            kind,
            origin: None,
        }
    }

    fn v(i: u32) -> Operand {
        Operand::Value(ValueId(i))
    }

    /// b0: `%2 = or %0, %1; cond_br %2, b1, b2`; b1 returns 1, b2
    /// returns 0.
    fn func() -> Function {
        Function {
            name: "f".into(),
            params: vec![],
            ret: Type::I64,
            blocks: vec![
                Block {
                    id: BlockId(0),
                    insts: vec![inst(2, InstKind::BinOp(BinOp::Or, v(0), v(1)))],
                    term: Terminator::CondBr {
                        cond: v(2),
                        then_blk: BlockId(1),
                        else_blk: BlockId(2),
                    },
                },
                Block {
                    id: BlockId(1),
                    insts: vec![],
                    term: Terminator::Ret(Some(Operand::ConstI64(1))),
                },
                Block {
                    id: BlockId(2),
                    insts: vec![],
                    term: Terminator::Ret(Some(Operand::ConstI64(0))),
                },
            ],
            values: (0..3)
                .map(|_| ValueInfo {
                    ty: Type::Bool,
                    name: None,
                })
                .collect(),
            current_origin: None,
        }
    }

    #[test]
    fn two_false_answers_fold_the_or_and_the_branch() {
        let mut f = func();
        let replace = [
            (ValueId(0), Operand::ConstBool(false)),
            (ValueId(1), Operand::ConstBool(false)),
        ]
        .into_iter()
        .collect();
        let st = fold_constant_bools(&mut f, replace);
        assert_eq!(
            st,
            ConstBoolFold {
                ops: 1,
                branches: 1,
                blocks: 1
            }
        );
        assert_eq!(f.blocks.len(), 2);
        assert!(f.blocks[0].insts.is_empty());
        assert!(matches!(f.blocks[0].term, Terminator::Br(BlockId(1))));
        assert!(matches!(
            f.blocks[1].term,
            Terminator::Ret(Some(Operand::ConstI64(0)))
        ));
    }

    /// One answer still unknown: the `or` reads the constant, nothing
    /// else changes.
    #[test]
    fn a_half_known_or_only_reads_the_constant() {
        let mut f = func();
        let replace = [(ValueId(0), Operand::ConstBool(false))]
            .into_iter()
            .collect();
        let st = fold_constant_bools(&mut f, replace);
        assert_eq!(st, ConstBoolFold::default());
        assert_eq!(
            f.blocks[0].insts[0].kind,
            InstKind::BinOp(BinOp::Or, Operand::ConstBool(false), v(1))
        );
        assert_eq!(f.blocks.len(), 3);
    }
}
