use super::*;
use torajs_core::ssa::{Block, BlockId, Inst, Type, ValueInfo};

fn copy(r: u32, src: Operand) -> Inst {
    Inst {
        result: Some(ValueId(r)),
        kind: InstKind::Copy(Type::I64, src),
        origin: None,
    }
}

/// `%0` is read in bb0, then written again in bb1 where nothing reads
/// it: the write still lands in `%0`'s register, so its interval must
/// reach that slot or the allocator may hand the register to `%1`,
/// which is live across it.
#[test]
fn dead_redefinition_stays_inside_the_interval() {
    let i64v = || ValueInfo {
        ty: Type::I64,
        name: None,
    };
    let func = Function {
        name: "deaddef".into(),
        params: Vec::new(),
        ret: Type::I64,
        values: vec![i64v(), i64v()],
        blocks: vec![
            Block {
                id: BlockId(0),
                insts: vec![
                    copy(0, Operand::ConstI64(5)),
                    copy(1, Operand::Value(ValueId(0))),
                ],
                term: Terminator::Br(BlockId(1)),
            },
            Block {
                id: BlockId(1),
                insts: vec![copy(0, Operand::ConstI64(7))],
                term: Terminator::Ret(Some(Operand::Value(ValueId(1)))),
            },
        ],
        current_origin: None,
    };
    let ivs = compute_intervals(&func);
    // slots: bb0 insts 0, 1, term 2; bb1 inst 3, term 4
    assert_eq!(ivs[&0], Interval { start: 0, end: 3 });
    assert!(ivs[&0].overlaps(&ivs[&1]));
}

fn add(r: u32, a: Operand, b: Operand) -> Inst {
    Inst {
        result: Some(ValueId(r)),
        kind: InstKind::BinOp(torajs_core::ssa::BinOp::Add, a, b),
        origin: None,
    }
}

/// The dead write sits in a loop latch, ahead of the back edge: `%0`'s
/// interval must reach it and so overlap `%2`, which the latch still
/// reads after that slot.
#[test]
fn dead_redefinition_in_a_latch_overlaps_the_loop_value() {
    let ty = |ty| ValueInfo { ty, name: None };
    let v = |n| Operand::Value(ValueId(n));
    let func = Function {
        name: "deadlatch".into(),
        params: Vec::new(),
        ret: Type::I64,
        values: vec![
            ty(Type::I64),
            ty(Type::I64),
            ty(Type::I64),
            ty(Type::Bool),
            ty(Type::I64),
        ],
        blocks: vec![
            Block {
                id: BlockId(0),
                insts: vec![
                    copy(0, Operand::ConstI64(5)),
                    add(4, v(0), Operand::ConstI64(1)),
                    copy(1, Operand::ConstI64(0)),
                ],
                term: Terminator::Br(BlockId(1)),
            },
            Block {
                id: BlockId(1),
                insts: vec![
                    add(2, v(1), Operand::ConstI64(1)),
                    Inst {
                        result: Some(ValueId(3)),
                        kind: InstKind::ICmp(
                            torajs_core::ssa::IPred::Slt,
                            v(2),
                            Operand::ConstI64(10),
                        ),
                        origin: None,
                    },
                ],
                term: Terminator::CondBr {
                    cond: v(3),
                    then_blk: BlockId(2),
                    else_blk: BlockId(3),
                },
            },
            Block {
                id: BlockId(2),
                insts: vec![copy(0, Operand::ConstI64(7)), copy(1, v(2))],
                term: Terminator::Br(BlockId(1)),
            },
            Block {
                id: BlockId(3),
                insts: Vec::new(),
                term: Terminator::Ret(Some(v(4))),
            },
        ],
        current_origin: None,
    };
    let ivs = compute_intervals(&func);
    // bb2's dead `%0 = copy 7` is slot 7
    assert!(ivs[&0].contains(7), "{:?}", ivs[&0]);
    assert!(ivs[&0].overlaps(&ivs[&2]));
}
