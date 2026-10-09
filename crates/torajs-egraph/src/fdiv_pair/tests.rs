use super::*;
use torajs_core::ssa::{Block, IPred};

fn v(id: u32) -> Operand {
    Operand::Value(ValueId(id))
}

fn i(r: u32, kind: InstKind) -> Inst {
    inst(ValueId(r), kind)
}

/// `%0`, `%1` are f64 parameters; `x2` is defined in the loop when
/// `inner` holds:
///
/// ```text
/// bb0: %2 = copy 1; %11 = fadd %1, 0.5; br bb1
/// bb1: %3 = icmp sle %2, 100; cond_br %3, bb2, bb3
/// bb2: %4 = sitofp %2; %5 = fdiv %0, %4; %6 = fptosi %5;
///      %7 = fdiv x2, %4; %8 = fptosi %7; %9 = add %6, %8;
///      %10 = add %2, 1; %2 = copy %10; br bb1
/// bb3: ret 0
/// ```
fn two_quotients(inner: bool) -> Function {
    let types = [
        Type::F64,
        Type::F64,
        Type::I64,
        Type::Bool,
        Type::F64,
        Type::F64,
        Type::I64,
        Type::F64,
        Type::I64,
        Type::I64,
        Type::I64,
        Type::F64,
    ];
    let x2 = 11;
    let mut body = vec![
        i(4, InstKind::SiToFp(v(2))),
        i(5, InstKind::BinOp(BinOp::FDiv, v(0), v(4))),
        i(6, InstKind::FpToSi(v(5))),
        i(7, InstKind::BinOp(BinOp::FDiv, v(x2), v(4))),
        i(8, InstKind::FpToSi(v(7))),
        i(9, InstKind::BinOp(BinOp::Add, v(6), v(8))),
        i(10, InstKind::BinOp(BinOp::Add, v(2), Operand::ConstI64(1))),
        i(2, InstKind::Copy(Type::I64, v(10))),
    ];
    let half = i(
        x2,
        InstKind::BinOp(BinOp::FAdd, v(1), Operand::ConstF64(0.5)),
    );
    let mut pre = vec![i(2, InstKind::Copy(Type::I64, Operand::ConstI64(1)))];
    if inner {
        body.insert(0, half);
    } else {
        pre.push(half);
    }
    let block = |id: u32, insts: Vec<Inst>, term: Terminator| Block {
        id: BlockId(id),
        insts,
        term,
    };
    Function {
        name: "f".into(),
        params: vec![ValueId(0), ValueId(1)],
        ret: Type::I64,
        blocks: vec![
            block(0, pre, Terminator::Br(BlockId(1))),
            block(
                1,
                vec![i(
                    3,
                    InstKind::ICmp(IPred::Sle, v(2), Operand::ConstI64(100)),
                )],
                Terminator::CondBr {
                    cond: v(3),
                    then_blk: BlockId(2),
                    else_blk: BlockId(3),
                },
            ),
            block(2, body, Terminator::Br(BlockId(1))),
            block(3, vec![], Terminator::Ret(Some(Operand::ConstI64(0)))),
        ],
        values: types
            .iter()
            .map(|t| ValueInfo { ty: *t, name: None })
            .collect(),
        current_origin: None,
    }
}

fn run(f: Function) -> (Function, u32) {
    let mut m = Module::default();
    m.funcs.push(f);
    let stats = pair_module(&mut m, None);
    (m.funcs.pop().expect("one function"), stats.pairs)
}

#[test]
fn pairs_invariant_dividends_over_a_shared_divisor() {
    let (f, pairs) = run(two_quotients(false));
    assert_eq!(pairs, 1);
    let pre = &f.blocks[0].insts;
    assert!(matches!(
        pre.last().map(|i| &i.kind),
        Some(InstKind::Vec(VecInst::Pair(
            VecShape::F64x2,
            Operand::Value(ValueId(0)),
            Operand::Value(ValueId(11))
        )))
    ));
    let body = &f.blocks[2].insts;
    assert!(
        !body
            .iter()
            .any(|i| matches!(i.kind, InstKind::BinOp(BinOp::FDiv, ..)))
    );
    let kinds: Vec<&InstKind> = body.iter().map(|i| &i.kind).collect();
    assert!(matches!(
        kinds[1],
        InstKind::Vec(VecInst::Splat(VecShape::F64x2, Operand::Value(ValueId(4))))
    ));
    assert!(matches!(
        kinds[2],
        InstKind::Vec(VecInst::BinOp(VecOp::FDiv, ..))
    ));
    assert!(matches!(kinds[3], InstKind::Vec(VecInst::FpToSi(_))));
    // the conversions keep their results, now read from the lanes
    assert_eq!(body[4].result, Some(ValueId(6)));
    assert!(matches!(body[4].kind, InstKind::Vec(VecInst::Lane(0, _))));
    assert_eq!(body[5].result, Some(ValueId(8)));
    assert!(matches!(body[5].kind, InstKind::Vec(VecInst::Lane(1, _))));
}

#[test]
fn leaves_a_dividend_defined_in_the_loop() {
    let (f, pairs) = run(two_quotients(true));
    assert_eq!(pairs, 0);
    assert_eq!(f, two_quotients(true));
}

/// `n` loop-invariant dividends (parameters `%0..%n`) over one shared
/// divisor in a single loop block.
fn many_quotients(n: u32) -> Function {
    let c = n;
    let cmp = n + 1;
    let d = n + 2;
    let q = |j: u32| n + 3 + 2 * j;
    let inc = n + 3 + 2 * n;
    let mut types = vec![Type::F64; n as usize];
    types.extend([Type::I64, Type::Bool, Type::F64]);
    for _ in 0..n {
        types.extend([Type::F64, Type::I64]);
    }
    types.push(Type::I64);
    let mut body = vec![i(d, InstKind::SiToFp(v(c)))];
    for j in 0..n {
        body.push(i(q(j), InstKind::BinOp(BinOp::FDiv, v(j), v(d))));
        body.push(i(q(j) + 1, InstKind::FpToSi(v(q(j)))));
    }
    body.push(i(
        inc,
        InstKind::BinOp(BinOp::Add, v(c), Operand::ConstI64(1)),
    ));
    body.push(i(c, InstKind::Copy(Type::I64, v(inc))));
    let block = |id: u32, insts: Vec<Inst>, term: Terminator| Block {
        id: BlockId(id),
        insts,
        term,
    };
    Function {
        name: "f".into(),
        params: (0..n).map(ValueId).collect(),
        ret: Type::I64,
        blocks: vec![
            block(
                0,
                vec![i(c, InstKind::Copy(Type::I64, Operand::ConstI64(1)))],
                Terminator::Br(BlockId(1)),
            ),
            block(
                1,
                vec![i(
                    cmp,
                    InstKind::ICmp(IPred::Sle, v(c), Operand::ConstI64(100)),
                )],
                Terminator::CondBr {
                    cond: v(cmp),
                    then_blk: BlockId(2),
                    else_blk: BlockId(3),
                },
            ),
            block(2, body, Terminator::Br(BlockId(1))),
            block(3, vec![], Terminator::Ret(Some(Operand::ConstI64(0)))),
        ],
        values: types
            .iter()
            .map(|t| ValueInfo { ty: *t, name: None })
            .collect(),
        current_origin: None,
    }
}

#[test]
fn a_loop_takes_at_most_its_cap_of_pairs() {
    let (_, pairs) = run(many_quotients(4));
    assert_eq!(pairs, 2);
    // the dividend vectors live through the loop and cannot spill
    let (f, pairs) = run(many_quotients(12));
    assert_eq!(pairs as usize, PAIRS_PER_LOOP_MAX);
    let scalar = f.blocks[2]
        .insts
        .iter()
        .filter(|i| matches!(i.kind, InstKind::BinOp(BinOp::FDiv, ..)))
        .count();
    assert_eq!(scalar, 12 - 2 * PAIRS_PER_LOOP_MAX);
}
