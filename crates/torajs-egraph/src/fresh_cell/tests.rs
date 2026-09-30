use super::*;
use torajs_core::ssa::{BinOp, Block, IPred, Inst, Type, ValueInfo};

const B: Operand = Operand::Value(ValueId(0));
const GUARD_BITS: i32 = ((0x4000_u32 | 0x1_u32) << 16) as i32;

fn decl(name: &str) -> Function {
    Function {
        name: name.into(),
        params: vec![],
        ret: Type::Void,
        blocks: vec![],
        values: vec![],
        current_origin: None,
    }
}

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

fn block(id: u32, insts: Vec<Inst>, term: Terminator) -> Block {
    Block {
        id: BlockId(id),
        insts,
        term,
    }
}

fn alloc() -> Inst {
    val(0, InstKind::Call(FuncId(0), vec![Operand::ConstI64(48)]))
}

fn store(v: Operand, off: u64) -> Inst {
    void(InstKind::Store(v, B, off))
}

fn load(r: u32, ty: Type, off: u64) -> Inst {
    val(r, InstKind::Load(ty, B, off))
}

/// fids: 0 obj_alloc · 1 an opaque callee. Values 0..8: the cell, then
/// i32 / i64 scratch.
fn module_with(blocks: Vec<Block>) -> Module {
    let mut values = vec![
        ValueInfo {
            ty: Type::I64,
            name: None,
        };
        8
    ];
    values[0].ty = Type::Ptr;
    for v in [1, 2] {
        values[v].ty = Type::I32;
    }
    values[3].ty = Type::Bool;
    let main = Function {
        name: "main".into(),
        params: vec![],
        ret: Type::Void,
        blocks,
        values,
        current_origin: None,
    };
    Module {
        funcs: vec![decl(OBJ_ALLOC), decl("__torajs_other"), main],
        ..Default::default()
    }
}

fn run(blocks: Vec<Block>) -> (FreshCellStats, Function) {
    let mut m = module_with(blocks);
    let stats = forward_fresh_cells(&mut m);
    (stats, m.funcs.pop().unwrap())
}

fn one_block(insts: Vec<Inst>) -> (FreshCellStats, Function) {
    run(vec![block(0, insts, Terminator::Ret(None))])
}

fn kinds(f: &Function, b: usize) -> Vec<InstKind> {
    f.blocks[b].insts.iter().map(|i| i.kind.clone()).collect()
}

#[test]
fn header_word_forwards_and_default_store_dies() {
    let (stats, f) = one_block(vec![
        alloc(),
        store(Operand::ConstI32(1), 4),
        store(Operand::ConstI64(0), 32),
        load(1, Type::I32, 4),
        val(
            2,
            InstKind::BinOp(
                BinOp::And,
                Operand::Value(ValueId(1)),
                Operand::ConstI32(GUARD_BITS),
            ),
        ),
        store(Operand::ConstI64(7), 32),
        load(4, Type::I64, 32),
    ]);
    assert_eq!((stats.loads_forwarded, stats.stores_removed), (2, 1));
    assert_eq!(
        kinds(&f, 0),
        vec![
            InstKind::Call(FuncId(0), vec![Operand::ConstI64(48)]),
            InstKind::Store(Operand::ConstI32(1), B, 4),
            InstKind::BinOp(
                BinOp::And,
                Operand::ConstI32(1),
                Operand::ConstI32(GUARD_BITS)
            ),
            InstKind::Store(Operand::ConstI64(7), B, 32),
        ]
    );
}

#[test]
fn unforwarded_load_keeps_the_overwritten_store() {
    // a narrower read at the offset, and a read at another offset
    let (stats, f) = one_block(vec![
        alloc(),
        store(Operand::ConstI64(0), 32),
        load(1, Type::I32, 32),
        store(Operand::ConstI64(7), 32),
        store(Operand::ConstI64(0), 40),
        load(4, Type::I64, 48),
        store(Operand::ConstI64(9), 40),
    ]);
    assert_eq!((stats.loads_forwarded, stats.stores_removed), (0, 0));
    assert_eq!(f.blocks[0].insts.len(), 7);
}

#[test]
fn call_taking_the_pointer_ends_the_window() {
    let (stats, f) = one_block(vec![
        alloc(),
        store(Operand::ConstI64(0), 32),
        void(InstKind::Call(FuncId(1), vec![B])),
        store(Operand::ConstI64(7), 32),
        load(4, Type::I64, 32),
    ]);
    assert_eq!((stats.loads_forwarded, stats.stores_removed), (0, 0));
    assert_eq!(f.blocks[0].insts.len(), 5);
}

#[test]
fn call_without_the_pointer_keeps_the_window() {
    let (stats, _) = one_block(vec![
        alloc(),
        store(Operand::ConstI32(1), 4),
        void(InstKind::Call(FuncId(1), vec![Operand::ConstI64(5)])),
        load(1, Type::I32, 4),
    ]);
    assert_eq!(stats.loads_forwarded, 1);
}

#[test]
fn stored_pointer_ends_the_window() {
    let (stats, _) = one_block(vec![
        alloc(),
        store(Operand::ConstI32(1), 4),
        void(InstKind::Store(B, Operand::Value(ValueId(5)), 0)),
        load(1, Type::I32, 4),
    ]);
    assert_eq!(stats.loads_forwarded, 0);
}

#[test]
fn overlapping_widths_neither_forward_nor_die() {
    let (stats, f) = one_block(vec![
        alloc(),
        store(Operand::ConstI32(1), 0),
        store(Operand::ConstI32(1), 4),
        load(4, Type::I64, 0),
        store(Operand::ConstI64(3), 0),
        store(Operand::ConstI32(2), 4),
    ]);
    assert_eq!((stats.loads_forwarded, stats.stores_removed), (0, 0));
    assert_eq!(f.blocks[0].insts.len(), 6);
}

/// The inlined ctor shape: header stores, then per field a guard
/// diamond whose check arm calls the runtime with the cell, then the
/// field store.
fn guard_shape(between: Vec<Inst>) -> Vec<Block> {
    let mut entry = vec![
        alloc(),
        store(Operand::ConstI32(1), 4),
        store(Operand::ConstI64(0), 32),
    ];
    entry.extend(between);
    vec![
        block(0, entry, Terminator::Br(BlockId(1))),
        block(
            1,
            vec![
                load(1, Type::I32, 4),
                val(
                    2,
                    InstKind::BinOp(
                        BinOp::And,
                        Operand::Value(ValueId(1)),
                        Operand::ConstI32(GUARD_BITS),
                    ),
                ),
                val(
                    3,
                    InstKind::ICmp(IPred::Ne, Operand::Value(ValueId(2)), Operand::ConstI32(0)),
                ),
            ],
            Terminator::CondBr {
                cond: Operand::Value(ValueId(3)),
                then_blk: BlockId(2),
                else_blk: BlockId(3),
            },
        ),
        block(
            2,
            vec![void(InstKind::Call(FuncId(1), vec![B]))],
            Terminator::Br(BlockId(3)),
        ),
        block(
            3,
            vec![store(Operand::ConstI64(7), 32)],
            Terminator::Ret(None),
        ),
    ]
}

#[test]
fn guard_over_the_fresh_header_folds_and_the_path_continues() {
    let (stats, f) = run(guard_shape(vec![]));
    assert_eq!(
        (
            stats.loads_forwarded,
            stats.stores_removed,
            stats.branches_folded
        ),
        (1, 1, 1)
    );
    assert!(f.blocks[1].insts.is_empty());
    assert!(matches!(f.blocks[1].term, Terminator::Br(BlockId(3))));
    assert!(f.blocks[2].insts.is_empty());
    assert!(matches!(f.blocks[2].term, Terminator::Unreachable));
    assert_eq!(kinds(&f, 0).len(), 2);
    assert_eq!(
        kinds(&f, 3),
        vec![InstKind::Store(Operand::ConstI64(7), B, 32)]
    );
}

#[test]
fn guard_after_a_call_with_the_pointer_stays() {
    let (stats, f) = run(guard_shape(vec![void(InstKind::Call(FuncId(1), vec![B]))]));
    assert_eq!(stats, FreshCellStats::default());
    assert_eq!(f.blocks[1].insts.len(), 3);
    assert!(matches!(f.blocks[1].term, Terminator::CondBr { .. }));
    assert_eq!(f.blocks[0].insts.len(), 4);
}

#[test]
fn join_block_is_not_on_the_path() {
    // bb1 has two predecessors: the store there cannot see bb0's
    let blocks = vec![
        block(
            0,
            vec![alloc(), store(Operand::ConstI32(1), 4)],
            Terminator::Br(BlockId(1)),
        ),
        block(
            1,
            vec![load(1, Type::I32, 4)],
            Terminator::CondBr {
                cond: Operand::Value(ValueId(3)),
                then_blk: BlockId(1),
                else_blk: BlockId(2),
            },
        ),
        block(2, vec![], Terminator::Ret(None)),
    ];
    let (stats, _) = run(blocks);
    assert_eq!(stats.loads_forwarded, 0);
}
