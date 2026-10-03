//! Lanes sharing what they compute alike: the trial divisor of
//! `for (i = 1; i <= n; i++) { s = 0; for (d = 1; d * d <= i; d++)
//! s = i % d == 0 ? s + d : s; total += s }`.

use super::interp::run;
use super::*;

/// params `n`; `%1 total, %2 i, %4 s, %5 d`.
///
///   b0: total=0; i=1       b3: d*d <= i ? b4 : b5
///   b1: i <= n ? b2 : b6   b4: s = i%d == 0 ? s+d : s; d++ → b3
///   b2: s=0; d=1 → b3      b5: total += s; i++ → b1     b6: ret total
fn divisor_sums() -> Function {
    let blocks = vec![
        blk(0, vec![copy(1, c(0)), copy(2, c(1))], br(1)),
        blk(
            1,
            vec![inst(3, InstKind::ICmp(IPred::Sle, v(2), v(0)))],
            cbr(3, 2, 6),
        ),
        blk(2, vec![copy(4, c(0)), copy(5, c(1))], br(3)),
        blk(
            3,
            vec![
                inst(6, InstKind::BinOp(BinOp::Mul, v(5), v(5))),
                inst(7, InstKind::ICmp(IPred::Sle, v(6), v(2))),
            ],
            cbr(7, 4, 5),
        ),
        blk(
            4,
            vec![
                inst(8, InstKind::BinOp(BinOp::SRem, v(2), v(5))),
                inst(9, InstKind::ICmp(IPred::Eq, v(8), c(0))),
                inst(10, InstKind::BinOp(BinOp::Add, v(4), v(5))),
                inst(11, InstKind::Select(Type::I64, v(9), v(10), v(4))),
                inst(12, InstKind::BinOp(BinOp::Add, v(5), c(1))),
                copy(4, v(11)),
                copy(5, v(12)),
            ],
            br(3),
        ),
        blk(
            5,
            vec![
                inst(13, InstKind::BinOp(BinOp::Add, v(1), v(4))),
                inst(14, InstKind::BinOp(BinOp::Add, v(2), c(1))),
                copy(1, v(13)),
                copy(2, v(14)),
            ],
            br(1),
        ),
        blk(6, vec![], Terminator::Ret(Some(v(1)))),
    ];
    Function {
        name: "f".into(),
        params: vec![ValueId(0)],
        ret: Type::I64,
        blocks,
        values: (0..15)
            .map(|_| ValueInfo {
                ty: Type::I64,
                name: None,
            })
            .collect(),
        current_origin: None,
    }
}

#[test]
fn shared_divisor_gives_the_same_sums() {
    let f = divisor_sums();
    let (g, n) = jam(&f);
    assert_eq!(n, 1);
    for bound in [-2, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 15, 16, 17, 100, 257] {
        assert_eq!(
            run(&f, &[bound], 1 << 22),
            run(&g, &[bound], 1 << 24),
            "n {bound}"
        );
    }
}

#[test]
fn lanes_step_one_shared_divisor() {
    let (g, _) = jam(&divisor_sums());
    let count = |b: &Block, pick: &dyn Fn(&InstKind) -> bool| {
        b.insts.iter().filter(|i| pick(&i.kind)).count()
    };
    let is_srem = |k: &InstKind| matches!(k, InstKind::BinOp(BinOp::SRem, _, _));
    let fused = g
        .blocks
        .iter()
        .find(|b| count(b, &is_srem) == LANES)
        .expect("one fused body");
    // every lane divides by the one divisor, stepped once
    let steps = count(fused, &|k| {
        matches!(k, InstKind::BinOp(BinOp::Add, _, Operand::ConstI64(1)))
    });
    assert_eq!(steps, 1);
    let mut divisors: Vec<Operand> = fused
        .insts
        .iter()
        .filter_map(|i| match i.kind {
            InstKind::BinOp(BinOp::SRem, _, d) => Some(d),
            _ => None,
        })
        .collect();
    divisors.dedup();
    assert_eq!(divisors.len(), 1);
    // its square: the original header, the fused test, and once per
    // lane finishing alone
    let squares: usize = g
        .blocks
        .iter()
        .map(|b| count(b, &|k| matches!(k, InstKind::BinOp(BinOp::Mul, _, _))))
        .sum();
    assert_eq!(squares, 2 + LANES);
}

/// `for (k = 1; k <= n; k++) { r = 0; i = 2; while (i <= m)
/// { r = (r + k) % i; i++ } total += r }`, params `n, m`: the inner
/// test reads only shared values.
fn survivors() -> Function {
    // %0 n, %1 m, %2 total, %3 k, %4 test, %5 r, %6 i, %7 inner test,
    // %8 r+k, %9 (r+k)%i, %10 i+1, %11 total+r, %12 k+1
    let blocks = vec![
        blk(0, vec![copy(2, c(0)), copy(3, c(1))], br(1)),
        blk(
            1,
            vec![inst(4, InstKind::ICmp(IPred::Sle, v(3), v(0)))],
            cbr(4, 2, 6),
        ),
        blk(2, vec![copy(5, c(0)), copy(6, c(2))], br(3)),
        blk(
            3,
            vec![inst(7, InstKind::ICmp(IPred::Sle, v(6), v(1)))],
            cbr(7, 4, 5),
        ),
        blk(
            4,
            vec![
                inst(8, InstKind::BinOp(BinOp::Add, v(5), v(3))),
                inst(9, InstKind::BinOp(BinOp::SRem, v(8), v(6))),
                inst(10, InstKind::BinOp(BinOp::Add, v(6), c(1))),
                copy(5, v(9)),
                copy(6, v(10)),
            ],
            br(3),
        ),
        blk(
            5,
            vec![
                inst(11, InstKind::BinOp(BinOp::Add, v(2), v(5))),
                inst(12, InstKind::BinOp(BinOp::Add, v(3), c(1))),
                copy(2, v(11)),
                copy(3, v(12)),
            ],
            br(1),
        ),
        blk(6, vec![], Terminator::Ret(Some(v(2)))),
    ];
    Function {
        name: "f".into(),
        params: vec![ValueId(0), ValueId(1)],
        ret: Type::I64,
        blocks,
        values: (0..13)
            .map(|_| ValueInfo {
                ty: Type::I64,
                name: None,
            })
            .collect(),
        current_origin: None,
    }
}

#[test]
fn a_shared_inner_test_is_made_once() {
    let f = survivors();
    let (g, n) = jam(&f);
    assert_eq!(n, 1);
    for (a, b) in [(0, 5), (1, 1), (3, 7), (4, 2), (5, 10), (9, 41), (13, 3)] {
        assert_eq!(
            run(&f, &[a, b], 1 << 22),
            run(&g, &[a, b], 1 << 24),
            "{a} {b}"
        );
    }
    // the original header, the fused one, and once per lane finishing
    // alone
    let m = Operand::Value(ValueId(1));
    let tests = g
        .blocks
        .iter()
        .flat_map(|b| &b.insts)
        .filter(|i| matches!(i.kind, InstKind::ICmp(IPred::Sle, _, b) if b == m))
        .count();
    assert_eq!(tests, 2 + LANES);
}

/// `divisor_sums` with `s = i + d` read between two defs of `d` in
/// `pre`: `d = 1; s = i + d; d = 2; while (d * d <= i) ...`.
fn divisor_redefined() -> Function {
    let mut f = divisor_sums();
    f.blocks[2].insts = vec![
        copy(5, c(1)),
        inst(15, InstKind::BinOp(BinOp::Add, v(2), v(5))),
        copy(4, v(15)),
        copy(5, c(2)),
    ];
    f.values.push(ValueInfo {
        ty: Type::I64,
        name: None,
    });
    f
}

#[test]
fn a_cell_defined_twice_before_the_inner_loop_is_not_shared() {
    let f = divisor_redefined();
    let (g, n) = jam(&f);
    assert_eq!(n, 1);
    for bound in [0, 1, 2, 3, 4, 5, 8, 9, 16, 17, 100, 257] {
        assert_eq!(
            run(&f, &[bound], 1 << 22),
            run(&g, &[bound], 1 << 24),
            "n {bound}"
        );
    }
}
