//! Escape-time inner loops (Mandelbrot): `for (j = 0; j < n; j++) {
//! ci = j * 0.0625 - 1; zr = zi = 0; k = 0; while (k < max) { if
//! (zr² + zi² > lim) { r = k; break } (zr, zi) = (zr² - zi² + cr,
//! 2·zr·zi + ci); k++ } else r = max; total = total * 0.5 + r }`, with
//! the exits' code reading different cells.

use super::interp::run;
use super::*;
use torajs_core::ssa::{FPred, VecInst};

/// What the exits' code reads.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum Reads {
    /// the body exit the count, the header exit `max`
    Count,
    /// the header exit `zr` too
    HeadZr,
    /// both exits `zr` and `zi`
    Point,
    /// as `Count`, but the step bumps `k` ahead of its body exit
    BumpFirst,
    /// the header exit the count, the body exit a constant, and the
    /// step bumps `k` ahead of its body exit: `k` is kept, yet a lane
    /// leaving by the body exit has bumped it
    CountAtHead,
    /// as `Count`, but the sum `k + 1` is computed ahead of the body
    /// exit and written after it
    SumFirst,
}

fn f(x: f64) -> Operand {
    Operand::ConstF64(x)
}
fn fbin(r: u32, op: BinOp, a: Operand, b: Operand) -> Inst {
    inst(r, InstKind::BinOp(op, a, b))
}
fn fcopy(r: u32, src: Operand) -> Inst {
    inst(r, InstKind::Copy(Type::F64, src))
}

/// params `%0 n, %1 max, %2 cr, %3 lim`; `%4 total, %5 j, %9 ci, %10
/// zr, %11 zi, %12 k, %24 r`.
///
///   b0: total=0; j=0 → b1     b4: zr²+zi² > lim ? b7 : b5
///   b1: j < n ? b2 : b9       b5: step; k++ → b3
///   b2: ci; zr=zi=0; k=0      b6: r = max (+ zr) → b8
///   b3: k < max ? b4 : b6     b7: r = k (+ zr + zi) → b8
///   b8: total = total*0.5 + r; j++ → b1      b9: ret total
pub(super) fn escape(reads: Reads) -> Function {
    let bump = vec![
        inst(23, InstKind::BinOp(BinOp::Add, v(12), c(1))),
        copy(12, v(23)),
    ];
    let mut test = vec![
        fbin(14, BinOp::FMul, v(10), v(10)),
        fbin(15, BinOp::FMul, v(11), v(11)),
        fbin(16, BinOp::FAdd, v(14), v(15)),
        inst(17, InstKind::FCmp(FPred::Ogt, v(16), v(3))),
    ];
    let mut step = vec![
        fbin(18, BinOp::FSub, v(14), v(15)),
        fbin(19, BinOp::FAdd, v(18), v(2)),
        fbin(20, BinOp::FAdd, v(10), v(10)),
        fbin(21, BinOp::FMul, v(20), v(11)),
        fbin(22, BinOp::FAdd, v(21), v(9)),
        fcopy(10, v(19)),
        fcopy(11, v(22)),
    ];
    match reads {
        Reads::BumpFirst | Reads::CountAtHead => drop(test.splice(0..0, bump)),
        Reads::SumFirst => {
            let [sum, write] = <[Inst; 2]>::try_from(bump).expect("two");
            test.insert(0, sum);
            step.push(write);
        }
        _ => step.extend(bump),
    }
    let head_reads = if reads == Reads::CountAtHead { 12 } else { 1 };
    let mut head_exit = vec![inst(28, InstKind::SiToFp(v(head_reads)))];
    let mut body_exit = vec![inst(29, InstKind::SiToFp(v(12)))];
    if reads == Reads::CountAtHead {
        body_exit = vec![fcopy(29, f(-1.0))];
    }
    let (mut head_r, mut body_r) = (28, 29);
    if matches!(reads, Reads::HeadZr | Reads::Point) {
        head_exit.push(fbin(30, BinOp::FAdd, v(28), v(10)));
        head_r = 30;
    }
    if reads == Reads::Point {
        head_exit.push(fbin(31, BinOp::FAdd, v(30), v(11)));
        body_exit.push(fbin(32, BinOp::FAdd, v(29), v(10)));
        body_exit.push(fbin(33, BinOp::FAdd, v(32), v(11)));
        (head_r, body_r) = (31, 33);
    }
    head_exit.push(fcopy(24, v(head_r)));
    body_exit.push(fcopy(24, v(body_r)));
    let blocks = vec![
        blk(0, vec![fcopy(4, f(0.0)), copy(5, c(0))], br(1)),
        blk(
            1,
            vec![inst(6, InstKind::ICmp(IPred::Slt, v(5), v(0)))],
            cbr(6, 2, 9),
        ),
        blk(
            2,
            vec![
                inst(7, InstKind::SiToFp(v(5))),
                fbin(8, BinOp::FMul, v(7), f(0.0625)),
                fbin(9, BinOp::FSub, v(8), f(1.0)),
                fcopy(10, f(0.0)),
                fcopy(11, f(0.0)),
                copy(12, c(0)),
            ],
            br(3),
        ),
        blk(
            3,
            vec![inst(13, InstKind::ICmp(IPred::Slt, v(12), v(1)))],
            cbr(13, 4, 6),
        ),
        blk(4, test, cbr(17, 7, 5)),
        blk(5, step, br(3)),
        blk(6, head_exit, br(8)),
        blk(7, body_exit, br(8)),
        blk(
            8,
            vec![
                fbin(25, BinOp::FMul, v(4), f(0.5)),
                fbin(26, BinOp::FAdd, v(25), v(24)),
                inst(27, InstKind::BinOp(BinOp::Add, v(5), c(1))),
                fcopy(4, v(26)),
                copy(5, v(27)),
            ],
            br(1),
        ),
        blk(9, vec![], Terminator::Ret(Some(v(4)))),
    ];
    let ints = [0, 1, 5, 12, 23, 27];
    let bools = [6, 13, 17];
    Function {
        name: "f".into(),
        params: vec![ValueId(0), ValueId(1), ValueId(2), ValueId(3)],
        ret: Type::F64,
        blocks,
        values: (0..34)
            .map(|i| ValueInfo {
                ty: if ints.contains(&i) {
                    Type::I64
                } else if bools.contains(&i) {
                    Type::Bool
                } else {
                    Type::F64
                },
                name: None,
            })
            .collect(),
        current_origin: None,
    }
}

fn plan_of(reads: Reads) -> Plan {
    let func = escape(reads);
    plan::find_plan(&func, &Default::default()).expect("the j loop is planned")
}

fn vec_selects(func: &Function) -> usize {
    let insts = func.blocks.iter().flat_map(|b| b.insts.iter());
    insts
        .filter(|i| matches!(i.kind, InstKind::Vec(VecInst::Select(..))))
        .count()
}

fn packed(func: &Function) -> bool {
    let insts = func.blocks.iter().flat_map(|b| b.insts.iter());
    insts
        .filter(|i| matches!(i.kind, InstKind::Vec(VecInst::BinOp(..))))
        .count()
        > 0
}

#[test]
fn a_lane_keeps_only_the_cells_its_exits_read() {
    let ids = |p: Plan| {
        p.exit_cells
            .map(|cs| cs.iter().map(|c| c.0).collect::<Vec<_>>())
    };
    assert_eq!(ids(plan_of(Reads::Count)), Some(vec![12]));
    assert_eq!(ids(plan_of(Reads::HeadZr)), Some(vec![10, 12]));
    // every written cell is read: the plain form
    assert_eq!(ids(plan_of(Reads::Point)), None);
    // `k` at the exit is not `k` at the step's start
    assert_eq!(ids(plan_of(Reads::BumpFirst)), None);
}

/// Two lanes to a vector, beside three splats (cr, lim, max) and three
/// temporaries (zr², zi² and their sum): 15 registers for pairs, 14
/// with the `+ 1` splat.
#[test]
fn fewer_snapshots_make_room_for_more_lanes() {
    let pk = |reads: Reads| {
        let func = escape(reads);
        let plan = plan::find_plan(&func, &Default::default()).expect("planned");
        let pk = masked_vec::packing(&func, &plan).expect("packed");
        (pk.per_pair, pk.splats, pk.temps, pk.frozen.len())
    };
    // the `+ 1` is a splat where the count is not frozen
    assert_eq!(pk(Reads::Point), (8, 4, 3, 0));
    assert_eq!(pk(Reads::Count), (5, 3, 3, 1));
    assert_eq!(pk(Reads::HeadZr), (6, 3, 3, 1));
    // ci, zr, zi, k, the flag and three snapshots: 8 a pair, one pair
    assert_eq!(plan_of(Reads::Point).lanes, 2);
    // the count freezes itself: 5 a pair, three pairs
    assert_eq!(plan_of(Reads::Count).lanes, 6);
    // zr's snapshot too: 6 a pair, two pairs
    assert_eq!(plan_of(Reads::HeadZr).lanes, 4);
    // every cell snapshotted: 8 a pair
    assert_eq!(plan_of(Reads::BumpFirst).lanes, 2);
}

#[test]
fn a_lane_keeping_only_its_count_carries_no_snapshot() {
    let (count, n) = jam(&escape(Reads::Count));
    assert_eq!(n, 1);
    assert!(packed(&count));
    assert_eq!(vec_selects(&count), 0);
    let (point, _) = jam(&escape(Reads::Point));
    assert!(packed(&point));
    assert_eq!(vec_selects(&point), 3);
}

#[test]
fn escape_time_lanes_retire_through_the_exit_they_took() {
    let fl = |x: f64| x.to_bits() as i64;
    for reads in [
        Reads::Count,
        Reads::HeadZr,
        Reads::Point,
        Reads::BumpFirst,
        Reads::CountAtHead,
        Reads::SumFirst,
    ] {
        let func = escape(reads);
        let (g, n) = jam(&func);
        assert_eq!(n, 1, "{reads:?}");
        for bound in [0, 1, 3, 4, 5, 7, 9, 12] {
            for max in [0, 1, 2, 5, 20, 60] {
                for cr in [-2.0, -1.0, -0.75, 0.0, 0.26, 0.3, 2.0, f64::NAN] {
                    for lim in [4.0, 0.0, 1.5, f64::NAN] {
                        let args = [bound, max, fl(cr), fl(lim)];
                        assert_eq!(
                            run(&func, &args, 1 << 22),
                            run(&g, &args, 1 << 24),
                            "{reads:?} args {bound} {max} {cr} {lim}"
                        );
                    }
                }
            }
        }
    }
}
