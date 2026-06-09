//! Arithmetic identity / annihilator rules — Phase 1 chunk 9a-2 +
//! Phase 1 chunk 9a-3 + Phase 2 chunk 10c (subtractive).
//!
//! - `(add x 0) → Identity(x)`  — additive identity.
//! - `(mul x 1) → Identity(x)`  — multiplicative identity.
//! - `(mul x 0) → 0`            — zero annihilator on Mul.
//! - `(sub x 0) → Identity(x)`  — subtractive zero identity.
//!
//! AddZero / MulOne / MulZero rely on `CommutativeCanon`
//! (registry first) to land the const on the RHS, so each rule
//! matches the single RHS-Const arm only (chunk 10b simplification).
//! SubZero is RHS-only by SSA emission shape (Sub is non-commutative,
//! `sub 0 x → -x` is a separate negate rewrite deferred to a later
//! chunk).
//!
//! AddZero / MulOne / SubZero rewrite to `InstKind::Identity(x)` so
//! the optimizer aliases `result` onto `x` via `set_opt_value` and
//! never emits an inst. MulZero rewrites to
//! `Identity(ConstI64(0))`; optimize.rs detects the const operand
//! and calls `set_const` on the eclass root.

use crate::rewrite::Rewrite;
use torajs_core::ssa::{BinOp, InstKind, Operand};

/// `(add x 0) → Identity(x)` — additive identity. Relies on
/// CommutativeCanon to land the const on the RHS.
pub struct AddZero;

impl Rewrite for AddZero {
    fn name(&self) -> &'static str {
        "add_zero_to_identity"
    }
    fn try_apply(&self, lhs: &InstKind) -> Option<InstKind> {
        if let InstKind::BinOp(BinOp::Add, a, Operand::ConstI64(0)) = lhs {
            return Some(InstKind::Identity(*a));
        }
        None
    }
}

/// `(mul x 1) → Identity(x)` — multiplicative identity. Relies on
/// CommutativeCanon to land the const on the RHS.
pub struct MulOne;

impl Rewrite for MulOne {
    fn name(&self) -> &'static str {
        "mul_one_to_identity"
    }
    fn try_apply(&self, lhs: &InstKind) -> Option<InstKind> {
        if let InstKind::BinOp(BinOp::Mul, a, Operand::ConstI64(1)) = lhs {
            return Some(InstKind::Identity(*a));
        }
        None
    }
}

/// `(mul x 0) → 0` — zero-annihilator. Relies on CommutativeCanon
/// to land the const on the RHS.
pub struct MulZero;

impl Rewrite for MulZero {
    fn name(&self) -> &'static str {
        "mul_zero_to_zero"
    }
    fn try_apply(&self, lhs: &InstKind) -> Option<InstKind> {
        if let InstKind::BinOp(BinOp::Mul, _, Operand::ConstI64(0)) = lhs {
            return Some(InstKind::Identity(Operand::ConstI64(0)));
        }
        None
    }
}

/// `(sub x 0) → Identity(x)` — subtractive zero identity. RHS-only
/// by SSA emission shape (Sub is non-commutative).
pub struct SubZero;

impl Rewrite for SubZero {
    fn name(&self) -> &'static str {
        "sub_zero_to_identity"
    }
    fn try_apply(&self, lhs: &InstKind) -> Option<InstKind> {
        if let InstKind::BinOp(BinOp::Sub, a, Operand::ConstI64(0)) = lhs {
            return Some(InstKind::Identity(*a));
        }
        None
    }
}

pub(crate) static ADD_ZERO: AddZero = AddZero;
pub(crate) static MUL_ONE: MulOne = MulOne;
pub(crate) static MUL_ZERO: MulZero = MulZero;
pub(crate) static SUB_ZERO: SubZero = SubZero;

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::ValueId;

    fn val(n: u32) -> Operand {
        Operand::Value(ValueId(n))
    }

    #[test]
    fn add_zero_rewrites_to_identity_rhs_only_after_canon() {
        // (add x 0) → Identity(x)
        let lhs = InstKind::BinOp(BinOp::Add, val(4), Operand::ConstI64(0));
        assert_eq!(
            AddZero.try_apply(&lhs),
            Some(InstKind::Identity(Operand::Value(ValueId(4))))
        );
        // (add 0 x) — chunk 10b drops the LHS-Const arm; CommutativeCanon
        // (registry first) lands the const on the RHS before AddZero
        // sees it. Direct call no longer fires.
        let lhs2 = InstKind::BinOp(BinOp::Add, Operand::ConstI64(0), val(7));
        assert!(AddZero.try_apply(&lhs2).is_none());
        // (add x 1) — doesn't fire.
        let nope = InstKind::BinOp(BinOp::Add, val(4), Operand::ConstI64(1));
        assert!(AddZero.try_apply(&nope).is_none());
        // (sub x 0) — wrong op, doesn't fire.
        let other_op = InstKind::BinOp(BinOp::Sub, val(4), Operand::ConstI64(0));
        assert!(AddZero.try_apply(&other_op).is_none());
    }

    #[test]
    fn mul_one_rewrites_to_identity_rhs_only_after_canon() {
        // (mul x 1) → Identity(x)
        let lhs = InstKind::BinOp(BinOp::Mul, val(4), Operand::ConstI64(1));
        assert_eq!(
            MulOne.try_apply(&lhs),
            Some(InstKind::Identity(Operand::Value(ValueId(4))))
        );
        // (mul 1 x) — chunk 10b drops the LHS-Const arm; canon swaps
        // first. Direct call no longer fires.
        let lhs2 = InstKind::BinOp(BinOp::Mul, Operand::ConstI64(1), val(9));
        assert!(MulOne.try_apply(&lhs2).is_none());
        // (mul x 0) — doesn't fire (MulZero is a Phase 1 chunk 9a-3 rule).
        let zero = InstKind::BinOp(BinOp::Mul, val(4), Operand::ConstI64(0));
        assert!(MulOne.try_apply(&zero).is_none());
        // (mul x 2) — doesn't fire (MulPow2ToShl handles it).
        let two = InstKind::BinOp(BinOp::Mul, val(4), Operand::ConstI64(2));
        assert!(MulOne.try_apply(&two).is_none());
    }

    #[test]
    fn mul_zero_folds_rhs_only_after_canon() {
        let rhs_zero = InstKind::BinOp(BinOp::Mul, val(4), Operand::ConstI64(0));
        assert_eq!(
            MulZero.try_apply(&rhs_zero),
            Some(InstKind::Identity(Operand::ConstI64(0)))
        );
        // chunk 10b drops the LHS-Const arm; canon swaps first.
        let lhs_zero = InstKind::BinOp(BinOp::Mul, Operand::ConstI64(0), val(4));
        assert!(MulZero.try_apply(&lhs_zero).is_none());
        // mul x 1 — doesn't fire (MulOne handles).
        let one = InstKind::BinOp(BinOp::Mul, val(4), Operand::ConstI64(1));
        assert!(MulZero.try_apply(&one).is_none());
    }

    #[test]
    fn sub_zero_rewrites_to_identity() {
        let lhs = InstKind::BinOp(BinOp::Sub, val(4), Operand::ConstI64(0));
        assert_eq!(
            SubZero.try_apply(&lhs),
            Some(InstKind::Identity(Operand::Value(ValueId(4))))
        );
        // `sub 0 x` — Sub isn't commutative; rule must NOT fire (this
        // is the `0 - x = -x` negate case, deferred to a later rule).
        let lhs2 = InstKind::BinOp(BinOp::Sub, Operand::ConstI64(0), val(7));
        assert_eq!(SubZero.try_apply(&lhs2), None);
        // wrong op doesn't fire.
        let add = InstKind::BinOp(BinOp::Add, val(4), Operand::ConstI64(0));
        assert_eq!(SubZero.try_apply(&add), None);
        // non-zero const doesn't fire.
        let one = InstKind::BinOp(BinOp::Sub, val(4), Operand::ConstI64(1));
        assert_eq!(SubZero.try_apply(&one), None);
    }
}
