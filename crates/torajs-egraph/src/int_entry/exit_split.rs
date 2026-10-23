//! The f64 half of an [`exit_test`](super::exit_test) taken off the
//! path the integer half decides:
//!
//! ```text
//! B: %f = icmp ...                       B: %f = icmp ...
//!    %x = sitofp %i                         cond_br %f, T, R
//!    %c = fcmp %x, %n               →    R: %x = sitofp %i
//!    %o = or %f, %c                         %c = fcmp %x, %n
//!    cond_br %o, T, E                       cond_br %c, T, E
//! ```
//!
//! Until here every pass saw one test where the loop had one: the
//! hoisting and rotation passes read the loop by it. So this runs after
//! the last of them (`concat_licm`), on every block of the shape — the
//! rotated latch and the guard in front of the loop.
//! `TORAJS_EXIT_SPLIT_OFF=1` skips.

use torajs_core::ssa::{
    BinOp, Block, BlockId, Function, Inst, InstKind, Module, Operand, Terminator, ValueId,
};

use super::exit_test::Reads;
use crate::cfg_edit::new_block;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ExitSplitStats {
    pub tests_split: u32,
}

pub fn split_exit_tests(module: &mut Module) -> ExitSplitStats {
    let mut stats = ExitSplitStats::default();
    for func in module.funcs.iter_mut().filter(|f| !f.is_declaration()) {
        // only `exit_test` makes the shape: most functions have none
        let ors = |b: &Block| {
            matches!((b.insts.last(), &b.term), (Some(Inst { result: Some(o), kind: InstKind::BinOp(BinOp::Or, ..), .. }),
                Terminator::CondBr { cond: Operand::Value(c), .. }) if o == c)
        };
        if !func.blocks.iter().any(ors) {
            continue;
        }
        let reads = Reads::of(func);
        for b in 0..func.blocks.len() {
            if split(func, BlockId(b as u32), &reads) {
                stats.tests_split += 1;
            }
        }
    }
    stats
}

/// The block's last three instructions are `sitofp`, the `fcmp` reading
/// it and the `or` of an earlier test with that, each read once.
fn split(func: &mut Function, b: BlockId, reads: &Reads) -> bool {
    let block = &func.blocks[b.0 as usize];
    let Terminator::CondBr {
        cond: Operand::Value(o),
        then_blk,
        else_blk,
    } = block.term
    else {
        return false;
    };
    let n = block.insts.len();
    if n < 3 {
        return false;
    }
    let [x, c, or] = [
        &block.insts[n - 3],
        &block.insts[n - 2],
        &block.insts[n - 1],
    ];
    let once = |v: Option<ValueId>| v.is_some_and(|v| reads.once(v));
    let compares_image = matches!(c.kind, InstKind::FCmp(_, Operand::Value(l), Operand::Value(r))
        if Some(l) == x.result || Some(r) == x.result);
    let fast = match or.kind {
        InstKind::BinOp(BinOp::Or, fast @ Operand::Value(_), Operand::Value(t))
            if Some(t) == c.result =>
        {
            fast
        }
        _ => return false,
    };
    let shaped = matches!(x.kind, InstKind::SiToFp(_))
        && compares_image
        && or.result == Some(o)
        && [x.result, c.result, or.result].into_iter().all(once);
    if !shaped {
        return false;
    }
    let test = c
        .result
        .map(Operand::Value)
        .expect("a compare defines a value");
    let mut moved = func.blocks[b.0 as usize].insts.split_off(n - 3);
    moved.pop();
    let rest = new_block(func);
    func.blocks[rest.0 as usize].insts = moved;
    func.blocks[rest.0 as usize].term = Terminator::CondBr {
        cond: test,
        then_blk,
        else_blk,
    };
    func.blocks[b.0 as usize].term = Terminator::CondBr {
        cond: fast,
        then_blk,
        else_blk: rest,
    };
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{Block, FPred, IPred, Inst, Type, ValueInfo};

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

    /// bb0: %2 = icmp slt %0, %1; %3 = sitofp %0; %4 = fcmp olt %3, %5;
    /// %6 = or %2, %4; cond_br %6, bb1, bb2
    fn guarded(extra_read: bool) -> Module {
        let tys = [
            Type::I64,
            Type::I64,
            Type::Bool,
            Type::F64,
            Type::Bool,
            Type::F64,
            Type::Bool,
        ];
        let ret = if extra_read { Some(v(4)) } else { None };
        let f = Function {
            name: "f".into(),
            params: vec![ValueId(0), ValueId(1), ValueId(5)],
            ret: Type::I64,
            blocks: vec![
                Block {
                    id: BlockId(0),
                    insts: vec![
                        inst(2, InstKind::ICmp(IPred::Slt, v(0), v(1))),
                        inst(3, InstKind::SiToFp(v(0))),
                        inst(4, InstKind::FCmp(FPred::Olt, v(3), v(5))),
                        inst(6, InstKind::BinOp(BinOp::Or, v(2), v(4))),
                    ],
                    term: Terminator::CondBr {
                        cond: v(6),
                        then_blk: BlockId(1),
                        else_blk: BlockId(2),
                    },
                },
                Block {
                    id: BlockId(1),
                    insts: vec![],
                    term: Terminator::Ret(ret),
                },
                Block {
                    id: BlockId(2),
                    insts: vec![],
                    term: Terminator::Ret(None),
                },
            ],
            values: tys
                .iter()
                .map(|ty| ValueInfo {
                    ty: *ty,
                    name: None,
                })
                .collect(),
            current_origin: None,
        };
        Module {
            funcs: vec![f],
            ..Module::default()
        }
    }

    #[test]
    fn the_f64_test_runs_only_where_the_integer_one_fails() {
        let mut m = guarded(false);
        assert_eq!(split_exit_tests(&mut m).tests_split, 1);
        let f = &m.funcs[0];
        assert_eq!(f.blocks[0].insts.len(), 1);
        assert_eq!(
            f.blocks[0].term,
            Terminator::CondBr {
                cond: v(2),
                then_blk: BlockId(1),
                else_blk: BlockId(3),
            }
        );
        assert!(matches!(f.blocks[3].insts[1].kind, InstKind::FCmp(..)));
        assert_eq!(
            f.blocks[3].term,
            Terminator::CondBr {
                cond: v(4),
                then_blk: BlockId(1),
                else_blk: BlockId(2),
            }
        );
    }

    #[test]
    fn a_test_read_elsewhere_stays_where_it_is() {
        let mut m = guarded(true);
        assert_eq!(split_exit_tests(&mut m).tests_split, 0);
    }
}
