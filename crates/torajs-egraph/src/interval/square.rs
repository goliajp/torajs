//! A square compared against a bound, `x * x <= y` (or `<`, `==`),
//! bounds `x` itself: `|x| <= isqrt(y)`. Trial-division loops test
//! `d * d <= n`; the counter's own range is what proves its
//! conversions exact and the sums of its values small.
//!
//! The i64 `mul` wraps, and a wrapped product passes the test with
//! `|x|` anywhere, so the bound applies only while `x`'s own fact
//! keeps `x * x` inside i64. A counter widened past that point would
//! never be refined again, so the root of each square's bound joins
//! the widening thresholds: the counter first stops there, where the
//! refined fixpoint holds.
//!
//! Inside a function the test is usually made in f64, `sitofp (d * d)
//! <= n`: a bound below 2^53 then says the same (a product at or past
//! 2^53 converts to at least 2^53). The square may also reach the
//! compare through copies and from a block the branching block is
//! entered from alone, as long as `x` is not redefined on the way (a
//! window check on the product sits between them in a loop copy that
//! int_entry made).

use super::SingleDefs;

use torajs_core::ssa::{BinOp, Function, InstKind, Operand, ValueId};

use super::edge::{edge_rel, int_pred};
use super::refine::{Constraint, Rel};
use super::transfer::{AbsVal, NumFact, POS_INF, op_const_int};

/// `floor(sqrt(i64::MAX))`: for `|x|` up to this, `x * x` is exact.
const ROOT_MAX: i128 = 3_037_000_499;

/// What the square is compared against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SquareBound {
    Value(ValueId),
    Const(i128),
}

/// `floor(sqrt(n))` for `n >= 0`.
fn isqrt(n: i128) -> i128 {
    let mut r = (n as f64).sqrt() as i128;
    while r * r > n {
        r -= 1;
    }
    while (r + 1) * (r + 1) <= n {
        r += 1;
    }
    r
}

/// What the compare closing block `bi` says, on its `taken` edge, of
/// the root of a square it compares.
pub(super) fn square_edge(
    func: &Function,
    bi: usize,
    kind: &InstKind,
    taken: bool,
    defs: &SingleDefs<'_>,
    sole_pred: &[Option<usize>],
) -> Option<Constraint> {
    let (pred, a, b, float) = match kind {
        InstKind::ICmp(p, a, b) => (*p, a, b, false),
        InstKind::FCmp(p, a, b) => (int_pred(*p)?, a, b, true),
        _ => return None,
    };
    let rel = edge_rel(pred, taken)?;
    [(a, b, true), (b, a, false)]
        .into_iter()
        .find_map(|(sq, other, left)| {
            let Operand::Value(v) = sq else { return None };
            let root = square_root(func, bi, *v, defs, float, sole_pred)?;
            square_constraint(root, rel, left, other, float)
        })
}

/// The root `x` of the `mul x, x` that `v` names through single-def
/// copies (and, for an f64 compare, one `sitofp`), when `x` is not
/// redefined between the product and the end of block `bi`.
fn square_root(
    func: &Function,
    bi: usize,
    mut v: ValueId,
    defs: &SingleDefs<'_>,
    float: bool,
    sole_pred: &[Option<usize>],
) -> Option<ValueId> {
    let mut converted = false;
    for _ in 0..8 {
        match defs.get(&v)? {
            InstKind::Copy(_, Operand::Value(s)) => v = *s,
            InstKind::SiToFp(Operand::Value(s)) if float && !converted => {
                v = *s;
                converted = true;
            }
            InstKind::BinOp(BinOp::Mul, Operand::Value(x), Operand::Value(y))
                if x == y && converted == float =>
            {
                return root_kept(func, bi, v, *x, sole_pred);
            }
            _ => return None,
        }
    }
    None
}

/// `x` when no def of it lies between `m`'s def and the end of block
/// `bi`, walking back through blocks entered from one block only.
fn root_kept(
    func: &Function,
    mut bi: usize,
    m: ValueId,
    x: ValueId,
    sole_pred: &[Option<usize>],
) -> Option<ValueId> {
    for _ in 0..8 {
        let insts = &func.blocks[bi].insts;
        let at = insts.iter().position(|i| i.result == Some(m));
        let after = at.map_or(0, |p| p + 1);
        if insts[after..].iter().any(|i| i.result == Some(x)) {
            return None;
        }
        if at.is_some() {
            return Some(x);
        }
        bi = sole_pred[bi]?;
    }
    None
}

/// The constraint on `root` when its square is the left (`left`) or
/// right operand of a compare whose edge says `rel` of the left one.
pub(super) fn square_constraint(
    root: ValueId,
    rel: Rel,
    left: bool,
    other: &Operand,
    via_sitofp: bool,
) -> Option<Constraint> {
    let rel = if left {
        rel
    } else {
        match rel {
            Rel::Lt => Rel::Gt,
            Rel::Le => Rel::Ge,
            Rel::Gt => Rel::Lt,
            Rel::Ge => Rel::Le,
            r => r,
        }
    };
    let strict = match rel {
        Rel::Lt => true,
        Rel::Le | Rel::Eq => false,
        _ => return None,
    };
    let bound = match other {
        Operand::Value(b) => SquareBound::Value(*b),
        c => SquareBound::Const(op_const_int(c)?),
    };
    Some(Constraint::Square {
        value: root,
        strict,
        bound,
        via_sitofp,
    })
}

/// The range of `x` under `x * x <= hi` (`< hi` when `strict`), given
/// `x`'s fact `f`; `None` unless `f` keeps the product exact.
pub(super) fn root_range(f: NumFact, hi: i128, strict: bool) -> Option<NumFact> {
    if f.lo < -ROOT_MAX || f.hi > ROOT_MAX || hi >= POS_INF {
        return None;
    }
    let hi = hi - i128::from(strict);
    (hi >= 0).then(|| {
        let r = isqrt(hi);
        NumFact::new(-r, r)
    })
}

/// The thresholds a square bound's high end adds: its root, where
/// the counter stops, and one past it, where the latch leaves it.
pub(super) fn root_thresholds(hi: i128, strict: bool) -> Option<[i128; 2]> {
    let hi = hi - i128::from(strict);
    (0..POS_INF).contains(&hi).then(|| {
        let r = isqrt(hi);
        [r, r + 1]
    })
}

/// One round's widening thresholds: the function's own, with the
/// roots the previous round read.
pub(super) fn with_roots(thresholds: &[i128], roots: &[i128]) -> Vec<i128> {
    let mut t: Vec<i128> = thresholds.iter().chain(roots).copied().collect();
    t.sort_unstable();
    t.dedup();
    t
}

/// The high end of a square's bound where the constraint is live: the
/// bound is not redefined since the compare, so what the path says of
/// it holds for the value compared (squares on it left out, so two
/// squares bounding each other cannot recurse). Compared as f64, only
/// a bound below 2^53 counts.
pub(super) fn bound_hi(
    bound: SquareBound,
    via_sitofp: bool,
    state: &super::State,
    live: &[Constraint],
) -> Option<i128> {
    let hi = bound_hi_any(bound, state, live)?;
    (!via_sitofp || hi < 1 << 53).then_some(hi)
}

/// Add to `roots` the thresholds of every square bound live in a
/// block whose bound `state` already caps.
pub(super) fn live_roots(live: &[Constraint], state: &super::State, roots: &mut Vec<i128>) {
    for c in live {
        if let Constraint::Square {
            strict,
            bound,
            via_sitofp,
            ..
        } = c
            && let Some(hi) = bound_hi(*bound, *via_sitofp, state, live)
        {
            roots.extend(root_thresholds(hi, *strict).into_iter().flatten());
        }
    }
}

fn bound_hi_any(bound: SquareBound, state: &super::State, live: &[Constraint]) -> Option<i128> {
    match bound {
        SquareBound::Const(k) => Some(k),
        SquareBound::Value(b) => {
            let plain: Vec<Constraint> = live
                .iter()
                .filter(|c| !matches!(c, Constraint::Square { .. }))
                .copied()
                .collect();
            match super::lookup(&Operand::Value(b), state, &plain) {
                AbsVal::Fact(k) => Some(k.hi),
                _ => None,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{
        Block, BlockId, FPred, Function, IPred, Inst, Terminator, Type, ValueInfo,
    };

    fn inst(r: u32, kind: InstKind) -> Inst {
        Inst {
            result: Some(ValueId(r)),
            kind,
            origin: None,
        }
    }

    fn v(id: u32) -> Operand {
        Operand::Value(ValueId(id))
    }

    /// `n = <bound>; d = 1; while (d * d <= n) d = d + 1; return d`
    /// with `%0 n, %1 d, %2 d*d, %3 test, %4 d+1`; `square_left` false
    /// writes the test as `n >= d * d`.
    fn trial_loop(bound: i64, square_left: bool) -> Function {
        let test = if square_left {
            InstKind::ICmp(IPred::Sle, v(2), v(0))
        } else {
            InstKind::ICmp(IPred::Sge, v(0), v(2))
        };
        let blocks = vec![
            Block {
                id: BlockId(0),
                insts: vec![
                    inst(0, InstKind::Copy(Type::I64, Operand::ConstI64(bound))),
                    inst(1, InstKind::Copy(Type::I64, Operand::ConstI64(1))),
                ],
                term: Terminator::Br(BlockId(1)),
            },
            Block {
                id: BlockId(1),
                insts: vec![
                    inst(2, InstKind::BinOp(BinOp::Mul, v(1), v(1))),
                    inst(3, test),
                ],
                term: Terminator::CondBr {
                    cond: v(3),
                    then_blk: BlockId(2),
                    else_blk: BlockId(3),
                },
            },
            Block {
                id: BlockId(2),
                insts: vec![
                    inst(4, InstKind::BinOp(BinOp::Add, v(1), Operand::ConstI64(1))),
                    inst(1, InstKind::Copy(Type::I64, v(4))),
                ],
                term: Terminator::Br(BlockId(1)),
            },
            Block {
                id: BlockId(3),
                insts: vec![],
                term: Terminator::Ret(Some(v(1))),
            },
        ];
        Function {
            name: "f".into(),
            params: vec![],
            ret: Type::I64,
            blocks,
            values: (0..5)
                .map(|_| ValueInfo {
                    ty: Type::I64,
                    name: None,
                })
                .collect(),
            current_origin: None,
        }
    }

    #[test]
    fn a_square_test_bounds_the_counter() {
        for left in [true, false] {
            let facts = super::super::analyze_function(&trial_loop(2_147_483_647, left));
            assert_eq!(facts[&ValueId(1)], NumFact::new(1, 46_341), "left {left}");
            assert_eq!(facts[&ValueId(4)], NumFact::new(2, 46_341), "left {left}");
        }
    }

    /// The test as the front end writes it, under the guard that keeps
    /// the product from wrapping: `and(icmp ult (d + R), 2R + 1, d * d
    /// <= n)` still bounds the counter.
    #[test]
    fn a_guarded_square_test_bounds_the_counter() {
        const R: i64 = 3_037_000_499;
        let mut f = trial_loop(2_147_483_647, true);
        f.blocks[1].insts.extend([
            inst(5, InstKind::BinOp(BinOp::Add, v(1), Operand::ConstI64(R))),
            inst(
                6,
                InstKind::ICmp(IPred::Ult, v(5), Operand::ConstI64(2 * R + 1)),
            ),
            inst(7, InstKind::BinOp(BinOp::And, v(6), v(3))),
        ]);
        f.blocks[1].term = Terminator::CondBr {
            cond: v(7),
            then_blk: BlockId(2),
            else_blk: BlockId(3),
        };
        for ty in [Type::I64, Type::Bool, Type::Bool] {
            f.values.push(ValueInfo { ty, name: None });
        }
        let facts = super::super::analyze_function(&f);
        assert_eq!(facts[&ValueId(1)], NumFact::new(1, 46_341));
    }

    /// The bound is an outer counter whose own fact is open (a def
    /// after the loop sets it to i64::MAX); only the outer header's
    /// `n <= 1000`, live through the inner loop, bounds it there.
    #[test]
    fn the_bound_is_read_where_the_square_test_holds() {
        let i64c = |k: i64| Operand::ConstI64(k);
        let block = |id: u32, insts: Vec<Inst>, term: Terminator| Block {
            id: BlockId(id),
            insts,
            term,
        };
        let cond = |c: u32, t: u32, e: u32| Terminator::CondBr {
            cond: v(c),
            then_blk: BlockId(t),
            else_blk: BlockId(e),
        };
        let blocks = vec![
            block(
                0,
                vec![inst(5, InstKind::Copy(Type::I64, i64c(1)))],
                Terminator::Br(BlockId(1)),
            ),
            block(
                1,
                vec![inst(6, InstKind::ICmp(IPred::Sle, v(5), i64c(1000)))],
                cond(6, 2, 6),
            ),
            block(
                2,
                vec![inst(1, InstKind::Copy(Type::I64, i64c(1)))],
                Terminator::Br(BlockId(3)),
            ),
            block(
                3,
                vec![
                    inst(2, InstKind::BinOp(BinOp::Mul, v(1), v(1))),
                    inst(3, InstKind::ICmp(IPred::Sle, v(2), v(5))),
                ],
                cond(3, 4, 5),
            ),
            block(
                4,
                vec![
                    inst(4, InstKind::BinOp(BinOp::Add, v(1), i64c(1))),
                    inst(1, InstKind::Copy(Type::I64, v(4))),
                ],
                Terminator::Br(BlockId(3)),
            ),
            block(
                5,
                vec![
                    inst(7, InstKind::BinOp(BinOp::Add, v(5), i64c(1))),
                    inst(5, InstKind::Copy(Type::I64, v(7))),
                ],
                Terminator::Br(BlockId(1)),
            ),
            block(
                6,
                vec![inst(5, InstKind::Copy(Type::I64, i64c(i64::MAX)))],
                Terminator::Ret(Some(v(1))),
            ),
        ];
        let f = Function {
            name: "f".into(),
            params: vec![],
            ret: Type::I64,
            blocks,
            values: (0..8)
                .map(|_| ValueInfo {
                    ty: Type::I64,
                    name: None,
                })
                .collect(),
            current_origin: None,
        };
        let facts = super::super::analyze_function(&f);
        assert_eq!(facts[&ValueId(5)].hi, i64::MAX as i128);
        assert_eq!(facts[&ValueId(1)], NumFact::new(1, 32));
    }

    /// `d = 1; while (<square test of d against n>) d++; return d`,
    /// `%0 n, %1 d, %2 d*d, %4 d+1`, the test's blocks given.
    fn counter_loop(n: Operand, test: Vec<Block>, n_ty: Type) -> Function {
        let mut blocks = vec![Block {
            id: BlockId(0),
            insts: vec![
                inst(0, InstKind::Copy(n_ty, n)),
                inst(1, InstKind::Copy(Type::I64, Operand::ConstI64(1))),
            ],
            term: Terminator::Br(BlockId(1)),
        }];
        blocks.extend(test);
        blocks.push(Block {
            id: BlockId(blocks.len() as u32),
            insts: vec![
                inst(4, InstKind::BinOp(BinOp::Add, v(1), Operand::ConstI64(1))),
                inst(1, InstKind::Copy(Type::I64, v(4))),
            ],
            term: Terminator::Br(BlockId(1)),
        });
        blocks.push(Block {
            id: BlockId(blocks.len() as u32),
            insts: vec![],
            term: Terminator::Ret(Some(v(1))),
        });
        Function {
            name: "f".into(),
            params: vec![],
            ret: Type::I64,
            blocks,
            values: (0..9)
                .map(|i| ValueInfo {
                    ty: if i == 5 || (i == 0 && n_ty == Type::F64) {
                        Type::F64
                    } else {
                        Type::I64
                    },
                    name: None,
                })
                .collect(),
            current_origin: None,
        }
    }

    fn cond(c: u32, t: u32, e: u32) -> Terminator {
        Terminator::CondBr {
            cond: v(c),
            then_blk: BlockId(t),
            else_blk: BlockId(e),
        }
    }

    /// `sitofp (d * d) <= n` in f64: b1 tests, b2 steps, b3 returns.
    fn f64_test(n: f64) -> Function {
        let test = vec![Block {
            id: BlockId(1),
            insts: vec![
                inst(2, InstKind::BinOp(BinOp::Mul, v(1), v(1))),
                inst(5, InstKind::SiToFp(v(2))),
                inst(3, InstKind::FCmp(FPred::Ole, v(5), v(0))),
            ],
            term: cond(3, 2, 3),
        }];
        counter_loop(Operand::ConstF64(n), test, Type::F64)
    }

    #[test]
    fn an_f64_square_test_bounds_the_counter_below_2_pow_53() {
        let facts = super::super::analyze_function(&f64_test(2_147_483_647.0));
        assert_eq!(facts[&ValueId(1)], NumFact::new(1, 46_341));
        // past 2^53 the compared bound may be a rounded one: only the
        // counter's trip horizon bounds it
        let facts = super::super::analyze_function(&f64_test(2f64.powi(60)));
        assert_eq!(facts[&ValueId(1)].hi, 1 + (1 << 48));
    }

    /// int_entry's copy: the product's window check in b1, the test on
    /// a copy of the product in b2, which only b1 enters.
    #[test]
    fn a_square_reaches_its_test_through_a_copy_and_a_check() {
        let test = vec![
            Block {
                id: BlockId(1),
                insts: vec![
                    inst(2, InstKind::BinOp(BinOp::Mul, v(1), v(1))),
                    inst(7, InstKind::BinOp(BinOp::LShr, v(2), Operand::ConstI64(53))),
                    inst(8, InstKind::ICmp(IPred::Ne, v(7), Operand::ConstI64(0))),
                ],
                term: cond(8, 4, 2),
            },
            Block {
                id: BlockId(2),
                insts: vec![
                    inst(6, InstKind::Copy(Type::I64, v(2))),
                    inst(3, InstKind::ICmp(IPred::Sle, v(6), v(0))),
                ],
                term: cond(3, 3, 4),
            },
        ];
        let f = counter_loop(Operand::ConstI64(1_000_000), test, Type::I64);
        let facts = super::super::analyze_function(&f);
        assert_eq!(facts[&ValueId(1)], NumFact::new(1, 1001));
    }

    #[test]
    fn a_bound_past_the_exact_square_leaves_the_counter_open() {
        // `d * d <= i64::MAX` holds for wrapped products too: only the
        // counter's trip horizon bounds it
        let facts = super::super::analyze_function(&trial_loop(i64::MAX, true));
        assert_eq!(facts[&ValueId(1)].hi, 1 + (1 << 48));
    }

    #[test]
    fn isqrt_floors() {
        assert_eq!(isqrt(0), 0);
        assert_eq!(isqrt(1), 1);
        assert_eq!(isqrt(15), 3);
        assert_eq!(isqrt(16), 4);
        assert_eq!(isqrt(2_147_483_647), 46_340);
        assert_eq!(isqrt(i64::MAX as i128), ROOT_MAX);
        assert_eq!(isqrt((1 << 106) - 1), (1 << 53) - 1);
    }

    #[test]
    fn root_is_bounded_only_while_the_product_is_exact() {
        let small = NumFact::new(1, 1 << 20);
        assert_eq!(root_range(small, 100, false), Some(NumFact::new(-10, 10)));
        assert_eq!(root_range(small, 100, true), Some(NumFact::new(-9, 9)));
        assert_eq!(root_range(small, 0, true), None);
        // a wrapped product says nothing of x
        let wide = NumFact::new(1, ROOT_MAX + 1);
        assert_eq!(root_range(wide, 100, false), None);
        let low = NumFact::new(-ROOT_MAX - 1, 0);
        assert_eq!(root_range(low, 100, false), None);
        assert_eq!(
            root_range(NumFact::new(-ROOT_MAX, ROOT_MAX), 100, false),
            Some(NumFact::new(-10, 10))
        );
        // an unbounded comparand bounds nothing
        assert_eq!(root_range(small, POS_INF, false), None);
    }

    #[test]
    fn only_an_upper_bound_on_the_square_bounds_the_root() {
        let (x, y) = (ValueId(1), ValueId(2));
        let c = |rel, left| square_constraint(x, rel, left, &Operand::Value(y), false);
        assert!(matches!(
            c(Rel::Le, true),
            Some(Constraint::Square { strict: false, .. })
        ));
        assert!(matches!(
            c(Rel::Lt, true),
            Some(Constraint::Square { strict: true, .. })
        ));
        // `y > x * x` written with the square on the right
        assert!(matches!(
            c(Rel::Gt, false),
            Some(Constraint::Square { strict: true, .. })
        ));
        assert!(c(Rel::Ge, true).is_none());
        assert!(c(Rel::Below, true).is_none());
    }
}
