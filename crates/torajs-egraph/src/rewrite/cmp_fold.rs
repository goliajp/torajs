//! Constant folding for compares over two literals —
//! `(icmp p ConstI64 ConstI64)` and `(fcmp p ConstF64 ConstF64)`
//! collapse to `Identity(ConstBool)`. Inlining a call with a literal
//! argument leaves its guards in this shape (`ackermann(m - 1, 1)`
//! inlined tests `1 === 0`); the folded condition then turns the
//! branch unconditional in elaboration.
//!
//! The ordered predicates are false when either side is NaN and `une`
//! is true, as IEEE 754 compares and ECMA-262 Number::equal /
//! Number::lessThan answer.

use crate::rewrite::Rewrite;
use torajs_core::ssa::{FPred, IPred, InstKind, Operand};

pub struct CmpConstFold;

impl Rewrite for CmpConstFold {
    fn name(&self) -> &'static str {
        "cmp_const_fold"
    }
    fn try_apply(&self, lhs: &InstKind) -> Option<InstKind> {
        let b = match lhs {
            InstKind::ICmp(p, Operand::ConstI64(a), Operand::ConstI64(b)) => icmp(*p, *a, *b),
            InstKind::FCmp(p, Operand::ConstF64(a), Operand::ConstF64(b)) => fcmp(*p, *a, *b),
            _ => return None,
        };
        Some(InstKind::Identity(Operand::ConstBool(b)))
    }
}

fn icmp(p: IPred, a: i64, b: i64) -> bool {
    match p {
        IPred::Eq => a == b,
        IPred::Ne => a != b,
        IPred::Slt => a < b,
        IPred::Sgt => a > b,
        IPred::Sle => a <= b,
        IPred::Sge => a >= b,
        IPred::Ult => (a as u64) < (b as u64),
        IPred::Uge => (a as u64) >= (b as u64),
    }
}

fn fcmp(p: FPred, a: f64, b: f64) -> bool {
    match p {
        FPred::Oeq => a == b,
        FPred::One => a < b || a > b,
        FPred::Olt => a < b,
        FPred::Ogt => a > b,
        FPred::Ole => a <= b,
        FPred::Oge => a >= b,
        FPred::Une => a != b,
    }
}

pub(crate) static CMP_CONST_FOLD: CmpConstFold = CmpConstFold;

#[cfg(test)]
mod tests {
    use super::*;

    fn fold(k: InstKind) -> Option<bool> {
        match CmpConstFold.try_apply(&k) {
            Some(InstKind::Identity(Operand::ConstBool(b))) => Some(b),
            None => None,
            other => panic!("unexpected fold {other:?}"),
        }
    }

    fn f(p: FPred, a: f64, b: f64) -> Option<bool> {
        fold(InstKind::FCmp(
            p,
            Operand::ConstF64(a),
            Operand::ConstF64(b),
        ))
    }

    fn i(p: IPred, a: i64, b: i64) -> Option<bool> {
        fold(InstKind::ICmp(
            p,
            Operand::ConstI64(a),
            Operand::ConstI64(b),
        ))
    }

    #[test]
    fn integer_predicates() {
        assert_eq!(i(IPred::Eq, 1, 0), Some(false));
        assert_eq!(i(IPred::Ne, 1, 0), Some(true));
        assert_eq!(i(IPred::Slt, -1, 0), Some(true));
        assert_eq!(i(IPred::Ult, -1, 0), Some(false));
        assert_eq!(i(IPred::Uge, -1, 0), Some(true));
        assert_eq!(i(IPred::Sge, 3, 3), Some(true));
    }

    #[test]
    fn float_predicates_keep_nan_and_signed_zero() {
        assert_eq!(f(FPred::Oeq, 1.0, 0.0), Some(false));
        assert_eq!(f(FPred::Oeq, -0.0, 0.0), Some(true));
        assert_eq!(f(FPred::Oeq, f64::NAN, f64::NAN), Some(false));
        assert_eq!(f(FPred::Une, f64::NAN, f64::NAN), Some(true));
        assert_eq!(f(FPred::One, f64::NAN, 1.0), Some(false));
        assert_eq!(f(FPred::One, 2.0, 1.0), Some(true));
        assert_eq!(f(FPred::Olt, f64::NAN, 1.0), Some(false));
        assert_eq!(f(FPred::Oge, f64::INFINITY, f64::MAX), Some(true));
    }

    #[test]
    fn a_non_literal_side_does_not_fire() {
        let v = Operand::Value(torajs_core::ssa::ValueId(0));
        assert_eq!(
            fold(InstKind::FCmp(FPred::Oeq, v, Operand::ConstF64(0.0))),
            None
        );
        assert_eq!(
            fold(InstKind::ICmp(IPred::Eq, Operand::ConstI64(1), v)),
            None
        );
        assert_eq!(
            fold(InstKind::FCmp(
                FPred::Oeq,
                Operand::ConstI64(1),
                Operand::ConstI64(0)
            )),
            None
        );
    }
}
