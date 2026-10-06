//! A small interpreter for the integer subset the tests build, so a
//! jammed function can be run against its original.

use crate::id_hash::HashMap;

use torajs_core::ssa::{BinOp, BlockId, Function, IPred, InstKind, Operand, Terminator, ValueId};

fn operand(env: &HashMap<ValueId, i64>, op: &Operand) -> i64 {
    match op {
        Operand::Value(x) => env[x],
        Operand::ConstI64(k) => *k,
        Operand::ConstBool(b) => *b as i64,
        other => panic!("operand {other:?} outside the test subset"),
    }
}

/// Run `f` on `args`; `None` when it does not return within `fuel`
/// instructions.
pub(super) fn run(f: &Function, args: &[i64], fuel: usize) -> Option<i64> {
    let mut env: HashMap<ValueId, i64> =
        f.params.iter().copied().zip(args.iter().copied()).collect();
    let mut b = BlockId(0);
    let mut left = fuel;
    loop {
        let block = &f.blocks[b.0 as usize];
        for i in &block.insts {
            left = left.checked_sub(1)?;
            let r = match &i.kind {
                InstKind::Copy(_, a) => operand(&env, a),
                InstKind::Select(_, c, a, bb) => {
                    if operand(&env, c) != 0 {
                        operand(&env, a)
                    } else {
                        operand(&env, bb)
                    }
                }
                InstKind::BinOp(op, a, bb) => {
                    let (x, y) = (operand(&env, a), operand(&env, bb));
                    match op {
                        BinOp::Add => x.wrapping_add(y),
                        BinOp::Sub => x.wrapping_sub(y),
                        BinOp::Mul => x.wrapping_mul(y),
                        BinOp::SRem => x.checked_rem(y).unwrap_or(x),
                        BinOp::And => x & y,
                        BinOp::AShr => x >> (y & 63),
                        other => panic!("{other:?} outside the test subset"),
                    }
                }
                InstKind::ICmp(p, a, bb) => {
                    let (x, y) = (operand(&env, a), operand(&env, bb));
                    (match p {
                        IPred::Eq => x == y,
                        IPred::Ne => x != y,
                        IPred::Slt => x < y,
                        IPred::Sle => x <= y,
                        IPred::Sgt => x > y,
                        IPred::Sge => x >= y,
                        other => panic!("{other:?} outside the test subset"),
                    }) as i64
                }
                other => panic!("{other:?} outside the test subset"),
            };
            env.insert(i.result.expect("value insts"), r);
        }
        b = match &block.term {
            Terminator::Br(t) => *t,
            Terminator::CondBr {
                cond,
                then_blk,
                else_blk,
            } => {
                if operand(&env, cond) != 0 {
                    *then_blk
                } else {
                    *else_blk
                }
            }
            Terminator::Ret(r) => return Some(operand(&env, r.as_ref().expect("ret value"))),
            Terminator::Unreachable => panic!("reached unreachable"),
        };
    }
}
