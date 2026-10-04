use super::*;
use torajs_core::ssa::{Block, BlockId, Inst, Type, ValueInfo};

fn val(r: u32, kind: InstKind) -> Inst {
    Inst {
        result: Some(ValueId(r)),
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

fn v(n: u32) -> Operand {
    Operand::Value(ValueId(n))
}

fn block(id: u32, insts: Vec<Inst>, term: Terminator) -> Block {
    Block {
        id: BlockId(id),
        insts,
        term,
    }
}

fn func(blocks: Vec<Block>, nvals: usize) -> Function {
    Function {
        name: "f".into(),
        params: vec![],
        ret: Type::Void,
        blocks,
        values: (0..nvals)
            .map(|_| ValueInfo {
                ty: Type::Ptr,
                name: None,
            })
            .collect(),
        current_origin: None,
    }
}

fn run(mut f: Function) -> (Function, FrameCellCleanStats) {
    let mut stats = FrameCellCleanStats::default();
    clean_in_function(&mut f, &mut stats);
    (f, stats)
}

/// The inlined release's entry: `p = id cell; p == null || p == UNDEF`.
fn release_entry(cmp_on: Operand) -> Function {
    func(
        vec![
            block(
                0,
                vec![
                    val(0, InstKind::AllocaBytes(48)),
                    val(1, InstKind::Identity(v(0))),
                    val(5, InstKind::Alloca(Type::Ptr)),
                    void(InstKind::Store(v(1), v(5), 0)),
                    val(6, InstKind::Load(Type::Ptr, v(5), 0)),
                    val(2, InstKind::ICmp(IPred::Eq, cmp_on, Operand::ConstPtrNull)),
                    val(3, InstKind::GlobalRef("___TORAJS_UNDEF_CELL".into())),
                    val(4, InstKind::ICmp(IPred::Eq, cmp_on, v(3))),
                    val(7, InstKind::BinOp(BinOp::Or, v(2), v(4))),
                ],
                Terminator::CondBr {
                    cond: v(7),
                    then_blk: BlockId(2),
                    else_blk: BlockId(1),
                },
            ),
            block(
                1,
                vec![void(InstKind::Store(Operand::ConstI64(0), v(1), 0))],
                Terminator::Br(BlockId(2)),
            ),
            block(2, vec![], Terminator::Ret(None)),
        ],
        8,
    )
}

#[test]
fn copy_of_frame_cell_compares_fold_and_release_runs() {
    let (f, stats) = run(release_entry(v(1)));
    assert!(matches!(f.blocks[0].term, Terminator::Br(BlockId(1))));
    assert_eq!(stats.copies_resolved, 1);
    assert_eq!(stats.values_folded, 3);
    // the store through the copy now names the cell itself
    assert!(matches!(
        f.blocks[1].insts[0].kind,
        InstKind::Store(Operand::ConstI64(0), Operand::Value(ValueId(0)), 0)
    ));
    let left: Vec<_> = f.blocks[0].insts.iter().map(|i| i.result).collect();
    assert!(!left.contains(&Some(ValueId(2))) && !left.contains(&Some(ValueId(3))));
}

#[test]
fn pointer_loaded_from_a_slot_may_be_null() {
    let (f, stats) = run(release_entry(v(6)));
    assert!(matches!(f.blocks[0].term, Terminator::CondBr { .. }));
    assert_eq!(stats.values_folded, 0);
}

#[test]
fn two_frame_cells_are_not_compared() {
    let f = func(
        vec![
            block(
                0,
                vec![
                    val(0, InstKind::AllocaBytes(48)),
                    val(1, InstKind::AllocaBytes(48)),
                    val(2, InstKind::ICmp(IPred::Eq, v(0), v(1))),
                ],
                Terminator::CondBr {
                    cond: v(2),
                    then_blk: BlockId(1),
                    else_blk: BlockId(2),
                },
            ),
            block(1, vec![], Terminator::Ret(None)),
            block(2, vec![], Terminator::Ret(None)),
        ],
        3,
    );
    let (f, stats) = run(f);
    assert!(matches!(f.blocks[0].term, Terminator::CondBr { .. }));
    assert_eq!(stats.values_folded, 0);
}

#[test]
fn empty_arm_that_joins_the_other_arm_folds() {
    let f = func(
        vec![
            block(
                0,
                vec![
                    val(0, InstKind::AllocaBytes(48)),
                    val(1, InstKind::GlobalRef("___torajs_weakref_active".into())),
                    val(2, InstKind::Load(Type::I64, v(1), 0)),
                    val(3, InstKind::ICmp(IPred::Ne, v(2), Operand::ConstI64(0))),
                ],
                Terminator::CondBr {
                    cond: v(3),
                    then_blk: BlockId(1),
                    else_blk: BlockId(2),
                },
            ),
            block(1, vec![], Terminator::Br(BlockId(2))),
            block(2, vec![], Terminator::Ret(None)),
        ],
        4,
    );
    let (f, stats) = run(f);
    assert!(matches!(f.blocks[0].term, Terminator::Br(BlockId(2))));
    assert_eq!(stats.branches_folded, 1);
    // the compare, the load of the flag and its address all go
    assert_eq!(f.blocks[0].insts.len(), 1);
}
