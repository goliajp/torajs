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
/// divisor in a single loop block, each divide followed by its
/// conversion, or with `grouped` every divide first.
fn many_quotients(n: u32, grouped: bool) -> Function {
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
        if !grouped {
            body.push(i(q(j) + 1, InstKind::FpToSi(v(q(j)))));
        }
    }
    if grouped {
        body.extend((0..n).map(|j| i(q(j) + 1, InstKind::FpToSi(v(q(j))))));
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
    let (_, pairs) = run(many_quotients(4, false));
    assert_eq!(pairs, 2);
    // the dividend vectors live through the loop and cannot spill
    let (f, pairs) = run(many_quotients(12, false));
    assert_eq!(pairs as usize, PAIRS_PER_LOOP_MAX);
    let scalar = f.blocks[2]
        .insts
        .iter()
        .filter(|i| matches!(i.kind, InstKind::BinOp(BinOp::FDiv, ..)))
        .count();
    assert_eq!(scalar, 12 - 2 * PAIRS_PER_LOOP_MAX);
}

#[test]
fn neighbouring_divides_issue_before_their_tails() {
    let (f, pairs) = run(many_quotients(4, true));
    assert_eq!(pairs, 2);
    let kinds: Vec<&InstKind> = f.blocks[2].insts.iter().map(|i| &i.kind).collect();
    let fdiv = |k: &InstKind| matches!(k, InstKind::Vec(VecInst::BinOp(VecOp::FDiv, ..)));
    let cvt = |k: &InstKind| matches!(k, InstKind::Vec(VecInst::FpToSi(_)));
    // sitofp, splat, the two divides back to back, then their conversions
    assert!(fdiv(kinds[2]) && fdiv(kinds[3]), "{kinds:?}");
    assert!(cvt(kinds[4]) && cvt(kinds[5]), "{kinds:?}");
    assert!(matches!(kinds[6], InstKind::Vec(VecInst::Lane(0, _))));
}

/// `two_quotients(false)` with the divisor `%2` known in `[1, hi]`;
/// answers the function and the f64 counters made.
fn run_counted(f: Function, hi: i128) -> (Function, u32) {
    let mut m = Module::default();
    m.funcs.push(f);
    let facts = vec![[(ValueId(2), NumFact::new(1, hi))].into_iter().collect()];
    let stats = pair_module(&mut m, Some(&facts));
    (m.funcs.pop().expect("one function"), stats.float_counters)
}

fn splats_in_loop(f: &Function) -> usize {
    f.blocks[2]
        .insts
        .iter()
        .filter(|i| matches!(i.kind, InstKind::Vec(VecInst::Splat(..))))
        .count()
}

#[test]
fn a_counter_divisor_inside_the_exact_window_is_a_cell() {
    let (f, counters) = run_counted(two_quotients(false), 101);
    assert_eq!(counters, 1);
    assert_eq!(splats_in_loop(&f), 0);
    let body = &f.blocks[2].insts;
    // no conversion of the counter is left; the divide reads the cell,
    // stepped right after it, and the loop's write of `%2` stays last
    assert!(!body.iter().any(|i| matches!(i.kind, InstKind::SiToFp(_))));
    let InstKind::Vec(VecInst::BinOp(VecOp::FDiv, _, Operand::Value(cell))) = body[0].kind else {
        panic!("the divide first: {body:?}");
    };
    assert!(matches!(
        body[1].kind,
        InstKind::Vec(VecInst::BinOp(VecOp::FAdd, Operand::Value(x), _)) if x == cell
    ));
    assert_eq!(body[2].result, Some(cell));
    assert_eq!(body.last().and_then(|i| i.result), Some(ValueId(2)));
    // the preheader sets the cell from `%2` as it enters
    let pre = &f.blocks[0].insts;
    let start = |i: &Inst| matches!(i.kind, InstKind::SiToFp(Operand::Value(ValueId(2))));
    assert!(pre.iter().any(start));
    assert!(pre.iter().any(|i| i.result == Some(cell)));
}

#[test]
fn a_counter_divisor_past_the_exact_window_is_splatted() {
    let (f, counters) = run_counted(two_quotients(false), 1 << 53);
    assert_eq!(counters, 0);
    assert_eq!(splats_in_loop(&f), 1);
    let (_, counters) = run_counted(two_quotients(false), (1 << 53) - 1);
    assert_eq!(counters, 1);
}

#[test]
fn a_divisor_the_loop_writes_twice_is_splatted() {
    let mut f = two_quotients(false);
    f.blocks[1]
        .insts
        .push(i(2, InstKind::Copy(Type::I64, v(2))));
    let (f, counters) = run_counted(f, 101);
    assert_eq!(counters, 0);
    assert_eq!(splats_in_loop(&f), 1);
}

#[test]
fn a_divisor_stepped_by_a_variable_is_splatted() {
    let mut f = two_quotients(false);
    f.blocks[2].insts[6].kind = InstKind::BinOp(BinOp::Add, v(2), v(6));
    let (f, counters) = run_counted(f, 101);
    assert_eq!(counters, 0);
    assert_eq!(splats_in_loop(&f), 1);
}

#[test]
fn a_counter_written_in_another_block_steps_its_cell_there() {
    let mut f = two_quotients(false);
    // the step and the write move to a latch block of their own
    let step: Vec<Inst> = f.blocks[2].insts.drain(6..).collect();
    f.blocks[2].term = Terminator::Br(BlockId(4));
    f.blocks.push(Block {
        id: BlockId(4),
        insts: step,
        term: Terminator::Br(BlockId(1)),
    });
    let (f, counters) = run_counted(f, 101);
    assert_eq!(counters, 1);
    assert_eq!(splats_in_loop(&f), 0);
    let latch = &f.blocks[4].insts;
    assert_eq!(latch.len(), 4);
    assert_eq!(latch[1].result, Some(ValueId(2)));
    assert!(matches!(
        latch[2].kind,
        InstKind::Vec(VecInst::BinOp(VecOp::FAdd, ..))
    ));
}
