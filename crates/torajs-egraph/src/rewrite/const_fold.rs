//! Constant folding for BinOps over two literals —
//! `(op ConstI64(a) ConstI64(b))` collapses to `Identity(ConstI64(folded))`
//! and `(fop ConstF64(a) ConstF64(b))` to `Identity(ConstF64(folded))`,
//! picked up by `optimize.rs::try_rewrite`'s existing `set_const` path so
//! downstream e-class consumers see the literal value.
//!
//! Covers the 11 integer BinOps `BinOp::{Add, Sub, Mul, SDiv, SRem,
//! And, Or, Xor, Shl, AShr, LShr}` and the 5 FP BinOps `FAdd / FSub /
//! FMul / FDiv / FRem`. FP folding is exact: ECMA-262 Number arithmetic
//! is IEEE 754 binary64 with round-to-nearest-even, which is what the
//! host's f64 ops compute, and `%` on Numbers is the truncating
//! remainder that Rust's f64 `%` computes. Signed zeros, infinities and
//! NaN come out as at run time; only a NaN's payload may differ, and
//! the spec leaves NaN bit patterns implementation-defined.
//!
//! Guards mirror LLVM `ConstantFold`:
//! - `SDiv` / `SRem` by zero → bail (would panic in `wrapping_div`)
//! - `i64::MIN s/ -1` → bail (Rust panics; LLVM treats as UB)
//! - Shift counts outside `0..64` → bail (poison in LLVM; let the
//!   backend handle)
//!
//! All arithmetic uses `wrapping_*` to match LLVM's two-complement
//! `add/sub/mul nuw/nsw` flag-free behaviour (torajs does not emit
//! nuw/nsw flags; overflow wraps).
//!
//! Constant operand order: by the time const-fold runs, `CommutativeCanon`
//! has already put any single constant on the RHS. The "both operands
//! const" case is the only thing left for this rule — identity /
//! annihilator rules above won't fire because they pattern-match a
//! const on RHS but a Value on LHS.

use crate::rewrite::Rewrite;
use torajs_core::ssa::{BinOp, InstKind, Operand};

/// `(op ConstI64(a) ConstI64(b)) → Identity(ConstI64(folded))` for
/// integer BinOps with well-defined evaluation. See module doc for
/// guard list.
pub struct BinOpConstFold;

impl Rewrite for BinOpConstFold {
    fn name(&self) -> &'static str {
        "binop_const_fold"
    }
    fn try_apply(&self, lhs: &InstKind) -> Option<InstKind> {
        if let InstKind::BinOp(op, Operand::ConstF64(a), Operand::ConstF64(b)) = lhs {
            return fold_f64(*op, *a, *b).map(|x| InstKind::Identity(Operand::ConstF64(x)));
        }
        let InstKind::BinOp(op, Operand::ConstI64(a), Operand::ConstI64(b)) = lhs else {
            return None;
        };
        let (a, b) = (*a, *b);
        let folded: i64 = match op {
            BinOp::Add => a.wrapping_add(b),
            BinOp::Sub => a.wrapping_sub(b),
            BinOp::Mul => a.wrapping_mul(b),
            BinOp::SDiv => {
                if b == 0 || (a == i64::MIN && b == -1) {
                    return None;
                }
                a.wrapping_div(b)
            }
            BinOp::SRem => {
                if b == 0 || (a == i64::MIN && b == -1) {
                    return None;
                }
                a.wrapping_rem(b)
            }
            BinOp::And => a & b,
            BinOp::Or => a | b,
            BinOp::Xor => a ^ b,
            BinOp::Shl => {
                if !(0..64).contains(&b) {
                    return None;
                }
                (a as u64).wrapping_shl(b as u32) as i64
            }
            BinOp::AShr => {
                if !(0..64).contains(&b) {
                    return None;
                }
                a.wrapping_shr(b as u32)
            }
            BinOp::LShr => {
                if !(0..64).contains(&b) {
                    return None;
                }
                ((a as u64).wrapping_shr(b as u32)) as i64
            }
            _ => return None,
        };
        Some(InstKind::Identity(Operand::ConstI64(folded)))
    }
}

fn fold_f64(op: BinOp, a: f64, b: f64) -> Option<f64> {
    Some(match op {
        BinOp::FAdd => a + b,
        BinOp::FSub => a - b,
        BinOp::FMul => a * b,
        BinOp::FDiv => a / b,
        BinOp::FRem => a % b,
        _ => return None,
    })
}

pub(crate) static BIN_OP_CONST_FOLD: BinOpConstFold = BinOpConstFold;

/// `sitofp ConstI64(n) → Identity(ConstF64(n as f64))`. Rust's
/// integer-to-float `as` rounds to nearest, ties to even — the IEEE
/// default the runtime conversion (`scvtf` under the default FPCR)
/// uses — so the literal is the value the instruction would produce.
/// The shape arises when an inlined callee's integer-typed parameter
/// (`scale(k: number)` lowered with `k: i64`) receives a literal: the
/// float use of the parameter stays a conversion of a constant, which
/// keeps one f64 register live across the caller's loop and hides the
/// integer value from the rules and passes that read constant operands.
pub struct SiToFpConstFold;

impl Rewrite for SiToFpConstFold {
    fn name(&self) -> &'static str {
        "sitofp_const_fold"
    }
    fn try_apply(&self, lhs: &InstKind) -> Option<InstKind> {
        let n = match lhs {
            InstKind::SiToFp(Operand::ConstI64(n)) => *n,
            InstKind::SiToFp(Operand::ConstI32(n)) => *n as i64,
            _ => return None,
        };
        Some(InstKind::Identity(Operand::ConstF64(n as f64)))
    }
}

pub(crate) static SI_TO_FP_CONST_FOLD: SiToFpConstFold = SiToFpConstFold;

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::ValueId;

    fn fold(op: BinOp, a: i64, b: i64) -> Option<InstKind> {
        BinOpConstFold.try_apply(&InstKind::BinOp(
            op,
            Operand::ConstI64(a),
            Operand::ConstI64(b),
        ))
    }

    fn expect_const(out: Option<InstKind>, expected: i64) {
        assert_eq!(out, Some(InstKind::Identity(Operand::ConstI64(expected))));
    }

    #[test]
    fn add_sub_mul_basic() {
        expect_const(fold(BinOp::Add, 3, 5), 8);
        expect_const(fold(BinOp::Sub, 10, 3), 7);
        expect_const(fold(BinOp::Mul, 6, 7), 42);
    }

    #[test]
    fn add_sub_mul_wraps_two_complement() {
        // i64::MAX + 1 wraps to i64::MIN
        expect_const(fold(BinOp::Add, i64::MAX, 1), i64::MIN);
        // i64::MIN - 1 wraps to i64::MAX
        expect_const(fold(BinOp::Sub, i64::MIN, 1), i64::MAX);
        // i64::MIN * -1 wraps to i64::MIN (overflow)
        expect_const(fold(BinOp::Mul, i64::MIN, -1), i64::MIN);
    }

    #[test]
    fn sdiv_srem_basic_and_signed() {
        expect_const(fold(BinOp::SDiv, 20, 4), 5);
        expect_const(fold(BinOp::SDiv, -20, 4), -5);
        expect_const(fold(BinOp::SDiv, -7, 2), -3); // truncate toward 0
        expect_const(fold(BinOp::SRem, 20, 6), 2);
        expect_const(fold(BinOp::SRem, -7, 2), -1); // sign follows dividend
    }

    #[test]
    fn sdiv_srem_by_zero_does_not_fire() {
        assert!(fold(BinOp::SDiv, 5, 0).is_none());
        assert!(fold(BinOp::SRem, 5, 0).is_none());
    }

    #[test]
    fn sdiv_srem_min_div_neg_one_does_not_fire() {
        // i64::MIN / -1 overflows (the only signed-division overflow);
        // LLVM treats as UB. Bail to keep semantics.
        assert!(fold(BinOp::SDiv, i64::MIN, -1).is_none());
        assert!(fold(BinOp::SRem, i64::MIN, -1).is_none());
        // i64::MIN / -1 on non-MIN dividend is fine.
        expect_const(fold(BinOp::SDiv, -10, -1), 10);
    }

    #[test]
    fn bitwise_and_or_xor() {
        expect_const(fold(BinOp::And, 0b1100, 0b1010), 0b1000);
        expect_const(fold(BinOp::Or, 0b1100, 0b1010), 0b1110);
        expect_const(fold(BinOp::Xor, 0b1100, 0b1010), 0b0110);
        // All-ones mask
        expect_const(fold(BinOp::And, -1, 0xFF), 0xFF);
        expect_const(fold(BinOp::Or, 0, -1), -1);
    }

    #[test]
    fn shifts_in_range() {
        expect_const(fold(BinOp::Shl, 1, 8), 256);
        expect_const(fold(BinOp::AShr, -16, 2), -4); // arithmetic — sign-extends
        expect_const(fold(BinOp::LShr, -1, 1), i64::MAX); // logical — zero-fills
        // Shift by 0 is fine (identity-equivalent, but const-fold still emits the value)
        expect_const(fold(BinOp::Shl, 42, 0), 42);
        expect_const(fold(BinOp::AShr, 42, 0), 42);
        expect_const(fold(BinOp::LShr, 42, 0), 42);
        // Shift by 63 (max in-range)
        expect_const(fold(BinOp::Shl, 1, 63), i64::MIN);
        expect_const(fold(BinOp::LShr, -1, 63), 1);
    }

    #[test]
    fn shifts_out_of_range_do_not_fire() {
        // count >= 64 is poison in LLVM — let backend handle
        assert!(fold(BinOp::Shl, 1, 64).is_none());
        assert!(fold(BinOp::AShr, 1, 64).is_none());
        assert!(fold(BinOp::LShr, 1, 64).is_none());
        assert!(fold(BinOp::Shl, 1, 100).is_none());
        // Negative shift count is also undefined — bail
        assert!(fold(BinOp::Shl, 1, -1).is_none());
        assert!(fold(BinOp::AShr, 1, -1).is_none());
    }

    #[test]
    fn partial_const_does_not_fire() {
        // Const on LHS, Value on RHS — not the both-const case.
        let lhs = InstKind::BinOp(BinOp::Add, Operand::ConstI64(3), Operand::Value(ValueId(4)));
        assert!(BinOpConstFold.try_apply(&lhs).is_none());
        // Value on LHS, Const on RHS — handled by identity / strength-reduction rules,
        // not const-fold.
        let rhs = InstKind::BinOp(BinOp::Add, Operand::Value(ValueId(4)), Operand::ConstI64(3));
        assert!(BinOpConstFold.try_apply(&rhs).is_none());
        // Both values — not const-fold either.
        let vv = InstKind::BinOp(
            BinOp::Add,
            Operand::Value(ValueId(4)),
            Operand::Value(ValueId(5)),
        );
        assert!(BinOpConstFold.try_apply(&vv).is_none());
    }

    fn fold_f(op: BinOp, a: f64, b: f64) -> Option<f64> {
        match BinOpConstFold.try_apply(&InstKind::BinOp(
            op,
            Operand::ConstF64(a),
            Operand::ConstF64(b),
        )) {
            Some(InstKind::Identity(Operand::ConstF64(x))) => Some(x),
            None => None,
            other => panic!("unexpected fold {other:?}"),
        }
    }

    #[test]
    fn fp_binops_fold_exactly() {
        assert_eq!(fold_f(BinOp::FAdd, 0.1, 0.2), Some(0.1 + 0.2));
        assert_eq!(fold_f(BinOp::FSub, 1.0, 3.0), Some(-2.0));
        assert_eq!(fold_f(BinOp::FMul, 1.5, 4.0), Some(6.0));
        assert_eq!(fold_f(BinOp::FDiv, 1.0, 3.0), Some(1.0 / 3.0));
        assert_eq!(fold_f(BinOp::FRem, -5.5, 2.0), Some(-1.5));
    }

    #[test]
    fn fp_folds_keep_ieee_edges() {
        let neg_zero = fold_f(BinOp::FMul, -1.0, 0.0).unwrap();
        assert!(neg_zero == 0.0 && neg_zero.is_sign_negative());
        let rem_zero = fold_f(BinOp::FRem, -4.0, 2.0).unwrap();
        assert!(rem_zero == 0.0 && rem_zero.is_sign_negative());
        assert_eq!(fold_f(BinOp::FDiv, 1.0, 0.0), Some(f64::INFINITY));
        assert_eq!(fold_f(BinOp::FDiv, -1.0, 0.0), Some(f64::NEG_INFINITY));
        assert!(fold_f(BinOp::FDiv, 0.0, 0.0).unwrap().is_nan());
        assert!(fold_f(BinOp::FRem, 1.0, 0.0).unwrap().is_nan());
        assert_eq!(fold_f(BinOp::FRem, 5.0, f64::INFINITY), Some(5.0));
    }

    #[test]
    fn fp_ops_on_int_literals_and_int_ops_on_fp_literals_do_not_fire() {
        for op in [
            BinOp::FAdd,
            BinOp::FSub,
            BinOp::FMul,
            BinOp::FDiv,
            BinOp::FRem,
        ] {
            assert!(fold(op, 1, 2).is_none(), "FP op {:?} on i64 literals", op);
        }
        assert_eq!(fold_f(BinOp::Add, 1.0, 2.0), None);
    }

    #[test]
    fn sitofp_of_a_literal_folds_with_round_to_nearest_even() {
        let fold_c = |op| SiToFpConstFold.try_apply(&InstKind::SiToFp(op));
        assert_eq!(
            fold_c(Operand::ConstI64(2)),
            Some(InstKind::Identity(Operand::ConstF64(2.0)))
        );
        assert_eq!(
            fold_c(Operand::ConstI32(-7)),
            Some(InstKind::Identity(Operand::ConstF64(-7.0)))
        );
        // 2^53 + 1 is a tie between 2^53 and 2^53 + 2: even wins
        assert_eq!(
            fold_c(Operand::ConstI64((1 << 53) + 1)),
            Some(InstKind::Identity(Operand::ConstF64(9007199254740992.0)))
        );
        // 2^53 + 3 is a tie between 2^53 + 2 and 2^53 + 4: even wins
        assert_eq!(
            fold_c(Operand::ConstI64((1 << 53) + 3)),
            Some(InstKind::Identity(Operand::ConstF64(9007199254740996.0)))
        );
        assert_eq!(fold_c(Operand::Value(ValueId(3))), None);
    }

    #[test]
    fn non_binop_inst_does_not_fire() {
        // ICmp, FCmp, Identity, Neg — not BinOp, so the rule bails on
        // the let-else.
        let icmp = InstKind::ICmp(
            torajs_core::ssa::IPred::Eq,
            Operand::ConstI64(1),
            Operand::ConstI64(2),
        );
        assert!(BinOpConstFold.try_apply(&icmp).is_none());
        let id = InstKind::Identity(Operand::ConstI64(42));
        assert!(BinOpConstFold.try_apply(&id).is_none());
        let neg = InstKind::Neg(Operand::ConstI64(5));
        assert!(BinOpConstFold.try_apply(&neg).is_none());
    }
}
