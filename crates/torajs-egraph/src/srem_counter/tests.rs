use super::*;

fn rewrite(f: &mut Function, facts: &HashMap<ValueId, NumFact>, stats: &mut SremCounterStats) {
    rewrite_function(f, facts, stats, &mut LoopShapes::new(1), 0);
}
use torajs_core::ssa::{Block, Terminator, ValueInfo};

fn v(id: u32) -> Operand {
    Operand::Value(ValueId(id))
}

fn i(r: u32, kind: InstKind) -> Inst {
    inst(ValueId(r), kind)
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

fn func(blocks: Vec<Block>) -> Function {
    Function {
        name: "f".into(),
        params: vec![ValueId(0)],
        ret: Type::I64,
        blocks,
        values: (0..10)
            .map(|_| ValueInfo {
                ty: Type::I64,
                name: None,
            })
            .collect(),
        current_origin: None,
    }
}

fn step(by: i64) -> Vec<Inst> {
    vec![
        i(5, InstKind::BinOp(BinOp::Add, v(1), Operand::ConstI64(by))),
        i(1, InstKind::Copy(Type::I64, v(5))),
    ]
}

/// `for (i = start; i < 100; i++) x = i % p` with `p = %0`, `i = %1`,
/// the remainder `%3` in `bb2` ahead of the step.
fn plain_loop(start: Operand, p: Operand, latch: Vec<Inst>) -> Function {
    let mut body = vec![i(3, InstKind::BinOp(BinOp::SRem, v(1), p))];
    body.extend(latch);
    func(vec![
        block(
            0,
            vec![i(1, InstKind::Copy(Type::I64, start))],
            Terminator::Br(BlockId(1)),
        ),
        block(
            1,
            vec![i(
                2,
                InstKind::ICmp(IPred::Slt, v(1), Operand::ConstI64(100)),
            )],
            cond(2, 2, 3),
        ),
        block(2, body, Terminator::Br(BlockId(1))),
        block(3, vec![], Terminator::Ret(Some(v(1)))),
    ])
}

/// frem_guard's shape: `%6 = p - 1`, `%7 = %6 >>> 53` before the loop,
/// the integer arm `bb5` taken when `%7 == 0`.
fn guarded_loop() -> Function {
    func(vec![
        block(
            0,
            vec![
                i(6, InstKind::BinOp(BinOp::Sub, v(0), Operand::ConstI64(1))),
                i(7, InstKind::BinOp(BinOp::LShr, v(6), Operand::ConstI64(53))),
                i(1, InstKind::Copy(Type::I64, Operand::ConstI64(0))),
            ],
            Terminator::Br(BlockId(1)),
        ),
        block(
            1,
            vec![i(
                2,
                InstKind::ICmp(IPred::Slt, v(1), Operand::ConstI64(100)),
            )],
            cond(2, 2, 3),
        ),
        block(
            2,
            vec![i(8, InstKind::ICmp(IPred::Ne, v(7), Operand::ConstI64(0)))],
            cond(8, 4, 5),
        ),
        block(3, vec![], Terminator::Ret(Some(v(1)))),
        block(4, vec![], Terminator::Br(BlockId(6))),
        block(
            5,
            vec![i(3, InstKind::BinOp(BinOp::SRem, v(1), v(0)))],
            Terminator::Br(BlockId(6)),
        ),
        block(6, step(1), Terminator::Br(BlockId(1))),
    ])
}

fn run(f: &mut Function, i_lo: i128, p_lo: i128) -> SremCounterStats {
    let facts = HashMap::from_iter([
        (ValueId(0), NumFact::new(p_lo, 1 << 40)),
        (ValueId(1), NumFact::new(i_lo, 100)),
    ]);
    let mut stats = SremCounterStats::default();
    rewrite(f, &facts, &mut stats);
    stats
}

const NONE: SremCounterStats = SremCounterStats {
    counters: 0,
    srems: 0,
};

/// One move of the counter: compare against `pm1`, increment,
/// select, and the copy back into the counter.
fn assert_counter_step(insts: &[Inst], r: ValueId, pm1: Operand) {
    let rv = Operand::Value(r);
    let res = |k: usize| Operand::Value(insts[k].result.unwrap());
    assert_eq!(insts[0].kind, InstKind::ICmp(IPred::Eq, rv, pm1));
    assert_eq!(
        insts[1].kind,
        InstKind::BinOp(BinOp::Add, rv, Operand::ConstI64(1))
    );
    assert_eq!(
        insts[2].kind,
        InstKind::Select(Type::I64, res(0), Operand::ConstI64(0), res(1))
    );
    assert_eq!(insts[3].kind, InstKind::Copy(Type::I64, res(2)));
    assert_eq!(insts[3].result, Some(r));
}

#[test]
fn a_proven_divisor_becomes_a_wrapping_counter() {
    let mut f = plain_loop(Operand::ConstI64(0), v(0), step(1));
    assert_eq!(
        run(&mut f, 0, 1),
        SremCounterStats {
            counters: 1,
            srems: 1
        }
    );
    // entry: `pm1 = p - 1`, `r = copy pm1`, one move short of 0
    let pre = &f.blocks[0].insts;
    let pm1 = Operand::Value(pre[1].result.unwrap());
    assert_eq!(
        pre[1].kind,
        InstKind::BinOp(BinOp::Sub, v(0), Operand::ConstI64(1))
    );
    assert_eq!(pre[2].kind, InstKind::Copy(Type::I64, pm1));
    let r = pre[2].result.unwrap();
    // the body's first block moves it ahead of the site; the header
    // stays a bare test
    assert_eq!(f.blocks[1].insts.len(), 1);
    let body = &f.blocks[2].insts;
    assert_counter_step(&body[..4], r, pm1);
    assert_eq!(body[4].kind, InstKind::Copy(Type::I64, Operand::Value(r)));
    assert_eq!(body.len(), 7);
}

#[test]
fn a_nonzero_start_divides_once_before_the_loop() {
    let mut f = plain_loop(Operand::ConstI64(7), v(0), step(1));
    assert_eq!(run(&mut f, 0, 1).counters, 1);
    let pre = &f.blocks[0].insts;
    let res = |k: usize| Operand::Value(pre[k].result.unwrap());
    // `(i + p - 1) mod p`, one move short of `i mod p`
    assert_eq!(pre[2].kind, InstKind::BinOp(BinOp::Add, v(1), res(1)));
    assert_eq!(pre[3].kind, InstKind::BinOp(BinOp::SRem, res(2), v(0)));
    assert_eq!(pre[4].kind, InstKind::Copy(Type::I64, res(3)));
}

#[test]
fn an_unbounded_divisor_starts_without_adding_to_the_counter() {
    // `i + p - 1` could wrap: `s == 0 ? p - 1 : s - 1` instead
    let mut f = plain_loop(Operand::ConstI64(7), v(0), step(1));
    let facts = HashMap::from_iter([
        (ValueId(0), NumFact::new(1, 1 << 63)),
        (ValueId(1), NumFact::new(0, 100)),
    ]);
    let mut stats = SremCounterStats::default();
    rewrite(&mut f, &facts, &mut stats);
    assert_eq!(stats.counters, 1);
    let pre = &f.blocks[0].insts;
    let res = |k: usize| Operand::Value(pre[k].result.unwrap());
    assert_eq!(pre[2].kind, InstKind::BinOp(BinOp::SRem, v(1), v(0)));
    assert_eq!(
        pre[3].kind,
        InstKind::ICmp(IPred::Eq, res(2), Operand::ConstI64(0))
    );
    assert_eq!(
        pre[4].kind,
        InstKind::BinOp(BinOp::Sub, res(2), Operand::ConstI64(1))
    );
    assert_eq!(
        pre[5].kind,
        InstKind::Select(Type::I64, res(3), res(1), res(4))
    );
}

#[test]
fn a_site_after_the_step_moves_the_counter_with_the_step() {
    // `i++; x = i % p` in one block: the counter moves right after `i`
    let mut f = plain_loop(Operand::ConstI64(0), v(0), step(1));
    let site = f.blocks[2].insts.remove(0);
    f.blocks[2].insts.push(site);
    assert_eq!(run(&mut f, 0, 1).srems, 1);
    let pre = &f.blocks[0].insts;
    let pm1 = Operand::Value(pre[1].result.unwrap());
    assert_eq!(pre[2].kind, InstKind::Copy(Type::I64, Operand::ConstI64(0)));
    let r = pre[2].result.unwrap();
    assert_eq!(f.blocks[1].insts.len(), 1);
    let body = &f.blocks[2].insts;
    assert_counter_step(&body[2..6], r, pm1);
    assert_eq!(body[6].kind, InstKind::Copy(Type::I64, Operand::Value(r)));
}

#[test]
fn the_window_test_proves_the_divisor_and_lends_its_p_minus_1() {
    let mut f = guarded_loop();
    assert_eq!(run(&mut f, 0, i128::MIN).srems, 1);
    let r = f.blocks[0].insts[3].result.unwrap();
    assert_eq!(f.blocks[0].insts[3].kind, InstKind::Copy(Type::I64, v(6)));
    assert_eq!(
        f.blocks[5].insts[0].kind,
        InstKind::Copy(Type::I64, Operand::Value(r))
    );
    assert_counter_step(&f.blocks[2].insts[..4], r, v(6));
}

#[test]
fn a_guarded_nonzero_start_divides_by_one_when_the_divisor_is_low() {
    let mut f = guarded_loop();
    f.blocks[0].insts[2] = i(1, InstKind::Copy(Type::I64, Operand::ConstI64(5)));
    assert_eq!(run(&mut f, 0, i128::MIN).counters, 1);
    let pre = &f.blocks[0].insts;
    let low = Operand::Value(pre[3].result.unwrap());
    assert_eq!(
        pre[3].kind,
        InstKind::ICmp(IPred::Slt, v(0), Operand::ConstI64(1))
    );
    assert_eq!(
        pre[4].kind,
        InstKind::Select(Type::I64, low, Operand::ConstI64(1), v(0))
    );
}

#[test]
fn a_test_that_does_not_guard_the_site_keeps_srem() {
    // the remainder runs on the arm the test sends the failures to
    let mut f = guarded_loop();
    f.blocks[2].term = cond(8, 5, 4);
    assert_eq!(run(&mut f, 0, i128::MIN), NONE);
}

#[test]
fn a_possibly_negative_dividend_keeps_srem() {
    let mut f = plain_loop(Operand::ConstI64(-3), v(0), step(1));
    assert_eq!(run(&mut f, -3, 1), NONE);
}

#[test]
fn a_divisor_not_proven_positive_keeps_srem() {
    let mut f = plain_loop(Operand::ConstI64(0), v(0), step(1));
    assert_eq!(run(&mut f, 0, 0), NONE);
}

#[test]
fn a_step_other_than_one_keeps_srem() {
    let mut f = plain_loop(Operand::ConstI64(0), v(0), step(2));
    assert_eq!(run(&mut f, 0, 1), NONE);
}

#[test]
fn a_divisor_the_loop_writes_keeps_srem() {
    let mut latch = step(1);
    latch.push(i(0, InstKind::Copy(Type::I64, v(5))));
    let mut f = plain_loop(Operand::ConstI64(0), v(0), latch);
    f.params.clear();
    assert_eq!(run(&mut f, 0, 1), NONE);
}

#[test]
fn constant_divisors_skip_powers_of_two() {
    let mut f = plain_loop(Operand::ConstI64(0), Operand::ConstI64(8), step(1));
    assert_eq!(run(&mut f, 0, 1), NONE);
    let mut f = plain_loop(Operand::ConstI64(0), Operand::ConstI64(6), step(1));
    assert_eq!(run(&mut f, 0, 1).counters, 1);
    assert_counter_step(
        &f.blocks[2].insts[..4],
        f.blocks[0].insts[1].result.unwrap(),
        Operand::ConstI64(5),
    );
}

#[test]
fn a_compare_against_one_before_the_loop_proves_the_divisor() {
    // `if (p < 1) bail` ahead of the loop, the start in its own block
    let mut f = plain_loop(Operand::ConstI64(0), v(0), step(1));
    let first = f.blocks[0].insts.remove(0);
    f.blocks[0].insts = vec![i(9, InstKind::ICmp(IPred::Slt, v(0), Operand::ConstI64(1)))];
    f.blocks[0].term = cond(9, 3, 4);
    f.blocks
        .push(block(4, vec![first], Terminator::Br(BlockId(1))));
    assert_eq!(run(&mut f, 0, i128::MIN).srems, 1);
    // `p - 1` is computed in the entry block, the start is one move
    // short of 0
    let entry = &f.blocks[4].insts;
    assert_eq!(
        entry[1].kind,
        InstKind::BinOp(BinOp::Sub, v(0), Operand::ConstI64(1))
    );
    let pm1 = Operand::Value(entry[1].result.unwrap());
    assert_eq!(entry[2].kind, InstKind::Copy(Type::I64, pm1));
}

#[test]
fn a_header_copy_of_the_counter_is_followed() {
    // `%4 = copy %1` in the header, the remainder of `%4` in the body
    let facts = HashMap::from_iter([
        (ValueId(0), NumFact::new(1, 1 << 40)),
        (ValueId(4), NumFact::new(0, 100)),
    ]);
    let mut f = plain_loop(Operand::ConstI64(0), v(0), step(1));
    f.blocks[1]
        .insts
        .insert(0, i(4, InstKind::Copy(Type::I64, v(1))));
    f.blocks[2].insts[0] = i(3, InstKind::BinOp(BinOp::SRem, v(4), v(0)));
    let mut stats = SremCounterStats::default();
    rewrite(&mut f, &facts, &mut stats);
    assert_eq!(stats.srems, 1);
    // a def of the counter between the copy and the site keeps it
    let mut f = plain_loop(Operand::ConstI64(0), v(0), step(1));
    f.blocks[1]
        .insts
        .insert(0, i(4, InstKind::Copy(Type::I64, v(1))));
    f.blocks[2].insts.remove(0);
    f.blocks[2]
        .insts
        .push(i(3, InstKind::BinOp(BinOp::SRem, v(4), v(0))));
    let mut stats = SremCounterStats::default();
    rewrite(&mut f, &facts, &mut stats);
    assert_eq!(stats.srems, 0);
}

#[test]
fn a_header_that_is_not_a_bare_test_moves_the_counter_at_its_top() {
    // the remainder in the header: the move goes ahead of it there
    let mut f = plain_loop(Operand::ConstI64(0), v(0), step(1));
    let site = f.blocks[2].insts.remove(0);
    f.blocks[1].insts.insert(0, site);
    assert_eq!(run(&mut f, 0, 1).srems, 1);
    let r = f.blocks[0].insts[2].result.unwrap();
    let pm1 = Operand::Value(f.blocks[0].insts[1].result.unwrap());
    let h = &f.blocks[1].insts;
    assert_counter_step(&h[..4], r, pm1);
    assert_eq!(h[4].kind, InstKind::Copy(Type::I64, Operand::Value(r)));
    assert_eq!(f.blocks[2].insts.len(), 2);
}

#[test]
fn a_loop_that_is_not_top_tested_moves_at_its_header() {
    // a do-while: the header is the body and ends in the step
    let mut f = plain_loop(Operand::ConstI64(0), v(0), step(1));
    let body = std::mem::take(&mut f.blocks[2].insts);
    f.blocks[1].insts = body;
    f.blocks[1].insts.push(i(
        2,
        InstKind::ICmp(IPred::Slt, v(1), Operand::ConstI64(100)),
    ));
    f.blocks[1].term = cond(2, 1, 3);
    f.blocks[2].term = Terminator::Br(BlockId(1));
    assert_eq!(run(&mut f, 0, 1).srems, 1);
    let r = f.blocks[0].insts[2].result.unwrap();
    let pm1 = Operand::Value(f.blocks[0].insts[1].result.unwrap());
    let h = &f.blocks[1].insts;
    assert_counter_step(&h[..4], r, pm1);
    assert_eq!(h[4].kind, InstKind::Copy(Type::I64, Operand::Value(r)));
}
