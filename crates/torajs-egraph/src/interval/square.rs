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

use std::collections::HashMap;

use torajs_core::ssa::{BinOp, Block, InstKind, Operand, ValueId};

use super::refine::Rel;
use super::transfer::{NumFact, POS_INF, op_const_int};

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

/// The root `x` of `m = mul x, x` when `m` is defined in `block` and
/// `x` is not redefined after it there, so the branch closing the
/// block still speaks of the `x` live on its edges.
pub(super) fn root_in_block(
    block: &Block,
    m: ValueId,
    defs: &HashMap<ValueId, InstKind>,
) -> Option<ValueId> {
    let Some(InstKind::BinOp(BinOp::Mul, Operand::Value(x), Operand::Value(y))) = defs.get(&m)
    else {
        return None;
    };
    if x != y {
        return None;
    }
    let at = block.insts.iter().position(|i| i.result == Some(m))?;
    let redefined = block.insts[at + 1..].iter().any(|i| i.result == Some(*x));
    (!redefined).then_some(*x)
}

/// The constraint on `root` when its square is the left (`left`) or
/// right operand of a compare whose edge says `rel` of the left one.
pub(super) fn square_constraint(
    root: ValueId,
    rel: Rel,
    left: bool,
    other: &Operand,
) -> Option<super::refine::Constraint> {
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
    Some(super::refine::Constraint::Square {
        value: root,
        strict,
        bound,
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

/// The widening thresholds of one round with the roots of every
/// square's bound as it now stands, so a squared counter stops at the
/// root (and one past it, where the latch leaves it) before a
/// sentinel.
pub(super) fn with_roots(
    thresholds: &[i128],
    bounds: &[(SquareBound, bool)],
    hi_of: impl Fn(ValueId) -> Option<i128>,
) -> Vec<i128> {
    let mut t = thresholds.to_vec();
    for &(bound, strict) in bounds {
        let hi = match bound {
            SquareBound::Const(k) => Some(k),
            SquareBound::Value(b) => hi_of(b),
        };
        if let Some(hi) = hi.map(|h| h - i128::from(strict))
            && (0..POS_INF).contains(&hi)
        {
            let r = isqrt(hi);
            t.extend([r, r + 1]);
        }
    }
    t.sort_unstable();
    t.dedup();
    t
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{BlockId, Function, IPred, Inst, Terminator, Type, ValueInfo};

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

    #[test]
    fn a_bound_past_the_exact_square_leaves_the_counter_open() {
        // `d * d <= i64::MAX` holds for wrapped products too
        let facts = super::super::analyze_function(&trial_loop(i64::MAX, true));
        assert_eq!(facts[&ValueId(1)].hi, POS_INF);
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
        let c = |rel, left| square_constraint(x, rel, left, &Operand::Value(y));
        assert!(matches!(
            c(Rel::Le, true),
            Some(super::super::refine::Constraint::Square { strict: false, .. })
        ));
        assert!(matches!(
            c(Rel::Lt, true),
            Some(super::super::refine::Constraint::Square { strict: true, .. })
        ));
        // `y > x * x` written with the square on the right
        assert!(matches!(
            c(Rel::Gt, false),
            Some(super::super::refine::Constraint::Square { strict: true, .. })
        ));
        assert!(c(Rel::Ge, true).is_none());
        assert!(c(Rel::Below, true).is_none());
    }
}
