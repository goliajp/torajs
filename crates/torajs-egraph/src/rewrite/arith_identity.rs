//! Arithmetic identity / annihilator rules — Phase 1 chunk 9a-2 +
//! Phase 1 chunk 9a-3 + Phase 2 chunk 10c (subtractive).
//!
//! - `(add x 0) → Identity(x)`  — additive identity.
//! - `(mul x 1) → Identity(x)`  — multiplicative identity.
//! - `(mul x 0) → 0`            — zero annihilator on Mul.
//! - `(sub x 0) → Identity(x)`  — subtractive zero identity.
//! - `(sub 0 x) → Neg(x)`       — subtract-from-zero negate
//!                                 (chunk 11b, sister to SubZero).
//!
//! AddZero / MulOne / MulZero rely on `CommutativeCanon`
//! (registry first) to land the const on the RHS, so each rule
//! matches the single RHS-Const arm only (chunk 10b simplification).
//! Sub is non-commutative — canon does not touch it — so SubZero
//! and SubNegate split the two LHS/RHS arms between them:
//! SubZero handles `sub x 0` (RHS-zero), SubNegate handles
//! `sub 0 x` (LHS-zero, the negate case). The two patterns are
//! disjoint so registry priority between them doesn't matter.
//!
//! AddZero / MulOne / SubZero rewrite to `InstKind::Identity(x)` so
//! the optimizer aliases `result` onto `x` via `set_opt_value` and
//! never emits an inst. MulZero rewrites to
//! `Identity(ConstI64(0))`; optimize.rs detects the const operand
//! and calls `set_const` on the eclass root. SubNegate rewrites to
//! `InstKind::Neg(x)`, which IS a real emit (one ALU op:
//! aarch64 `sub Xd, XZR, Xn` / LLVM `sub i64 0, %x`) — the eclass
//! keeps the original BinOp alongside Neg, and the cost model
//! picks Neg as strictly cheaper (1 ALU vs 1 ALU + the const
//! materialization MOVZ for the 0).

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

/// `(sub 0 x) → Neg(x)` — subtract-from-zero negate. Sister of
/// `SubZero`: Sub is non-commutative so canon does not touch it,
/// and the LHS-zero / RHS-zero arms are split between the two
/// rules (disjoint patterns).
pub struct SubNegate;

impl Rewrite for SubNegate {
    fn name(&self) -> &'static str {
        "sub_neg_to_negate"
    }
    fn try_apply(&self, lhs: &InstKind) -> Option<InstKind> {
        if let InstKind::BinOp(BinOp::Sub, Operand::ConstI64(0), b) = lhs {
            return Some(InstKind::Neg(*b));
        }
        None
    }
}

pub(crate) static ADD_ZERO: AddZero = AddZero;
pub(crate) static MUL_ONE: MulOne = MulOne;
pub(crate) static MUL_ZERO: MulZero = MulZero;
pub(crate) static SUB_ZERO: SubZero = SubZero;
pub(crate) static SUB_NEGATE: SubNegate = SubNegate;

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
        // `sub 0 x` — Sub isn't commutative; SubZero must NOT fire
        // (SubNegate handles the `0 - x = -x` case).
        let lhs2 = InstKind::BinOp(BinOp::Sub, Operand::ConstI64(0), val(7));
        assert_eq!(SubZero.try_apply(&lhs2), None);
        // wrong op doesn't fire.
        let add = InstKind::BinOp(BinOp::Add, val(4), Operand::ConstI64(0));
        assert_eq!(SubZero.try_apply(&add), None);
        // non-zero const doesn't fire.
        let one = InstKind::BinOp(BinOp::Sub, val(4), Operand::ConstI64(1));
        assert_eq!(SubZero.try_apply(&one), None);
    }

    #[test]
    fn sub_negate_rewrites_lhs_zero_to_neg() {
        // `sub 0 x` → Neg(x).
        let lhs = InstKind::BinOp(BinOp::Sub, Operand::ConstI64(0), val(7));
        assert_eq!(
            SubNegate.try_apply(&lhs),
            Some(InstKind::Neg(Operand::Value(ValueId(7))))
        );
        // `sub x 0` — SubZero territory; SubNegate must NOT fire.
        let rhs_zero = InstKind::BinOp(BinOp::Sub, val(4), Operand::ConstI64(0));
        assert_eq!(SubNegate.try_apply(&rhs_zero), None);
        // LHS = non-zero const — doesn't fire.
        let lhs_non_zero = InstKind::BinOp(BinOp::Sub, Operand::ConstI64(3), val(7));
        assert_eq!(SubNegate.try_apply(&lhs_non_zero), None);
        // Wrong op (Add) — doesn't fire.
        let add = InstKind::BinOp(BinOp::Add, Operand::ConstI64(0), val(7));
        assert_eq!(SubNegate.try_apply(&add), None);
        // Both operands const — `sub 0 0` fires (Neg(0)); harmless,
        // const-fold will follow if/when a Neg(Const) rule lands.
        let both_const = InstKind::BinOp(BinOp::Sub, Operand::ConstI64(0), Operand::ConstI64(5));
        assert_eq!(
            SubNegate.try_apply(&both_const),
            Some(InstKind::Neg(Operand::ConstI64(5)))
        );
    }
}
