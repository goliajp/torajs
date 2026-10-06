//! Inner loops without a divide, taken for the latency of what one
//! step carries to the next: Collatz steps of `for (i = 1; i <= n;
//! i++) { x = i; k = 0; while (x != 1) { if (x > lim) return ..;
//! x = x & 1 ? 3x + 1 : x >> 1; k++ } total = total * 7 + k }`.

use super::interp::run;
use super::*;

/// params `%0 n, %21 lim`; `%1 total, %2 i, %4 x, %5 k`. With `halve_only`
/// the step is `x = x >> 1` alone: one cycle a step, nothing to hide.
///
///   b0: total=0; i=1       b4: x > lim ? b8 : b5
///   b1: i <= n ? b2 : b7   b5: x = step(x); k++ → b3
///   b2: x=i; k=0 → b3      b6: total = total*7 + k; i++ → b1
///   b3: x != 1 ? b4 : b6   b7: ret total   b8: ret x*1000 + i + total
fn collatz(halve_only: bool) -> Function {
    let step = if halve_only {
        vec![inst(13, InstKind::BinOp(BinOp::AShr, v(4), c(1)))]
    } else {
        vec![
            inst(8, InstKind::BinOp(BinOp::And, v(4), c(1))),
            inst(9, InstKind::ICmp(IPred::Eq, v(8), c(0))),
            inst(10, InstKind::BinOp(BinOp::AShr, v(4), c(1))),
            inst(11, InstKind::BinOp(BinOp::Mul, v(4), c(3))),
            inst(12, InstKind::BinOp(BinOp::Add, v(11), c(1))),
            inst(13, InstKind::Select(Type::I64, v(9), v(10), v(12))),
        ]
    };
    let mut body = step;
    body.extend([
        inst(14, InstKind::BinOp(BinOp::Add, v(5), c(1))),
        copy(4, v(13)),
        copy(5, v(14)),
    ]);
    let blocks = vec![
        blk(0, vec![copy(1, c(0)), copy(2, c(1))], br(1)),
        blk(
            1,
            vec![inst(3, InstKind::ICmp(IPred::Sle, v(2), v(0)))],
            cbr(3, 2, 7),
        ),
        blk(2, vec![copy(4, v(2)), copy(5, c(0))], br(3)),
        blk(
            3,
            vec![inst(6, InstKind::ICmp(IPred::Ne, v(4), c(1)))],
            cbr(6, 4, 6),
        ),
        blk(
            4,
            vec![inst(7, InstKind::ICmp(IPred::Sgt, v(4), v(21)))],
            Terminator::CondBr {
                cond: v(7),
                then_blk: BlockId(8),
                else_blk: BlockId(5),
            },
        ),
        blk(5, body, br(3)),
        blk(
            6,
            vec![
                inst(15, InstKind::BinOp(BinOp::Mul, v(1), c(7))),
                inst(16, InstKind::BinOp(BinOp::Add, v(15), v(5))),
                inst(17, InstKind::BinOp(BinOp::Add, v(2), c(1))),
                copy(1, v(16)),
                copy(2, v(17)),
            ],
            br(1),
        ),
        blk(7, vec![], Terminator::Ret(Some(v(1)))),
        blk(
            8,
            vec![
                inst(18, InstKind::BinOp(BinOp::Mul, v(4), c(1000))),
                inst(19, InstKind::BinOp(BinOp::Add, v(18), v(2))),
                inst(20, InstKind::BinOp(BinOp::Add, v(19), v(1))),
            ],
            Terminator::Ret(Some(v(20))),
        ),
    ];
    let bools = [3, 6, 7, 9];
    Function {
        name: "f".into(),
        params: vec![ValueId(0), ValueId(21)],
        ret: Type::I64,
        blocks,
        values: (0..22)
            .map(|i| ValueInfo {
                ty: if bools.contains(&i) {
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

#[test]
fn a_multiply_add_recurrence_is_jammed_and_keeps_every_exit() {
    let f = collatz(false);
    let (g, n) = jam(&f);
    assert_eq!(n, 1);
    // `lim` makes some starts leave from the inner guard: in the fused
    // loop (lane 0 or a later lane), or in a lane finishing alone
    for bound in [-1, 0, 1, 2, 3, 4, 5, 6, 7, 9, 27, 40, 97] {
        for lim in [i64::MAX, 1, 8, 16, 40, 52, 100, 160, 9232] {
            let args = [bound, lim];
            assert_eq!(
                run(&f, &args, 1 << 22),
                run(&g, &args, 1 << 24),
                "args {args:?}"
            );
        }
    }
}

#[test]
fn a_one_cycle_recurrence_is_left_alone() {
    assert_eq!(jam(&collatz(true)).1, 0);
}
