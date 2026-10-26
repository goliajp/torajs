use super::*;
use torajs_core::ssa::{BinOp, Block, BlockId, IPred, Type, ValueInfo};

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

fn v(id: u32) -> Operand {
    Operand::Value(ValueId(id))
}

fn cond_br(c: u32, t: u32, e: u32) -> Terminator {
    Terminator::CondBr {
        cond: v(c),
        then_blk: BlockId(t),
        else_blk: BlockId(e),
    }
}

/// b0: %1 = icmp uge %0, 10; br b1
/// b1: %3 = lshr %2, 49; %4 = icmp ne %3, 0; cond_br %4, b3, b2
/// b2: cond_br %1, b4, b5
/// b3: (cold side) cond_br %1, b4, b5 when `cold_reads`, else ret
/// b4 / b5: ret
fn guarded(cold_reads: bool) -> Function {
    let block = |id: u32, insts: Vec<Inst>, term: Terminator| Block {
        id: BlockId(id),
        insts,
        term,
    };
    Function {
        name: "f".into(),
        params: vec![],
        ret: Type::Void,
        values: vec![
            vi(Type::I64),
            vi(Type::Bool),
            vi(Type::I64),
            vi(Type::I64),
            vi(Type::Bool),
        ],
        blocks: vec![
            block(
                0,
                vec![mk(
                    1,
                    InstKind::ICmp(IPred::Uge, v(0), Operand::ConstI64(10)),
                )],
                Terminator::Br(BlockId(1)),
            ),
            block(
                1,
                vec![
                    mk(3, InstKind::BinOp(BinOp::LShr, v(2), Operand::ConstI64(49))),
                    mk(4, InstKind::ICmp(IPred::Ne, v(3), Operand::ConstI64(0))),
                ],
                cond_br(4, 3, 2),
            ),
            block(2, vec![], cond_br(1, 4, 5)),
            block(
                3,
                vec![],
                if cold_reads {
                    cond_br(1, 4, 5)
                } else {
                    Terminator::Ret(None)
                },
            ),
            block(4, vec![], Terminator::Ret(None)),
            block(5, vec![], Terminator::Ret(None)),
        ],
        current_origin: None,
    }
}

fn run(func: Function) -> (Function, GuardCmpSinkStats) {
    let mut module = Module {
        funcs: vec![func],
        ..Default::default()
    };
    let stats = sink_guarded_cmps(&mut module);
    (module.funcs.pop().unwrap(), stats)
}

#[test]
fn the_compare_moves_under_the_guard_and_the_cold_side_recomputes_it() {
    let (f, stats) = run(guarded(true));
    assert_eq!(stats.sunk, 1);
    assert_eq!(stats.recomputed, 1);
    assert!(f.blocks[0].insts.is_empty());
    let moved = &f.blocks[2].insts[0];
    assert!(matches!(
        moved.kind,
        InstKind::ICmp(IPred::Uge, _, Operand::ConstI64(10))
    ));
    let fresh = moved.result.unwrap();
    assert_ne!(fresh, ValueId(1));
    assert!(
        matches!(f.blocks[2].term, Terminator::CondBr { cond: Operand::Value(c), .. } if c == fresh)
    );
    assert_eq!(f.blocks[3].insts[0].result, Some(ValueId(1)));
}

#[test]
fn a_cold_side_that_does_not_read_the_flag_gets_no_copy() {
    let (f, stats) = run(guarded(false));
    assert_eq!(stats.sunk, 1);
    assert_eq!(stats.recomputed, 0);
    assert!(f.blocks[3].insts.is_empty());
}

#[test]
fn a_flag_read_after_the_branch_stays_where_it_is() {
    let mut func = guarded(true);
    func.blocks[4].term = Terminator::Ret(Some(v(1)));
    let (f, stats) = run(func);
    assert_eq!(stats.sunk, 0);
    assert_eq!(f.blocks[0].insts.len(), 1);
}

#[test]
fn a_guard_that_redefines_an_operand_blocks_the_move() {
    let mut func = guarded(true);
    func.blocks[1].insts.insert(
        0,
        mk(0, InstKind::BinOp(BinOp::Add, v(2), Operand::ConstI64(1))),
    );
    let (f, stats) = run(func);
    assert_eq!(stats.sunk, 0);
    assert_eq!(f.blocks[0].insts.len(), 1);
}
