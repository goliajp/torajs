use super::*;
use crate::id_hash::HashMap;
use crate::interval::NumFact;
use torajs_core::ssa::{
    BinOp, Block, Function, IPred, Inst, InstKind, Terminator, Type, ValueInfo, VecInst, VecOp,
    VecShape,
};

fn v(id: u32) -> Operand {
    Operand::Value(ValueId(id))
}

fn i(r: u32, kind: InstKind) -> Inst {
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

fn cond(c: u32, then_blk: u32, else_blk: u32) -> Terminator {
    Terminator::CondBr {
        cond: v(c),
        then_blk: BlockId(then_blk),
        else_blk: BlockId(else_blk),
    }
}

/// The trial-division loop srem_fquot leaves, `n = %0`, `i = %1`:
///
/// ```text
/// bb0: %1 = copy 2; %10 = sitofp %0; br bb1
/// bb1: %2 = mul %1, %1; %3 = icmp sle %2, %0; cond_br %3, bb2, bb4
/// bb2: %5 = sitofp %1; %6 = fdiv %10, %5; %7 = fptosi %6; %8 = mul %7, %1;
///      %9 = icmp eq %8, %0; cond_br %9, bb5, bb3
/// bb3: %11 = add %1, 1; %1 = copy %11; br bb1
/// bb4: ret 1        bb5: ret 0
/// ```
fn trial_loop() -> Function {
    let types = [
        Type::I64,  // 0 n
        Type::I64,  // 1 i
        Type::I64,  // 2
        Type::Bool, // 3
        Type::I64,  // 4 unused
        Type::F64,  // 5
        Type::F64,  // 6
        Type::I64,  // 7
        Type::I64,  // 8
        Type::Bool, // 9
        Type::F64,  // 10
        Type::I64,  // 11
    ];
    Function {
        name: "f".into(),
        params: vec![ValueId(0)],
        ret: Type::I64,
        blocks: vec![
            block(
                0,
                vec![
                    i(1, InstKind::Copy(Type::I64, Operand::ConstI64(2))),
                    i(10, InstKind::SiToFp(v(0))),
                ],
                Terminator::Br(BlockId(1)),
            ),
            block(
                1,
                vec![
                    i(2, InstKind::BinOp(BinOp::Mul, v(1), v(1))),
                    i(3, InstKind::ICmp(IPred::Sle, v(2), v(0))),
                ],
                cond(3, 2, 4),
            ),
            block(
                2,
                vec![
                    i(5, InstKind::SiToFp(v(1))),
                    i(6, InstKind::BinOp(BinOp::FDiv, v(10), v(5))),
                    i(7, InstKind::FpToSi(v(6))),
                    i(8, InstKind::BinOp(BinOp::Mul, v(7), v(1))),
                    i(9, InstKind::ICmp(IPred::Eq, v(8), v(0))),
                ],
                cond(9, 5, 3),
            ),
            block(
                3,
                vec![
                    i(11, InstKind::BinOp(BinOp::Add, v(1), Operand::ConstI64(1))),
                    i(1, InstKind::Copy(Type::I64, v(11))),
                ],
                Terminator::Br(BlockId(1)),
            ),
            block(4, vec![], Terminator::Ret(Some(Operand::ConstI64(1)))),
            block(5, vec![], Terminator::Ret(Some(Operand::ConstI64(0)))),
        ],
        values: types
            .iter()
            .map(|t| ValueInfo { ty: *t, name: None })
            .collect(),
        current_origin: None,
    }
}

fn run(f: &mut Function) -> u32 {
    run_with(f, None).loops_paired
}

/// Runs the pass with the counter `%1` bounded by `hi`, when given.
fn run_with(f: &mut Function, hi: Option<i128>) -> UnrollPairStats {
    let mut m = Module::default();
    m.funcs.push(f.clone());
    let facts = hi.map(|hi| vec![HashMap::from_iter([(ValueId(1), NumFact::new(2, hi))])]);
    let stats = pair_module(&mut m, facts.as_deref(), &[]);
    *f = m.funcs.pop().unwrap();
    stats
}

fn kinds(f: &Function, b: u32) -> Vec<&InstKind> {
    f.blocks[b as usize].insts.iter().map(|i| &i.kind).collect()
}

fn is_vec(kind: &InstKind, want: impl Fn(&VecInst) -> bool) -> bool {
    matches!(kind, InstKind::Vec(vi) if want(vi))
}

#[test]
fn the_loop_is_unrolled_twice_around_one_lane_divide() {
    let mut f = trial_loop();
    assert_eq!(run(&mut f), 1);
    // the dividend lanes are made once, before the loop
    assert!(is_vec(kinds(&f, 0)[2], |vi| matches!(
        vi,
        VecInst::Splat(VecShape::F64x2, Operand::Value(ValueId(10)))
    )));
    // the first copy's site: early step, its conversion, the pair,
    // lane divide, lane convert, lane 0 read
    let site = kinds(&f, 2);
    assert!(matches!(
        site[0],
        InstKind::SiToFp(Operand::Value(ValueId(1)))
    ));
    assert!(matches!(
        site[1],
        InstKind::BinOp(BinOp::Add, Operand::Value(ValueId(1)), Operand::ConstI64(1))
    ));
    assert!(matches!(site[2], InstKind::SiToFp(_)));
    assert!(is_vec(site[3], |vi| matches!(
        vi,
        VecInst::Pair(Operand::Value(ValueId(5)), _)
    )));
    assert!(is_vec(site[4], |vi| matches!(
        vi,
        VecInst::BinOp(VecOp::FDiv, _, _)
    )));
    assert_eq!(f.blocks[2].insts[4].result, Some(ValueId(6)));
    assert_eq!(f.value_type(ValueId(6)), Type::Vec(VecShape::F64x2));
    assert!(is_vec(site[5], |vi| matches!(
        vi,
        VecInst::FpToSi(Operand::Value(ValueId(6)))
    )));
    assert!(is_vec(site[6], |vi| matches!(vi, VecInst::Lane(0, _))));
    assert_eq!(f.blocks[2].insts[6].result, Some(ValueId(7)));
    // the latch copies the early step and goes on to the clone's header
    let latch = kinds(&f, 3);
    assert_eq!(latch.len(), 1);
    let early = f.blocks[2].insts[1].result.unwrap();
    assert!(matches!(latch[0], InstKind::Copy(Type::I64, Operand::Value(e)) if *e == early));
    assert_eq!(f.blocks[3].term, Terminator::Br(BlockId(6)));
    // the clone: header test, lane 1 read, its own step, back to bb1
    assert_eq!(f.blocks.len(), 9);
    assert_eq!(kinds(&f, 6).len(), 2);
    let test = f.blocks[6].insts[1].result.unwrap().0;
    assert_eq!(f.blocks[6].term, cond(test, 7, 4));
    let site2 = kinds(&f, 7);
    assert!(is_vec(site2[0], |vi| matches!(vi, VecInst::Lane(1, _))));
    assert_eq!(site2.len(), 3);
    assert!(matches!(
        f.blocks[7].term,
        Terminator::CondBr {
            then_blk: BlockId(5),
            else_blk: BlockId(8),
            ..
        }
    ));
    assert!(matches!(kinds(&f, 8)[1], InstKind::Copy(Type::I64, _)));
    assert_eq!(f.blocks[8].insts[1].result, Some(ValueId(1)));
    assert_eq!(f.blocks[8].term, Terminator::Br(BlockId(1)));
    // the pass does not take the clone as a loop of its own
    assert_eq!(run(&mut f), 0);
}

#[test]
fn a_counter_bounded_inside_the_exact_window_steps_as_float_lanes() {
    let mut f = trial_loop();
    let stats = run_with(&mut f, Some((1 << 53) - 2));
    assert_eq!((stats.loops_paired, stats.float_counters), (1, 1));
    // preheader: dividend splat, the two lanes, the cell, the step splat
    let pre = kinds(&f, 0);
    assert!(matches!(
        pre[3],
        InstKind::SiToFp(Operand::Value(ValueId(1)))
    ));
    assert!(matches!(
        pre[4],
        InstKind::BinOp(BinOp::FAdd, _, Operand::ConstF64(c)) if *c == 1.0
    ));
    assert!(is_vec(pre[5], |vi| matches!(vi, VecInst::Pair(..))));
    assert!(matches!(
        pre[6],
        InstKind::Copy(Type::Vec(VecShape::F64x2), _)
    ));
    let cell = f.blocks[0].insts[6].result.unwrap();
    assert!(is_vec(pre[7], |vi| matches!(
        vi,
        VecInst::Splat(VecShape::F64x2, Operand::ConstF64(c)) if *c == 2.0
    )));
    // the site reads the cell: no conversion and no early step remain
    let site = kinds(&f, 2);
    assert!(is_vec(site[0], |vi| matches!(
        vi,
        VecInst::BinOp(VecOp::FDiv, _, Operand::Value(y)) if *y == cell
    )));
    assert_eq!(site.len(), 5);
    // the first latch keeps its own step; the clone's latch steps the lanes
    assert_eq!(kinds(&f, 3).len(), 2);
    let latch2 = kinds(&f, 8);
    assert_eq!(latch2.len(), 4);
    assert!(is_vec(latch2[2], |vi| matches!(
        vi,
        VecInst::BinOp(VecOp::FAdd, Operand::Value(y), _) if *y == cell
    )));
    assert_eq!(f.blocks[8].insts[3].result, Some(cell));
}

#[test]
fn a_value_defined_twice_in_the_body_keeps_the_loop() {
    let mut f = trial_loop();
    // `%8` again, after its compare: not a cell (its first def is a mul)
    f.blocks[2]
        .insts
        .push(i(8, InstKind::BinOp(BinOp::Add, v(7), v(1))));
    assert_eq!(run(&mut f), 0);
}

#[test]
fn a_function_the_jam_rewrote_keeps_its_loops() {
    let mut m = Module::default();
    m.funcs.push(trial_loop());
    assert_eq!(pair_module(&mut m, None, &[1]).loops_paired, 0);
}

#[test]
fn a_counter_that_may_leave_the_exact_window_is_converted_each_trip() {
    let mut f = trial_loop();
    let stats = run_with(&mut f, Some((1 << 53) - 1));
    assert_eq!((stats.loops_paired, stats.float_counters), (1, 0));
}

#[test]
fn a_body_value_read_after_the_loop_keeps_the_loop() {
    let mut f = trial_loop();
    f.blocks[5].term = Terminator::Ret(Some(v(8)));
    assert_eq!(run(&mut f), 0);
}

#[test]
fn a_counter_not_stepped_by_a_constant_keeps_the_loop() {
    let mut f = trial_loop();
    f.blocks[3].insts[0].kind = InstKind::BinOp(BinOp::Add, v(1), v(7));
    assert_eq!(run(&mut f), 0);
}

#[test]
fn a_quotient_read_twice_keeps_the_loop() {
    let mut f = trial_loop();
    f.blocks[2].insts.push(i(4, InstKind::FpToSi(v(6))));
    f.values[4].ty = Type::I64;
    assert_eq!(run(&mut f), 0);
}

#[test]
fn a_call_in_the_body_keeps_the_loop() {
    let mut f = trial_loop();
    f.blocks[3].insts.insert(
        0,
        Inst {
            result: None,
            kind: InstKind::Call(torajs_core::ssa::FuncId(0), Vec::new()),
            origin: None,
        },
    );
    assert_eq!(run(&mut f), 0);
}

#[test]
fn a_dividend_converted_inside_the_loop_keeps_the_loop() {
    let mut f = trial_loop();
    let conv = f.blocks[0].insts.remove(1);
    f.blocks[2].insts.insert(0, conv);
    assert_eq!(run(&mut f), 0);
}
