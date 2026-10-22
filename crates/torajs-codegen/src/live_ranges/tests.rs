use super::LiveRanges;
use crate::coalesce::{Classes, build_copy_classes};
use crate::linear_scan::allocate_linear_scan;
use crate::liveness::compute_liveness;
use crate::reg::{Reg, aapcs64};
use crate::regalloc::collect_ret_value_ids;
use torajs_core::ssa::id_hash::HashMap;
use torajs_core::ssa::{
    BinOp, Block, BlockId, Function, IPred, Inst, InstKind, Operand, Terminator, Type, ValueId,
    ValueInfo,
};

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

fn add(r: u32, a: Operand, b: Operand) -> Inst {
    def(r, InstKind::BinOp(BinOp::Add, a, b))
}

fn block(id: u32, insts: Vec<Inst>, term: Terminator) -> Block {
    Block {
        id: BlockId(id),
        insts,
        term,
    }
}

/// `p0` is a Bool param; every other value is I64 unless listed in
/// `bools`.
fn func(nvals: u32, bools: &[u32], blocks: Vec<Block>) -> Function {
    Function {
        name: "f".into(),
        params: vec![ValueId(0)],
        ret: Type::I64,
        blocks,
        values: (0..nvals)
            .map(|i| ValueInfo {
                ty: if i == 0 || bools.contains(&i) {
                    Type::Bool
                } else {
                    Type::I64
                },
                name: None,
            })
            .collect(),
        current_origin: None,
    }
}

fn ranges(f: &Function) -> LiveRanges {
    let (intervals, blocks) = compute_liveness(f);
    let none = Classes::default();
    LiveRanges::compute(f, &blocks, &intervals, &none).fold(&none)
}

/// `v1` is defined in bb0 and read in bb2; bb1, laid out between them,
/// runs only on the path where `v1` is dead and computes `w2` there.
///
/// ```text
/// bb0: v1 = 3 + 4             ; slot 0
///      br p0 ? bb1 : bb2      ; slot 1
/// bb1: w2 = 1 + 2             ; slot 2
///      x3 = w2 + 1            ; slot 3
///      ret x3                 ; slot 4
/// bb2: y4 = v1 + 1            ; slot 5
///      ret y4                 ; slot 6
/// ```
fn hole_over_other_arm() -> Function {
    let cond = Terminator::CondBr {
        cond: v(0),
        then_blk: BlockId(1),
        else_blk: BlockId(2),
    };
    func(
        5,
        &[],
        vec![
            block(
                0,
                vec![add(1, Operand::ConstI64(3), Operand::ConstI64(4))],
                cond,
            ),
            block(
                1,
                vec![
                    add(2, Operand::ConstI64(1), Operand::ConstI64(2)),
                    add(3, v(2), Operand::ConstI64(1)),
                ],
                Terminator::Ret(Some(v(3))),
            ),
            block(
                2,
                vec![add(4, v(1), Operand::ConstI64(1))],
                Terminator::Ret(Some(v(4))),
            ),
        ],
    )
}

#[test]
fn ranges_skip_the_arm_where_the_value_is_dead() {
    let f = hole_over_other_arm();
    let r = ranges(&f);
    // written at slot 0 and live out of bb0, then read at bb2's head
    assert_eq!(r.of(1), &[(1, 3), (10, 10)]);
    assert_eq!(r.of(2), &[(5, 6)]);
    // a param keeps its hull
    assert_eq!(r.of(0), &[(0, 3)]);
    assert!(!LiveRanges::intersect(r.of(1), r.of(2)));
    // y4 is written by the instruction that reads v1 for the last time
    assert_eq!(r.of(4), &[(11, 12)]);
    assert!(!LiveRanges::intersect(r.of(1), r.of(4)));
    let (intervals, _) = compute_liveness(&f);
    assert!(r.within(&intervals));
}

#[test]
fn intersect_handles_touching_and_nested_ranges() {
    assert!(LiveRanges::intersect(&[(0, 3)], &[(3, 5)]));
    assert!(!LiveRanges::intersect(&[(0, 2), (8, 9)], &[(3, 7)]));
    assert!(LiveRanges::intersect(&[(0, 2), (8, 9)], &[(3, 8)]));
    assert!(LiveRanges::intersect(&[(4, 4)], &[(0, 1), (2, 10)]));
    assert!(!LiveRanges::intersect(&[], &[(0, 1)]));
}

#[test]
fn live_across_needs_both_halves_of_the_slot() {
    // live from the write at slot 1 through the read at slot 5
    let r = [(3, 10)];
    assert!(!LiveRanges::live_across(&r, 1), "defined there");
    assert!(LiveRanges::live_across(&r, 2));
    assert!(LiveRanges::live_across(&r, 4));
    assert!(
        !LiveRanges::live_across(&r, 5),
        "read there for the last time"
    );
    // a hole over slots 3 and 4
    let r = [(3, 5), (10, 12)];
    assert!(LiveRanges::live_across(&r, 2));
    assert!(!LiveRanges::live_across(&r, 3));
    assert!(!LiveRanges::live_across(&r, 4));
    assert!(LiveRanges::live_across(&r, 5));
    assert!(!LiveRanges::live_across(&[], 0));
}

#[test]
fn union_from_merges_and_drops_what_ends_before_from() {
    let a = [(0, 3), (6, 8), (12, 14)];
    let b = [(4, 5), (9, 10), (20, 21)];
    // (0, 3) ends before 5; (4, 5) is b's and stays; 6-8 and 9-10 touch
    assert_eq!(
        LiveRanges::union_from(&a, &b, 5),
        vec![(4, 10), (12, 14), (20, 21)]
    );
    assert_eq!(LiveRanges::union_from(&[], &b, 0), b.to_vec());
    assert_eq!(LiveRanges::union_from(&a, &[], 100), Vec::new());
}

/// Every pool register is held: `v1` and twenty `k`s live from bb0 to
/// the sum in bb3. bb1 runs only where `v1` is dead (it is read in bb2
/// alone), and `w` computed there finds no free register. When
/// `v1_read_in_bb1`, bb1 reads `v1` too and `w` must not share.
///
/// ```text
/// bb0: v1 = 3 + 4 ; k2..k21 = i + 0 ; br p0 ? bb1 : bb2
/// bb1: w22 = 1 + 2 ; x23 = w22 + (v1 | 1) ; c24 = x23 == 0 ; br c24 ? bb3 : bb3
/// bb2: y25 = v1 + 1 ; c26 = y25 == 0 ; br c26 ? bb3 : bb3
/// bb3: s = k2 + k3 + ... + k21 ; ret s
/// ```
fn full_pools(v1_read_in_bb1: bool) -> Function {
    let mut bb0 = vec![add(1, Operand::ConstI64(3), Operand::ConstI64(4))];
    for k in 2..22 {
        bb0.push(add(k, Operand::ConstI64(k as i64), Operand::ConstI64(0)));
    }
    let x_rhs = if v1_read_in_bb1 {
        v(1)
    } else {
        Operand::ConstI64(1)
    };
    let mut bb3 = vec![add(27, v(2), v(3))];
    for k in 4..22 {
        bb3.push(add(28 + k - 4, v(27 + k - 4), v(k)));
    }
    let last = 27 + 18;
    let eq0 = |r: u32, a: u32| def(r, InstKind::ICmp(IPred::Eq, v(a), Operand::ConstI64(0)));
    let both = |c: u32| Terminator::CondBr {
        cond: v(c),
        then_blk: BlockId(3),
        else_blk: BlockId(3),
    };
    func(
        last + 1,
        &[24, 26],
        vec![
            block(
                0,
                bb0,
                Terminator::CondBr {
                    cond: v(0),
                    then_blk: BlockId(1),
                    else_blk: BlockId(2),
                },
            ),
            block(
                1,
                vec![
                    add(22, Operand::ConstI64(1), Operand::ConstI64(2)),
                    add(23, v(22), x_rhs),
                    eq0(24, 23),
                ],
                both(24),
            ),
            block(
                2,
                vec![add(25, v(1), Operand::ConstI64(1)), eq0(26, 25)],
                both(26),
            ),
            block(3, bb3, Terminator::Ret(Some(v(last)))),
        ],
    )
}

#[test]
fn a_value_in_a_hole_shares_the_holders_register_when_the_pools_are_full() {
    let f = full_pools(false);
    let alloc = allocate_linear_scan(&f);
    let home = alloc.of(ValueId(1));
    assert!(matches!(home, Reg::Gpr(_)), "{home:?}");
    assert_eq!(alloc.of(ValueId(22)), home);
    assert_eq!(
        aapcs64::CALLER_SAVED_SCRATCH.len() + aapcs64::CALLEE_SAVED_SCRATCH.len(),
        21,
        "the shape fills every pool register exactly"
    );
}

#[test]
fn a_value_live_with_every_holder_does_not_share() {
    let f = full_pools(true);
    let alloc = allocate_linear_scan(&f);
    assert_ne!(alloc.of(ValueId(22)), alloc.of(ValueId(1)));
}

/// A copy class's ranges are the union of its members' ranges, and
/// keep the hole between the two arms the class is not live across.
///
/// ```text
/// bb0: br p0 ? bb1 : bb2      ; slot 0
/// bb1: v2 = 1 + 2             ; slot 1
///      v1 = copy v2           ; slot 2
///      br bb3                 ; slot 3
/// bb2: v3 = 3 + 4             ; slot 4
///      v1 = copy v3           ; slot 5
///      br bb3                 ; slot 6
/// bb3: v4 = v1 + 1            ; slot 7
///      ret v4                 ; slot 8
/// ```
#[test]
fn class_ranges_are_the_union_of_their_members() {
    let copy = |r: u32, src: u32| def(r, InstKind::Copy(Type::I64, v(src)));
    let f = func(
        5,
        &[],
        vec![
            block(
                0,
                Vec::new(),
                Terminator::CondBr {
                    cond: v(0),
                    then_blk: BlockId(1),
                    else_blk: BlockId(2),
                },
            ),
            block(
                1,
                vec![
                    add(2, Operand::ConstI64(1), Operand::ConstI64(2)),
                    copy(1, 2),
                ],
                Terminator::Br(BlockId(3)),
            ),
            block(
                2,
                vec![
                    add(3, Operand::ConstI64(3), Operand::ConstI64(4)),
                    copy(1, 3),
                ],
                Terminator::Br(BlockId(3)),
            ),
            block(
                3,
                vec![add(4, v(1), Operand::ConstI64(1))],
                Terminator::Ret(Some(v(4))),
            ),
        ],
    );
    let (mut intervals, blocks) = compute_liveness(&f);
    let ranges = LiveRanges::compute(&f, &blocks, &intervals, &Classes::default());
    // the copy's write in bb1 up to bb1's end, then the one in bb2 on
    // through bb3's read
    assert_eq!(ranges.of(1), &[(5, 7), (11, 14)]);
    assert_eq!(ranges.of(2), &[(3, 4)]);
    assert_eq!(ranges.of(3), &[(9, 10)]);
    let rets = collect_ret_value_ids(&f);
    let widened = crate::zext_load::widened_loads(&f);
    let classes = build_copy_classes(&f, &intervals, &blocks, &rets, &[], &widened);
    let leader = classes.leader(1);
    assert_eq!(classes.leader(2), leader);
    assert_eq!(classes.leader(3), leader);
    let ranges = ranges.fold(&classes);
    classes.fold_into(&mut intervals, &mut HashMap::default());
    assert_eq!(ranges.of(leader), &[(3, 7), (9, 14)]);
    assert!(ranges.within(&intervals));
}
