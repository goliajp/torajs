use super::super::pair_module;
use super::*;
use torajs_core::ssa::{Block, Module, ValueInfo};

fn v(id: u32) -> Operand {
    Operand::Value(ValueId(id))
}

fn i(r: u32, kind: InstKind) -> Inst {
    inst(ValueId(r), kind)
}

/// Two dividends `%0`, `%1` summing their divisors `d = 1..=100`:
///
/// ```text
/// bb0: %2 = sitofp %0; %3 = sitofp %1; %4 = copy 1; %5 = copy 0; %6 = copy 0; br bb1
/// bb1: %7 = icmp sle %4, 100; cond_br %7, bb2, bb3
/// bb2: %8 = sitofp %4
///      %9 = fdiv %2, %8; %10 = fptosi %9; %11 = mul %10, %4
///      %12 = fdiv %3, %8; %13 = fptosi %12; %14 = mul %13, %4
///      %15 = copy %4
///      %16 = add %5, %15; %17 = icmp eq %11, %0; %18 = select %17, %16, %5
///      %19 = add %6, %15; %20 = icmp eq %14, %1; %21 = select %20, %19, %6
///      %22 = add %4, 1; %5 = copy %18; %6 = copy %21; %4 = copy %22; br bb1
/// bb3: %23 = add %5, %6; ret %23
/// ```
fn divisor_sums() -> Function {
    use Type::{Bool, F64, I64};
    let types = [
        I64, I64, F64, F64, I64, I64, I64, Bool, F64, F64, I64, I64, F64, I64, I64, I64, I64, Bool,
        I64, I64, Bool, I64, I64, I64,
    ];
    let bin = |op, a, b| InstKind::BinOp(op, a, b);
    let copy = |a| InstKind::Copy(I64, a);
    let c = Operand::ConstI64;
    let pre = vec![
        i(2, InstKind::SiToFp(v(0))),
        i(3, InstKind::SiToFp(v(1))),
        i(4, copy(c(1))),
        i(5, copy(c(0))),
        i(6, copy(c(0))),
    ];
    let body = vec![
        i(8, InstKind::SiToFp(v(4))),
        i(9, bin(BinOp::FDiv, v(2), v(8))),
        i(10, InstKind::FpToSi(v(9))),
        i(11, bin(BinOp::Mul, v(10), v(4))),
        i(12, bin(BinOp::FDiv, v(3), v(8))),
        i(13, InstKind::FpToSi(v(12))),
        i(14, bin(BinOp::Mul, v(13), v(4))),
        i(15, copy(v(4))),
        i(16, bin(BinOp::Add, v(5), v(15))),
        i(17, InstKind::ICmp(IPred::Eq, v(11), v(0))),
        i(18, InstKind::Select(I64, v(17), v(16), v(5))),
        i(19, bin(BinOp::Add, v(6), v(15))),
        i(20, InstKind::ICmp(IPred::Eq, v(14), v(1))),
        i(21, InstKind::Select(I64, v(20), v(19), v(6))),
        i(22, bin(BinOp::Add, v(4), c(1))),
        i(5, copy(v(18))),
        i(6, copy(v(21))),
        i(4, copy(v(22))),
    ];
    let block = |id: u32, insts: Vec<Inst>, term: Terminator| Block {
        id: BlockId(id),
        insts,
        term,
    };
    Function {
        name: "f".into(),
        params: vec![ValueId(0), ValueId(1)],
        ret: I64,
        blocks: vec![
            block(0, pre, Terminator::Br(BlockId(1))),
            block(
                1,
                vec![i(7, InstKind::ICmp(IPred::Sle, v(4), c(100)))],
                Terminator::CondBr {
                    cond: v(7),
                    then_blk: BlockId(2),
                    else_blk: BlockId(3),
                },
            ),
            block(2, body, Terminator::Br(BlockId(1))),
            block(
                3,
                vec![i(23, bin(BinOp::Add, v(5), v(6)))],
                Terminator::Ret(Some(v(23))),
            ),
        ],
        values: types
            .iter()
            .map(|t| ValueInfo { ty: *t, name: None })
            .collect(),
        current_origin: None,
    }
}

/// Both dividends known to lie in `[0, hi]`.
fn run(f: Function, hi: Option<i128>) -> (Function, u32, u32) {
    let mut m = Module::default();
    m.funcs.push(f);
    let facts: Option<Vec<HashMap<ValueId, NumFact>>> = hi.map(|hi| {
        let fact = NumFact::new(0, hi);
        vec![
            [(ValueId(0), fact), (ValueId(1), fact)]
                .into_iter()
                .collect(),
        ]
    });
    let stats = pair_module(&mut m, facts.as_deref());
    let f = m.funcs.pop().expect("one function");
    (f, stats.pairs, stats.accumulated)
}

fn is_vec(i: &Inst, want: impl Fn(&VecInst) -> bool) -> bool {
    matches!(&i.kind, InstKind::Vec(vi) if want(vi))
}

#[test]
fn moves_the_tests_and_sums_onto_the_lanes() {
    let (f, pairs, accumulated) = run(divisor_sums(), Some(1 << 31));
    assert_eq!((pairs, accumulated), (1, 1));
    let pre = &f.blocks[0].insts;
    let n = pre.len();
    // the counter cells first, then the dividend pair and the sum cell
    assert!(is_vec(&pre[n - 2], |vi| matches!(
        vi,
        VecInst::Pair(
            VecShape::I64x2,
            Operand::Value(ValueId(5)),
            Operand::Value(ValueId(6))
        )
    )));
    assert!(matches!(pre[n - 1].kind, InstKind::Copy(I64X2, _)));
    let body = &f.blocks[2].insts;
    // no scalar step of either chain is left
    assert!(!body.iter().any(|i| matches!(
        i.kind,
        InstKind::FpToSi(_)
            | InstKind::BinOp(BinOp::Mul | BinOp::FDiv, ..)
            | InstKind::ICmp(..)
            | InstKind::Select(..)
    )));
    let shape: Vec<String> = body
        .iter()
        .map(|i| match &i.kind {
            InstKind::Vec(vi) => vi.mnemonic(),
            InstKind::Copy(t, _) if *t == I64X2 => "copy.v".into(),
            k => format!("{k:?}")
                .split('(')
                .next()
                .unwrap_or("")
                .to_lowercase(),
        })
        .collect();
    // the divisor `%4` counts up by one: its lanes are cells, each
    // stepped right after its last read, and its conversion and copy
    // are gone
    assert_eq!(
        shape,
        [
            "vec.fdiv",
            "vec.fadd",
            "copy",
            "vec.frintz",
            "vec.fcmeq",
            "vec.and",
            "vec.add",
            "copy.v",
            "vec.add",
            "binop",
            "copy.v",
            "copy",
        ]
    );
    // the sum cell's one write in the loop is the carry
    let sv = body[10].result;
    assert!(
        matches!(body[8].kind, InstKind::Vec(VecInst::BinOp(VecOp::Add, Operand::Value(x), _)) if Some(x) == sv)
    );
    // the exit splits the sums back into the scalar cells
    let exit = &f.blocks[3].insts;
    assert!(is_vec(
        &exit[0],
        |vi| matches!(vi, VecInst::Lane(0, Operand::Value(x)) if Some(*x) == sv)
    ));
    assert_eq!(exit[1].result, Some(ValueId(5)));
    assert!(is_vec(
        &exit[2],
        |vi| matches!(vi, VecInst::Lane(1, Operand::Value(x)) if Some(*x) == sv)
    ));
    assert_eq!(exit[3].result, Some(ValueId(6)));
}

#[test]
fn needs_the_dividends_inside_the_exact_window() {
    // no facts: the divide still pairs, the test stays scalar
    let (_, pairs, accumulated) = run(divisor_sums(), None);
    assert_eq!((pairs, accumulated), (1, 0));
    let (_, pairs, accumulated) = run(divisor_sums(), Some(1 << 53));
    assert_eq!((pairs, accumulated), (1, 0));
    let (_, _, accumulated) = run(divisor_sums(), Some((1 << 53) - 1));
    assert_eq!(accumulated, 1);
}

#[test]
fn leaves_a_sum_the_loop_reads_elsewhere() {
    let mut f = divisor_sums();
    // the header tests the first sum too
    f.blocks[1].insts[0].kind = InstKind::ICmp(IPred::Sle, v(5), Operand::ConstI64(100));
    let (_, pairs, accumulated) = run(f, Some(1 << 31));
    assert_eq!((pairs, accumulated), (1, 0));
}

#[test]
fn leaves_a_divisor_that_may_start_below_one() {
    let mut f = divisor_sums();
    f.blocks[0].insts[2].kind = InstKind::Copy(Type::I64, Operand::ConstI64(0));
    let (_, _, accumulated) = run(f, Some(1 << 31));
    assert_eq!(accumulated, 0);
}

#[test]
fn leaves_an_exit_entered_from_outside_the_loop() {
    let mut f = divisor_sums();
    // bb4 now enters the exit beside the loop's header test
    let pre = std::mem::take(&mut f.blocks[0].insts);
    f.blocks[0].insts = vec![];
    f.blocks[0].term = Terminator::CondBr {
        cond: Operand::ConstBool(true),
        then_blk: BlockId(4),
        else_blk: BlockId(3),
    };
    f.blocks.push(Block {
        id: BlockId(4),
        insts: pre,
        term: Terminator::Br(BlockId(1)),
    });
    let (_, pairs, accumulated) = run(f, Some(1 << 31));
    assert_eq!((pairs, accumulated), (1, 0));
}

/// The pass's counter counts, on `divisor_sums` with both dividends in
/// `[0, 2^31]`.
fn counters(f: Function) -> (u32, u32) {
    let mut m = Module::default();
    m.funcs.push(f);
    let fact = NumFact::new(0, 1 << 31);
    let facts = vec![
        [(ValueId(0), fact), (ValueId(1), fact)]
            .into_iter()
            .collect(),
    ];
    let stats = pair_module(&mut m, Some(&facts));
    (stats.float_counters, stats.int_counters)
}

#[test]
fn a_counter_divisor_is_read_from_cells() {
    assert_eq!(counters(divisor_sums()), (1, 1));
    let (f, _, _) = run(divisor_sums(), Some(1 << 31));
    let pre = &f.blocks[0].insts;
    // the f64 cell from `sitofp %4`, the i64 cell from `%4`, each with
    // the splat of its step
    let splat_of = |i: &Inst| match &i.kind {
        InstKind::Vec(VecInst::Splat(_, o)) => Some(*o),
        _ => None,
    };
    let splats: Vec<Operand> = pre.iter().filter_map(splat_of).collect();
    assert!(matches!(splats[0], Operand::Value(_)));
    assert_eq!(splats[1], Operand::ConstF64(1.0));
    assert_eq!(splats[2], v(4));
    assert_eq!(splats[3], Operand::ConstI64(1));
    // the loop splats nothing
    assert!(!f.blocks[2].insts.iter().any(|i| splat_of(i).is_some()));
}
