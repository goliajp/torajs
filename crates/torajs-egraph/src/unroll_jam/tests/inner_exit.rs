//! Inner loops left early by a guard that stays in the outer loop, the
//! shape an inlined `while (k < m) { if (x > lim) return k; ... }`
//! leaves: `for (i = 1; i <= n; i++) { x = i; k = 0; r = m;
//! while (k < m) { if (x > lim) { r = k; break } x = 3x + i; k++ }
//! total = total * 7 + r }`.

use super::interp::run;
use super::*;

/// params `%0 n, %21 m, %22 lim`; `%1 total, %2 i, %4 x, %5 k, %15 r`.
/// With `write_first` the step bumps `k` ahead of its exit test; with
/// `divide` it takes `x % 1000003` after the multiply-add.
///
///   b0: total=0; i=1         b4: x > lim ? b8 : b5
///   b1: i <= n ? b2 : b7     b5: x = 3x + i; k++ → b3
///   b2: x=i; k=0 → b3        b6: total = total*7 + r; i++ → b1
///   b3: k < m ? b4 : b9      b7: ret total
///   b8: r = k → b6           b9: r = m → b6
fn early_exit(write_first: bool, divide: bool) -> Function {
    let bump = [
        inst(14, InstKind::BinOp(BinOp::Add, v(5), c(1))),
        copy(5, v(14)),
    ];
    let mut test = vec![inst(7, InstKind::ICmp(IPred::Sgt, v(4), v(22)))];
    let mut step = vec![
        inst(11, InstKind::BinOp(BinOp::Mul, v(4), c(3))),
        inst(12, InstKind::BinOp(BinOp::Add, v(11), v(2))),
    ];
    if divide {
        step.push(inst(13, InstKind::BinOp(BinOp::SRem, v(12), c(1_000_003))));
        step.push(copy(4, v(13)));
    } else {
        step.push(copy(4, v(12)));
    }
    if write_first {
        test.splice(0..0, bump);
    } else {
        step.extend(bump);
    }
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
            vec![inst(6, InstKind::ICmp(IPred::Slt, v(5), v(21)))],
            cbr(6, 4, 9),
        ),
        blk(4, test, cbr(7, 8, 5)),
        blk(5, step, br(3)),
        blk(
            6,
            vec![
                inst(16, InstKind::BinOp(BinOp::Mul, v(1), c(7))),
                inst(17, InstKind::BinOp(BinOp::Add, v(16), v(15))),
                inst(18, InstKind::BinOp(BinOp::Add, v(2), c(1))),
                copy(1, v(17)),
                copy(2, v(18)),
            ],
            br(1),
        ),
        blk(7, vec![], Terminator::Ret(Some(v(1)))),
        blk(8, vec![copy(15, v(5))], br(6)),
        blk(9, vec![copy(15, v(21))], br(6)),
    ];
    let bools = [3, 6, 7];
    Function {
        name: "f".into(),
        params: vec![ValueId(0), ValueId(21), ValueId(22)],
        ret: Type::I64,
        blocks,
        values: (0..23)
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

fn same_results(f: &Function) {
    let (g, n) = jam(f);
    assert_eq!(n, 1);
    // lanes leave by the exit at different steps (x grows with i), by
    // the step limit, or one by each; a group's first lane may finish
    // before or after the others
    for bound in [-1, 0, 1, 2, 3, 4, 5, 6, 7, 9, 13, 40] {
        for m in [0, 1, 2, 3, 5, 8, 30] {
            for lim in [i64::MIN, 0, 1, 5, 20, 100, 1000, 1 << 40] {
                let args = [bound, m, lim];
                assert_eq!(
                    run(f, &args, 1 << 22),
                    run(&g, &args, 1 << 24),
                    "args {args:?}"
                );
            }
        }
    }
}

#[test]
fn lanes_leaving_the_inner_loop_early_finish_in_order() {
    same_results(&early_exit(false, false));
}

#[test]
fn a_cell_written_ahead_of_the_exit_is_kept_from_the_step_start() {
    same_results(&early_exit(true, false));
}

#[test]
fn an_early_exit_around_an_integer_divide_keeps_the_loop_alone() {
    assert_eq!(jam(&early_exit(false, true)).1, 0);
}
