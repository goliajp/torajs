//! An inner test lane 0 decides for every lane: `d * d <= i` against
//! the outer counter, in each spelling, and a bound it does not decide.

use super::interp::run;
use super::shared::divisor_sums;
use super::*;

use crate::id_hash::HashSet;

/// `divisor_sums` with the inner test `%7` replaced, staying in the
/// inner loop when it is `stay`.
fn with_test(test: InstKind, stay: bool) -> Function {
    let mut f = divisor_sums();
    f.blocks[3].insts[1].kind = test;
    f.blocks[3].term = if stay { cbr(7, 4, 5) } else { cbr(7, 5, 4) };
    f
}

/// Compares reading a square: the inner tests.
fn square_tests(f: &Function) -> usize {
    let insts = || f.blocks.iter().flat_map(|b| &b.insts);
    let squares: HashSet<ValueId> = insts()
        .filter(|i| matches!(i.kind, InstKind::BinOp(BinOp::Mul, _, _)))
        .filter_map(|i| i.result)
        .collect();
    let square = |o: &Operand| matches!(o, Operand::Value(x) if squares.contains(x));
    insts()
        .filter(|i| matches!(&i.kind, InstKind::ICmp(_, a, b) if square(a) || square(b)))
        .count()
}

fn same_sums(f: &Function, g: &Function) {
    for bound in [-2, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 15, 16, 17, 100, 257] {
        assert_eq!(
            run(f, &[bound], 1 << 22),
            run(g, &[bound], 1 << 24),
            "n {bound}"
        );
    }
}

#[test]
fn lane_zero_alone_tests_a_rising_bound() {
    let spellings = [
        (InstKind::ICmp(IPred::Sle, v(6), v(2)), true),
        (InstKind::ICmp(IPred::Sge, v(2), v(6)), true),
        (InstKind::ICmp(IPred::Sgt, v(6), v(2)), false),
        (InstKind::ICmp(IPred::Slt, v(2), v(6)), false),
    ];
    for (test, stay) in spellings {
        let f = with_test(test, stay);
        let (g, n) = jam(&f);
        assert_eq!(n, 1);
        // the original header, the fused one, and each lane's own
        assert_eq!(square_tests(&g), 2 + MIN_LANES);
        same_sums(&f, &g);
    }
}

#[test]
fn a_bound_off_the_counter_is_tested_per_lane() {
    // `d * d <= i + 0`: the bound is a lane value, not the counter
    let mut f = with_test(InstKind::ICmp(IPred::Sle, v(6), v(15)), true);
    let i0 = InstKind::BinOp(BinOp::Add, v(2), c(0));
    f.blocks[2].insts.push(inst(15, i0));
    f.values.push(ValueInfo {
        ty: Type::I64,
        name: None,
    });
    let (g, n) = jam(&f);
    assert_eq!(n, 1);
    assert_eq!(square_tests(&g), 1 + 2 * MIN_LANES);
    same_sums(&f, &g);
}
