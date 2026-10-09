use super::LiveRanges;
use crate::coalesce::{Classes, build_copy_classes};
use crate::linear_scan::allocate_linear_scan;
use crate::liveness::compute_liveness;
use crate::reg::{Reg, aapcs64};
use crate::regalloc::collect_ret_value_ids;
use torajs_core::ssa::id_hash::HashMap;
use torajs_core::ssa::{
    BinOp, Block, BlockId, FuncId, Function, Inst, InstKind, Operand, Terminator, Type, ValueId,
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

/// `p0` is a Bool param; every other value is I64.
fn func(nvals: u32, blocks: Vec<Block>) -> Function {
    Function {
        name: "f".into(),
        params: vec![ValueId(0)],
        ret: Type::I64,
        blocks,
        values: (0..nvals)
            .map(|i| ValueInfo {
                ty: if i == 0 { Type::Bool } else { Type::I64 },
                name: None,
            })
            .collect(),
        current_origin: None,
    }
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

fn ranges(f: &Function) -> LiveRanges {
    let (intervals, ranges, _) = compute_liveness(f);
    ranges.fold(&intervals, &Classes::default())
}

#[test]
fn ranges_skip_the_arm_where_the_value_is_dead() {
    let f = hole_over_other_arm();
    let r = ranges(&f);
    // live out of bb0, then live into bb2 from the slot before its head
    assert_eq!(r.of(1), &[(0, 1), (4, 5)]);
    assert_eq!(r.of(2), &[(2, 3)]);
    // a param keeps its hull
    assert_eq!(r.of(0), &[(0, 6)]);
    assert_eq!(r.next_from(1, 2, 0), Some((1, (4, 5))));
    assert_eq!(r.next_from(1, 6, 0), None);
    assert!(!LiveRanges::intersect(r.of(1), r.of(2)));
    assert!(LiveRanges::intersect(r.of(1), r.of(4)));
}

#[test]
fn intersect_handles_touching_and_nested_ranges() {
    assert!(LiveRanges::intersect(&[(0, 3)], &[(3, 5)]));
    assert!(!LiveRanges::intersect(&[(0, 2), (8, 9)], &[(3, 7)]));
    assert!(LiveRanges::intersect(&[(0, 2), (8, 9)], &[(3, 8)]));
    assert!(LiveRanges::intersect(&[(4, 4)], &[(0, 1), (2, 10)]));
    assert!(!LiveRanges::intersect(&[], &[(0, 1)]));
}

/// The value computed in the hole takes the register of the value that
/// is dead there.
#[test]
fn value_in_a_hole_shares_the_holders_register() {
    let alloc = allocate_linear_scan(&hole_over_other_arm());
    let home = alloc.of(ValueId(1));
    assert!(matches!(home, Reg::Gpr(_)));
    assert_eq!(alloc.of(ValueId(2)), home);
}

/// A call inside the hole is not crossed: the value stays in a
/// caller-saved register with nothing saved around the call.
///
/// ```text
/// bb0: v1 = 3 + 4             ; slot 0
///      br p0 ? bb1 : bb2      ; slot 1
/// bb1: c2 = call f0()         ; slot 2
///      ret c2                 ; slot 3
/// bb2: y3 = v1 + 1            ; slot 4
///      ret y3                 ; slot 5
/// ```
#[test]
fn call_in_a_hole_is_not_crossed() {
    let cond = Terminator::CondBr {
        cond: v(0),
        then_blk: BlockId(1),
        else_blk: BlockId(2),
    };
    let f = func(
        4,
        vec![
            block(
                0,
                vec![add(1, Operand::ConstI64(3), Operand::ConstI64(4))],
                cond,
            ),
            block(
                1,
                vec![def(2, InstKind::Call(FuncId(0), Vec::new()))],
                Terminator::Ret(Some(v(2))),
            ),
            block(
                2,
                vec![add(3, v(1), Operand::ConstI64(1))],
                Terminator::Ret(Some(v(3))),
            ),
        ],
    );
    let alloc = allocate_linear_scan(&f);
    let Reg::Gpr(g) = alloc.of(ValueId(1)) else {
        panic!("v1 has no register");
    };
    assert!(aapcs64::CALLER_SAVED_SCRATCH.contains(&g));
    assert!(alloc.call_saves.is_empty());
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
    let (mut intervals, ranges, blocks) = compute_liveness(&f);
    assert_eq!(ranges.of(1), &[(2, 3), (5, 7)]);
    assert_eq!(ranges.of(2), &[(1, 2)]);
    assert_eq!(ranges.of(3), &[(4, 5)]);
    let rets = collect_ret_value_ids(&f);
    let classes = build_copy_classes(&f, &intervals, &blocks, &rets, &[]);
    let leader = classes.leader(1);
    assert_eq!(classes.leader(2), leader);
    assert_eq!(classes.leader(3), leader);
    classes.fold_into(&mut intervals, &mut HashMap::default());
    let ranges = ranges.fold(&intervals, &classes);
    assert_eq!(ranges.of(leader), &[(1, 3), (4, 7)]);
    assert!(ranges.within(&intervals));
}
