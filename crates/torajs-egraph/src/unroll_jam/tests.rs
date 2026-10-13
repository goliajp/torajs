mod carried;
mod escape;
mod inner_exit;
mod interp;
mod packed;
mod rising;
mod shared;

use super::*;
use interp::run;
use torajs_core::ssa::{
    BinOp, Block, FuncId, Function, Inst, InstKind, Terminator, Type, ValueInfo,
};

fn v(id: u32) -> Operand {
    Operand::Value(ValueId(id))
}
fn c(x: i64) -> Operand {
    Operand::ConstI64(x)
}
fn inst(r: u32, kind: InstKind) -> Inst {
    Inst {
        result: Some(ValueId(r)),
        kind,
        origin: None,
    }
}
fn copy(r: u32, src: Operand) -> Inst {
    inst(r, InstKind::Copy(Type::I64, src))
}
fn blk(id: u32, insts: Vec<Inst>, term: Terminator) -> Block {
    Block {
        id: BlockId(id),
        insts,
        term,
    }
}
fn br(t: u32) -> Terminator {
    Terminator::Br(BlockId(t))
}
fn cbr(cond: u32, t: u32, e: u32) -> Terminator {
    Terminator::CondBr {
        cond: v(cond),
        then_blk: BlockId(t),
        else_blk: BlockId(e),
    }
}

/// Where `tmp`, a cell only the inner body sets, is read.
#[derive(Clone, Copy, PartialEq)]
enum Temp {
    /// only inside the inner body (and at an inner guard's exit)
    Inside,
    /// also in `post`
    Post,
    /// also after the loop ends normally
    Exit,
}

#[derive(Clone, Copy)]
struct Shape {
    step: i64,
    /// header exits when `i > n` is true instead of looping on `i <= n`
    inverted: bool,
    const_bound: Option<i64>,
    /// pre reads the running total (a carried value)
    carried: bool,
    /// inner step without a divide
    no_divide: bool,
    /// a call inside the inner loop
    call: bool,
    /// the bail reads the new total, a value `post` computes
    escape: bool,
    /// `b = i - 1; while (b != 0) b = b % 7 + 7`: done at once for
    /// `i == 1`, never for any later `i`
    spin: bool,
    /// the header computes the bound itself: `nb = n - 1; i <= nb`
    bound_in_header: bool,
    /// pre leaves the loop when `i > lim` (an overflow-style bail)
    pre_guard: bool,
    /// the inner body leaves the loop when `a % b > lim` (`== lim` with
    /// a temporary), reading the remainder after the loop (exclusive
    /// with `pre_guard`)
    inner_guard: bool,
    /// the inner body divides through `tmp = copy b`, a cell only it
    /// sets; an inner guard's exit then reads `tmp`
    temp: Option<Temp>,
    /// `a = t, b = i` instead of `a = i, b = t`, and `post` never bails
    swap: bool,
}

const PLAIN: Shape = Shape {
    step: 1,
    inverted: false,
    const_bound: None,
    carried: false,
    no_divide: false,
    call: false,
    escape: false,
    spin: false,
    bound_in_header: false,
    pre_guard: false,
    inner_guard: false,
    temp: None,
    swap: false,
};

/// `total = 0; for (i = 1; i <= n; i += step) { a = i; b = t; while (b != 0)
/// { r = a % b; a = b; b = r } total += a; if (total > lim) return
/// total' * 1000003 + i * 1009 + a } return total` — params `n, t, lim`,
/// the bail reading the pre-add total.
fn gcd_loop(s: Shape) -> Function {
    let bound = if s.bound_in_header {
        v(22)
    } else {
        s.const_bound.map_or(v(0), c)
    };
    let test = if s.inverted {
        inst(7, InstKind::ICmp(IPred::Sgt, v(4), bound))
    } else {
        inst(7, InstKind::ICmp(IPred::Sle, v(4), bound))
    };
    let head_term = if s.inverted {
        cbr(7, 7, 2)
    } else {
        cbr(7, 2, 7)
    };
    let b_init = if s.carried { v(3) } else { v(1) };
    let step_op = if s.no_divide {
        InstKind::BinOp(BinOp::Sub, v(8), c(1))
    } else {
        InstKind::BinOp(BinOp::SRem, v(8), v(9))
    };
    let mut inner_body = vec![inst(11, step_op), copy(8, v(9)), copy(9, v(11))];
    if s.temp.is_some() {
        inner_body = vec![
            copy(27, v(9)),
            inst(11, InstKind::BinOp(BinOp::SRem, v(8), v(27))),
            copy(8, v(27)),
            copy(9, v(11)),
        ];
    }
    // an inner guard tests the remainder right after it is computed;
    // with a temporary it fires on `r == lim`, a remainder that often
    // shows up only after another lane of the group is already done
    let guard_pred = if s.temp.is_some() {
        IPred::Eq
    } else {
        IPred::Sgt
    };
    let split = inner_body
        .iter()
        .position(|i| i.result == Some(ValueId(11)))
        .expect("the step defines %11");
    let mut pre = if s.swap {
        vec![copy(8, v(1)), copy(9, v(6))]
    } else {
        vec![copy(8, v(6)), copy(9, b_init)]
    };
    if s.spin {
        pre = vec![
            copy(8, v(6)),
            inst(20, InstKind::BinOp(BinOp::Sub, v(4), c(1))),
            copy(9, v(20)),
        ];
        inner_body = vec![
            inst(11, InstKind::BinOp(BinOp::SRem, v(9), c(7))),
            inst(21, InstKind::BinOp(BinOp::Add, v(11), c(7))),
            copy(9, v(21)),
        ];
    }
    if s.call {
        inner_body.insert(0, inst(19, InstKind::Call(FuncId(0), vec![])));
    }
    let mut blocks = vec![
        blk(0, vec![copy(3, c(0)), copy(4, c(1))], br(1)),
        blk(
            1,
            if s.bound_in_header {
                vec![
                    copy(6, v(4)),
                    inst(22, InstKind::BinOp(BinOp::Sub, v(0), c(1))),
                    test,
                ]
            } else {
                vec![copy(6, v(4)), test]
            },
            head_term,
        ),
        if s.pre_guard {
            blk(
                2,
                vec![
                    pre[0].clone(),
                    inst(23, InstKind::ICmp(IPred::Sgt, v(4), v(2))),
                ],
                cbr(23, 9, 10),
            )
        } else {
            blk(2, pre.clone(), br(3))
        },
        blk(
            3,
            vec![inst(10, InstKind::ICmp(IPred::Ne, v(9), c(0)))],
            cbr(10, 4, 5),
        ),
        if s.inner_guard {
            blk(
                4,
                inner_body[..=split]
                    .iter()
                    .cloned()
                    .chain([inst(25, InstKind::ICmp(guard_pred, v(11), v(2)))])
                    .collect(),
                cbr(25, 9, 10),
            )
        } else {
            blk(4, inner_body.clone(), br(3))
        },
        blk(
            5,
            vec![
                inst(
                    12,
                    InstKind::BinOp(
                        BinOp::Add,
                        v(3),
                        v(if s.temp == Some(Temp::Post) { 27 } else { 8 }),
                    ),
                ),
                inst(
                    13,
                    InstKind::ICmp(IPred::Sgt, v(12), if s.swap { c(i64::MAX) } else { v(2) }),
                ),
            ],
            cbr(13, 6, 8),
        ),
        blk(
            6,
            vec![
                inst(
                    14,
                    InstKind::BinOp(BinOp::Mul, v(if s.escape { 12 } else { 3 }), c(1_000_003)),
                ),
                inst(15, InstKind::BinOp(BinOp::Mul, v(4), c(1009))),
                inst(16, InstKind::BinOp(BinOp::Add, v(14), v(15))),
                inst(17, InstKind::BinOp(BinOp::Add, v(16), v(8))),
            ],
            Terminator::Ret(Some(v(17))),
        ),
        blk(
            7,
            vec![],
            Terminator::Ret(Some(v(if s.temp == Some(Temp::Exit) { 27 } else { 3 }))),
        ),
        blk(
            8,
            vec![
                inst(18, InstKind::BinOp(BinOp::Add, v(4), c(s.step))),
                copy(3, v(12)),
                copy(4, v(18)),
            ],
            br(1),
        ),
    ];
    if s.pre_guard {
        blocks.push(blk(
            9,
            vec![inst(24, InstKind::BinOp(BinOp::Mul, v(4), c(7)))],
            Terminator::Ret(Some(v(24))),
        ));
        blocks.push(blk(10, pre[1..].to_vec(), br(3)));
    }
    if s.inner_guard {
        blocks.push(blk(
            9,
            vec![inst(
                26,
                InstKind::BinOp(BinOp::Mul, v(if s.temp.is_some() { 27 } else { 11 }), c(13)),
            )],
            Terminator::Ret(Some(v(26))),
        ));
        blocks.push(blk(10, inner_body[split + 1..].to_vec(), br(3)));
    }
    let ty = |t| ValueInfo { ty: t, name: None };
    let mut values = vec![ty(Type::I64); 28];
    values[25] = ty(Type::Bool);
    values[23] = ty(Type::Bool);
    values[7] = ty(Type::Bool);
    values[10] = ty(Type::Bool);
    values[13] = ty(Type::Bool);
    Function {
        name: "f".into(),
        params: vec![ValueId(0), ValueId(1), ValueId(2)],
        ret: Type::I64,
        blocks,
        values,
        current_origin: None,
    }
}

fn jam(f: &Function) -> (Function, u32) {
    let mut m = Module {
        funcs: vec![f.clone()],
        ..Default::default()
    };
    let n = jam_module(&mut m).loops_jammed;
    (m.funcs.pop().expect("one fn"), n)
}

fn same_results(s: Shape) {
    let f = gcd_loop(s);
    let (g, n) = jam(&f);
    assert_eq!(n, 1, "the counted gcd loop is jammed");
    for bound in [-3, 0, 1, 2, 3, 4, 5, 7, 8, 9, 13, 20] {
        for t in [0, 1, 6, 7, 12, 30, 97] {
            for lim in [i64::MAX, 0, 3, 10, 25, 60] {
                let args = [bound, t, lim];
                assert_eq!(
                    run(&f, &args, 1 << 20),
                    run(&g, &args, 1 << 22),
                    "args {args:?}"
                );
            }
        }
    }
}

#[test]
fn jammed_loop_gives_the_same_result_and_bail_state() {
    same_results(PLAIN);
}

#[test]
fn step_and_inverted_test_and_constant_bound() {
    same_results(Shape { step: 3, ..PLAIN });
    same_results(Shape {
        escape: true,
        ..PLAIN
    });
    same_results(Shape {
        pre_guard: true,
        ..PLAIN
    });
    same_results(Shape {
        inner_guard: true,
        ..PLAIN
    });
    same_results(Shape {
        inverted: true,
        ..PLAIN
    });
    for k in [0, 2, 3, 4, 11] {
        same_results(Shape {
            const_bound: Some(k),
            ..PLAIN
        });
    }
}

#[test]
fn bound_near_i64_min_never_starts_a_group() {
    let f = gcd_loop(PLAIN);
    let (g, _) = jam(&f);
    let args = [i64::MIN + 1, 5, i64::MAX];
    assert_eq!(run(&g, &args, 1 << 20), Some(0));
    assert_eq!(
        jam(&gcd_loop(Shape {
            const_bound: Some(i64::MIN + 1),
            ..PLAIN
        }))
        .1,
        0
    );
}

#[test]
fn carried_state_divide_free_or_calling_inner_loops_are_left_alone() {
    for s in [
        Shape {
            carried: true,
            ..PLAIN
        },
        Shape {
            no_divide: true,
            ..PLAIN
        },
        Shape {
            call: true,
            ..PLAIN
        },
        Shape {
            bound_in_header: true,
            ..PLAIN
        },
    ] {
        assert_eq!(jam(&gcd_loop(s)).1, 0);
    }
}

#[test]
fn a_lane_that_never_finishes_behind_a_lane_that_leaves_the_loop() {
    let f = gcd_loop(Shape {
        spin: true,
        ..PLAIN
    });
    let (g, n) = jam(&f);
    assert_eq!(n, 1);
    // lane i = 1 leaves at once (total 1 > 0); i = 2.. would spin
    let args = [10, 0, 0];
    assert_eq!(run(&f, &args, 1 << 16), Some(1009 + 1));
    assert_eq!(run(&g, &args, 1 << 16), run(&f, &args, 1 << 16));
    // with no early exit both spin at i = 2
    let args = [10, 0, i64::MAX];
    assert_eq!(run(&f, &args, 1 << 16), None);
    assert_eq!(run(&g, &args, 1 << 16), None);
}

#[test]
fn a_temporary_the_inner_body_sets_stays_inside_it() {
    same_results(Shape {
        temp: Some(Temp::Inside),
        ..PLAIN
    });
    // a lane finishing on its own takes the guard exit, which reads its
    // temporary under the temporary's own id
    same_results(Shape {
        temp: Some(Temp::Inside),
        inner_guard: true,
        ..PLAIN
    });
    // gcd(t, i) with t = 8: lane i = 1 is done after one step, lane i = 3
    // reaches remainder 1 at its second, on its own
    let f = gcd_loop(Shape {
        temp: Some(Temp::Inside),
        inner_guard: true,
        swap: true,
        ..PLAIN
    });
    let (g, n) = jam(&f);
    assert_eq!(n, 1);
    for bound in 4..=12 {
        for t in [2, 8, 14, 20, 26] {
            let args = [bound, t, 1];
            assert_eq!(
                run(&f, &args, 1 << 20),
                run(&g, &args, 1 << 22),
                "args {args:?}"
            );
        }
    }
    // after the loop `tmp` is whatever the last step left; a lane that
    // finished in the fused loop never writes its copy back
    for t in [Temp::Post, Temp::Exit] {
        assert_eq!(
            jam(&gcd_loop(Shape {
                temp: Some(t),
                ..PLAIN
            }))
            .1,
            0
        );
    }
}
