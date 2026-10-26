//! The subtract a range test mints, reused by the blocks after it.
//!
//! `c >= 48 && c <= 57` folds to `d = c - 48; d <u 10` (`range_fold`),
//! and the body it guards usually computes `c - 48` again — the digit's
//! value. The two are the same number wherever no path from the test
//! to the second subtract redefines `c`. `late_gvn` would merge them,
//! but it runs before select formation mints the first one. Here the
//! blocks entered only from the one before them, starting at the test,
//! are walked in order until `c` is redefined; a `c - 48` met on the
//! way gives its readers the test's value and goes.

use torajs_core::ssa::{
    BinOp, Function, InstKind, Operand, Terminator, ValueId, rewrite_value_operands,
};

use crate::id_hash::HashMap;

use super::arm::reachable;

/// Reuse every `x - k` minted at or after value id `first` (the range
/// tests of this function's formation rounds); returns the number of
/// subtracts removed.
pub(super) fn reuse_shifts(func: &mut Function, first: u32) -> u32 {
    let minted: Vec<(usize, ValueId, ValueId, i64)> = func
        .blocks
        .iter()
        .enumerate()
        .flat_map(|(b, block)| block.insts.iter().map(move |i| (b, i)))
        .filter_map(|(b, i)| match (i.result, &i.kind) {
            (Some(s), InstKind::BinOp(BinOp::Sub, Operand::Value(x), Operand::ConstI64(k)))
                if s.0 >= first =>
            {
                Some((b, s, *x, *k))
            }
            _ => None,
        })
        .collect();
    if minted.is_empty() {
        return 0;
    }
    let preds = reachable_pred_counts(func);
    let defs = def_counts(func);
    let mut replace: HashMap<ValueId, ValueId> = HashMap::default();
    for (h, s, x, k) in minted {
        // a minted subtract an earlier one already replaced is gone
        let Some(at) = func.blocks[h]
            .insts
            .iter()
            .position(|i| i.result == Some(s))
        else {
            continue;
        };
        if defs[s.0 as usize] != 1 {
            continue;
        }
        let mut work = vec![(h, at + 1)];
        while let Some((b, from)) = work.pop() {
            let mut redefined = false;
            let block = &mut func.blocks[b];
            let mut i = from;
            while i < block.insts.len() {
                let inst = &block.insts[i];
                let dup = matches!(
                    (&inst.kind, inst.result),
                    (InstKind::BinOp(BinOp::Sub, Operand::Value(y), Operand::ConstI64(j)), Some(r))
                        if *y == x && *j == k && defs[r.0 as usize] == 1
                );
                if dup {
                    let r = block.insts.remove(i).result.expect("matched above");
                    replace.insert(r, s);
                    continue;
                }
                if inst.result == Some(x) {
                    redefined = true;
                    break;
                }
                i += 1;
            }
            if redefined {
                continue;
            }
            let next: Vec<usize> = match &block.term {
                Terminator::Br(t) => vec![t.0 as usize],
                Terminator::CondBr {
                    then_blk, else_blk, ..
                } => vec![then_blk.0 as usize, else_blk.0 as usize],
                Terminator::Ret(_) | Terminator::Unreachable => vec![],
            };
            work.extend(
                next.into_iter()
                    .filter(|&n| n != h && preds.get(n) == Some(&1))
                    .map(|n| (n, 0)),
            );
        }
    }
    if replace.is_empty() {
        return 0;
    }
    let to = |v: ValueId| replace.get(&v).copied().unwrap_or(v);
    for block in &mut func.blocks {
        for inst in &mut block.insts {
            rewrite_value_operands(&mut inst.kind, to);
        }
        match &mut block.term {
            Terminator::CondBr {
                cond: Operand::Value(v),
                ..
            }
            | Terminator::Ret(Some(Operand::Value(v))) => *v = to(*v),
            _ => {}
        }
    }
    replace.len() as u32
}

/// Predecessor counts over the blocks reachable from the entry: the
/// arms a conversion emptied still branch on to their old join.
fn reachable_pred_counts(func: &Function) -> Vec<u32> {
    let live = reachable(func);
    let mut preds = vec![0u32; func.blocks.len()];
    for block in func.blocks.iter().filter(|b| live[b.id.0 as usize]) {
        match &block.term {
            Terminator::Br(t) => preds[t.0 as usize] += 1,
            Terminator::CondBr {
                then_blk, else_blk, ..
            } => {
                preds[then_blk.0 as usize] += 1;
                if else_blk != then_blk {
                    preds[else_blk.0 as usize] += 1;
                }
            }
            _ => {}
        }
    }
    preds
}

fn def_counts(func: &Function) -> Vec<u32> {
    let mut defs = vec![0u32; func.values.len()];
    for inst in func.blocks.iter().flat_map(|b| &b.insts) {
        if let Some(r) = inst.result {
            defs[r.0 as usize] += 1;
        }
    }
    defs
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{Block, BlockId, IPred, Inst, Type, ValueInfo};

    fn mk(result: u32, kind: InstKind) -> Inst {
        Inst {
            result: Some(ValueId(result)),
            kind,
            origin: None,
        }
    }

    fn v(id: u32) -> Operand {
        Operand::Value(ValueId(id))
    }

    fn sub48(r: u32, x: u32) -> Inst {
        mk(r, InstKind::BinOp(BinOp::Sub, v(x), Operand::ConstI64(48)))
    }

    /// b0: %1 = sub %0, 48 (minted); %2 = icmp ult %1, 10; cond_br %2, b1, b2
    /// b1: [redef] %3 = sub %0, 48; ret %3
    /// b2: ret 0
    fn digit(redef: bool) -> Function {
        let mut body = vec![];
        if redef {
            body.push(mk(0, InstKind::Copy(Type::I64, Operand::ConstI64(7))));
        }
        body.push(sub48(3, 0));
        Function {
            name: "f".into(),
            params: vec![],
            ret: Type::I64,
            values: (0..4)
                .map(|i| ValueInfo {
                    ty: if i == 2 { Type::Bool } else { Type::I64 },
                    name: None,
                })
                .collect(),
            blocks: vec![
                Block {
                    id: BlockId(0),
                    insts: vec![
                        sub48(1, 0),
                        mk(2, InstKind::ICmp(IPred::Ult, v(1), Operand::ConstI64(10))),
                    ],
                    term: Terminator::CondBr {
                        cond: v(2),
                        then_blk: BlockId(1),
                        else_blk: BlockId(2),
                    },
                },
                Block {
                    id: BlockId(1),
                    insts: body,
                    term: Terminator::Ret(Some(v(3))),
                },
                Block {
                    id: BlockId(2),
                    insts: vec![],
                    term: Terminator::Ret(Some(Operand::ConstI64(0))),
                },
            ],
            current_origin: None,
        }
    }

    #[test]
    fn the_body_reads_the_tests_subtract() {
        let mut f = digit(false);
        assert_eq!(reuse_shifts(&mut f, 1), 1);
        assert!(f.blocks[1].insts.is_empty());
        assert!(matches!(f.blocks[1].term, Terminator::Ret(Some(o)) if o == v(1)));
    }

    #[test]
    fn a_redefined_subject_keeps_its_own_subtract() {
        let mut f = digit(true);
        assert_eq!(reuse_shifts(&mut f, 1), 0);
        assert_eq!(f.blocks[1].insts.len(), 2);
    }

    #[test]
    fn a_subtract_from_before_the_formation_is_not_a_source() {
        let mut f = digit(false);
        assert_eq!(reuse_shifts(&mut f, 2), 0);
    }
}
