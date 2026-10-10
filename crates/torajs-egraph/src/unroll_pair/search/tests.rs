use super::super::tests::{i, trial_loop, v};
use super::super::{UnrollPairStats, pair_module};
use super::{LANES, PROLOGUE};
use crate::id_hash::HashMap;
use crate::interval::NumFact;
use torajs_core::ssa::{
    BinOp, BlockId, Function, IPred, InstKind, Module, Operand, Terminator, Type, ValueId,
    ValueInfo, VecInst, VecOp,
};

const N_HI: i128 = (1 << 31) - 1;

/// Runs the pass with `n = %0` in `[0, n_hi]` and the counter `%1` in
/// `[2, i_hi]`.
fn run(f: &mut Function, n_hi: Option<i128>, i_hi: i128) -> UnrollPairStats {
    let mut m = Module::default();
    m.funcs.push(f.clone());
    let mut known = HashMap::from_iter([(ValueId(1), NumFact::new(2, i_hi))]);
    if let Some(hi) = n_hi {
        known.insert(ValueId(0), NumFact::new(0, hi));
    }
    let stats = pair_module(&mut m, Some(std::slice::from_ref(&known)), &[]);
    *f = m.funcs.pop().unwrap();
    stats
}

fn kinds(f: &Function, b: u32) -> Vec<&InstKind> {
    f.blocks[b as usize].insts.iter().map(|i| &i.kind).collect()
}

fn vec_op(kind: &InstKind) -> Option<&VecInst> {
    match kind {
        InstKind::Vec(vi) => Some(vi),
        _ => None,
    }
}

#[test]
fn a_trial_division_loop_is_searched_ahead_and_kept() {
    let mut f = trial_loop();
    let before = f.clone();
    let stats = run(&mut f, Some(N_HI), 46_341);
    assert_eq!((stats.loops_searched, stats.loops_paired), (1, 0));
    // the original loop, its exits and its entry are untouched but for
    // the latch, which enters the search once the prologue is done
    for b in [1, 2, 4, 5] {
        assert_eq!(f.blocks[b], before.blocks[b]);
    }
    assert_eq!(f.blocks[0].term, Terminator::Br(BlockId(1)));
    let (s, fb, fl, fh) = (BlockId(6), BlockId(7), BlockId(8), BlockId(9));
    assert!(matches!(
        kinds(&f, 0).last(),
        Some(InstKind::BinOp(BinOp::Add, Operand::Value(ValueId(1)), Operand::ConstI64(k)))
            if *k == PROLOGUE
    ));
    assert!(matches!(
        kinds(&f, 3).last(),
        Some(InstKind::ICmp(IPred::Eq, Operand::Value(ValueId(1)), _))
    ));
    assert!(matches!(
        f.blocks[3].term,
        Terminator::CondBr { then_blk, else_blk: BlockId(1), .. } if then_blk == s
    ));
    // S tests the group's last divisor: on to F, else back to the loop
    let entry = kinds(&f, 6);
    let n = entry.len();
    assert!(matches!(
        entry[n - 3],
        InstKind::BinOp(BinOp::Add, Operand::Value(ValueId(1)), Operand::ConstI64(k)) if *k == LANES - 1
    ));
    assert!(matches!(entry[n - 2], InstKind::BinOp(BinOp::Mul, _, _)));
    assert!(matches!(
        entry[n - 1],
        InstKind::ICmp(IPred::Sle, _, Operand::Value(ValueId(0)))
    ));
    assert!(matches!(
        f.blocks[6].term,
        Terminator::CondBr { then_blk, else_blk: BlockId(1), .. } if then_blk == fb
    ));
    // F: per vector a divide, its truncation and the equality mask;
    // a set lane goes to FH, which leaves through the divisibility exit
    let ops: Vec<&VecInst> = kinds(&f, 7).into_iter().filter_map(vec_op).collect();
    let vectors = (LANES / 2) as usize;
    let divides = ops
        .iter()
        .filter(|vi| matches!(vi, VecInst::BinOp(VecOp::FDiv, _, _)))
        .count();
    assert_eq!(divides, vectors);
    assert!(matches!(ops.last(), Some(VecInst::Reduce(VecOp::Add, _))));
    assert!(matches!(
        f.blocks[7].term,
        Terminator::CondBr { then_blk, else_blk, .. } if then_blk == fh && else_blk == fl
    ));
    let lanes = kinds(&f, 9)
        .iter()
        .filter(|k| matches!(vec_op(k), Some(VecInst::Lane(..))))
        .count();
    assert_eq!(lanes as i64, LANES - 1);
    assert_eq!(f.blocks[9].insts.last().unwrap().result, Some(ValueId(1)));
    assert_eq!(f.blocks[9].term, Terminator::Br(BlockId(5)));
    // FL: the counter steps a group, the lanes with it, then the test
    let step = kinds(&f, 8);
    assert!(matches!(
        step[0],
        InstKind::BinOp(BinOp::Add, Operand::Value(ValueId(1)), Operand::ConstI64(k)) if *k == LANES
    ));
    assert_eq!(f.blocks[8].insts[1].result, Some(ValueId(1)));
    assert!(matches!(
        f.blocks[8].term,
        Terminator::CondBr { then_blk, else_blk: BlockId(1), .. } if then_blk == fb
    ));
}

#[test]
fn a_loop_writing_another_cell_is_not_searched() {
    let mut f = trial_loop();
    f.values.push(ValueInfo {
        ty: Type::I64,
        name: None,
    });
    f.values.push(ValueInfo {
        ty: Type::I64,
        name: None,
    });
    let s = Operand::ConstI64(0);
    f.blocks[0].insts.push(i(12, InstKind::Copy(Type::I64, s)));
    let one = Operand::ConstI64(1);
    f.blocks[3]
        .insts
        .push(i(13, InstKind::BinOp(BinOp::Add, v(12), one)));
    f.blocks[3]
        .insts
        .push(i(12, InstKind::Copy(Type::I64, v(13))));
    assert_eq!(run(&mut f, Some(N_HI), 46_341).loops_searched, 0);
}

#[test]
fn a_counter_whose_square_may_wrap_is_not_searched() {
    let mut f = trial_loop();
    let stats = run(&mut f, Some(N_HI), (1 << 53) - 2);
    assert_eq!((stats.loops_searched, stats.loops_paired), (0, 1));
}

#[test]
fn a_dividend_without_a_window_is_not_searched() {
    let mut f = trial_loop();
    assert_eq!(run(&mut f, None, 46_341).loops_searched, 0);
    let mut f = trial_loop();
    assert_eq!(run(&mut f, Some(1 << 53), 46_341).loops_searched, 0);
}

#[test]
fn a_header_test_holding_for_large_counters_is_not_searched() {
    let mut f = trial_loop();
    f.blocks[1].insts[1].kind = InstKind::ICmp(IPred::Sge, v(2), v(0));
    assert_eq!(run(&mut f, Some(N_HI), 46_341).loops_searched, 0);
}

#[test]
fn a_second_body_exit_is_not_searched() {
    let mut f = trial_loop();
    // the latch leaves when the counter reaches 100
    f.values.push(ValueInfo {
        ty: Type::Bool,
        name: None,
    });
    f.blocks[3].insts.insert(
        0,
        i(12, InstKind::ICmp(IPred::Eq, v(1), Operand::ConstI64(100))),
    );
    let latch = f.blocks[3].clone();
    let mut tail = latch.clone();
    tail.id = BlockId(6);
    tail.insts.remove(0);
    f.blocks[3].insts.truncate(1);
    f.blocks[3].term = Terminator::CondBr {
        cond: v(12),
        then_blk: BlockId(4),
        else_blk: BlockId(6),
    };
    f.blocks.push(tail);
    assert_eq!(run(&mut f, Some(N_HI), 46_341).loops_searched, 0);
}
