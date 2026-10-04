use torajs_core::ssa::{
    BinOp, Block, BlockId, FuncId, Function, IPred, Inst, InstKind, Module, Operand, Terminator,
    Type, ValueId, ValueInfo, VecInst,
};

use super::widen_sum_loops;

fn v(n: u32) -> Operand {
    Operand::Value(ValueId(n))
}

fn def(r: u32, kind: InstKind) -> Inst {
    Inst {
        result: Some(ValueId(r)),
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

fn guard(cond: u32, exit: u32, next: u32) -> Terminator {
    Terminator::CondBr {
        cond: v(cond),
        then_blk: BlockId(exit),
        else_blk: BlockId(next),
    }
}

/// `for (i = 0; i < arr.length; i++) s += arr[i]` as float_demote
/// leaves it: %0 arr, %1 len, %2 data, %3 i, %4 s.
fn sum_loop(elem_guard: InstKind, body_extra: Vec<Inst>) -> Function {
    let ty = |t| ValueInfo { ty: t, name: None };
    let mut values = vec![ty(Type::Ptr), ty(Type::I64), ty(Type::Ptr)];
    values.extend((3..20).map(|_| ty(Type::I64)));
    values[5] = ty(Type::Bool);
    values[9] = ty(Type::Bool);
    values[12] = ty(Type::Bool);
    let blocks = vec![
        block(
            0,
            vec![
                def(1, InstKind::Load(Type::I64, v(0), 8)),
                def(2, InstKind::Load(Type::Ptr, v(0), 32)),
                def(3, InstKind::Copy(Type::I64, Operand::ConstI64(0))),
                def(4, InstKind::Copy(Type::I64, Operand::ConstI64(0))),
            ],
            Terminator::Br(BlockId(1)),
        ),
        block(
            1,
            vec![def(5, InstKind::ICmp(IPred::Slt, v(3), v(1)))],
            guard(5, 2, 5),
        ),
        block(
            2,
            vec![
                def(6, InstKind::BinOp(BinOp::Shl, v(3), Operand::ConstI64(3))),
                def(7, InstKind::LoadDyn(Type::I64, v(2), v(6))),
                def(8, InstKind::BinOp(BinOp::LShr, v(7), Operand::ConstI64(53))),
                def(9, elem_guard),
            ],
            guard(9, 6, 3),
        ),
        block(
            3,
            vec![
                def(10, InstKind::BinOp(BinOp::Add, v(4), v(7))),
                def(
                    11,
                    InstKind::BinOp(BinOp::LShr, v(10), Operand::ConstI64(53)),
                ),
                def(12, InstKind::ICmp(IPred::Ne, v(11), Operand::ConstI64(0))),
            ],
            guard(12, 6, 4),
        ),
        block(
            4,
            [
                body_extra,
                vec![
                    def(13, InstKind::BinOp(BinOp::Add, v(3), Operand::ConstI64(1))),
                    def(4, InstKind::Copy(Type::I64, v(10))),
                    def(3, InstKind::Copy(Type::I64, v(13))),
                ],
            ]
            .concat(),
            Terminator::Br(BlockId(1)),
        ),
        block(5, vec![], Terminator::Ret(Some(v(4)))),
        block(6, vec![], Terminator::Ret(Some(Operand::ConstI64(-1)))),
    ];
    Function {
        name: "sum".into(),
        params: vec![ValueId(0)],
        ret: Type::I64,
        blocks,
        values,
        current_origin: None,
    }
}

fn high_bits_guard() -> InstKind {
    InstKind::ICmp(IPred::Ne, v(8), Operand::ConstI64(0))
}

fn widened(func: Function) -> (u32, Function) {
    let mut m = Module::default();
    m.funcs.push(func);
    let stats = widen_sum_loops(&mut m);
    (stats.loops_widened, m.funcs.pop().unwrap())
}

fn vec_insts(f: &Function) -> usize {
    f.blocks
        .iter()
        .flat_map(|b| &b.insts)
        .filter(|i| matches!(i.kind, InstKind::Vec(_)))
        .count()
}

#[test]
fn guarded_sum_loop_is_widened_in_front_of_the_scalar_loop() {
    let (n, f) = widened(sum_loop(high_bits_guard(), vec![]));
    assert_eq!(n, 1);
    // the entry edge now reaches the widened path, which falls back to
    // the untouched scalar header
    assert!(matches!(f.blocks[0].term, Terminator::Br(b) if b.0 >= 7));
    assert_eq!(f.blocks[1].insts.len(), 1);
    let reduces = f
        .blocks
        .iter()
        .flat_map(|b| &b.insts)
        .filter(|i| matches!(i.kind, InstKind::Vec(VecInst::Reduce(..))))
        .count();
    assert_eq!(reduces, 2);
    let to_header = f
        .blocks
        .iter()
        .skip(7)
        .filter(|b| super::flow::successors(&b.term).any(|s| s == BlockId(1)))
        .count();
    assert_eq!(to_header, 3, "entry check, short tail, failed block");
}

#[test]
fn call_in_the_body_keeps_the_loop_scalar() {
    let call = Inst {
        result: None,
        kind: InstKind::Call(FuncId(0), vec![]),
        origin: None,
    };
    let (n, f) = widened(sum_loop(high_bits_guard(), vec![call]));
    assert_eq!(n, 0);
    assert_eq!(vec_insts(&f), 0);
}

#[test]
fn element_guard_tighter_than_the_exact_range_keeps_the_loop_scalar() {
    // `e > 2^40` fires on values the widened lane check would accept
    let tight = InstKind::ICmp(IPred::Sgt, v(7), Operand::ConstI64(1 << 40));
    let (n, _) = widened(sum_loop(tight, vec![]));
    assert_eq!(n, 0);
}

#[test]
fn element_guard_looser_than_the_exact_range_is_widened() {
    let loose = InstKind::ICmp(IPred::Sgt, v(7), Operand::ConstI64((1 << 53) - 1));
    let (n, _) = widened(sum_loop(loose, vec![]));
    assert_eq!(n, 1);
}

#[test]
fn counter_read_after_its_step_keeps_the_loop_scalar() {
    let mut f = sum_loop(high_bits_guard(), vec![]);
    f.values.push(ValueInfo {
        ty: Type::I64,
        name: None,
    });
    f.blocks[4].insts.push(def(
        20,
        InstKind::BinOp(BinOp::Add, v(3), Operand::ConstI64(1)),
    ));
    let (n, _) = widened(f);
    assert_eq!(n, 0);
}
