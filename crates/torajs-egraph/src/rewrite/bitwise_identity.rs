//! Bitwise identity / annihilator rules — Phase 2 chunk 10c.
//!
//! All five rely on `CommutativeCanon` (registry first) landing the
//! const operand on the RHS, so each rule matches the single
//! RHS-Const arm only.
//!
//! - `(xor x 0)  → Identity(x)`  — xor zero identity.
//! - `(or  x 0)  → Identity(x)`  — or zero identity.
//! - `(and x -1) → Identity(x)`  — bitwise all-ones identity for i64
//!   (`-1` two's-complement is `0xFFFF_FFFF_FFFF_FFFF`).
//! - `(and x 0)  → 0`            — And-with-zero annihilator.
//! - `(or  x -1) → -1`           — Or-with-all-ones annihilator.

use crate::rewrite::Rewrite;
use torajs_core::ssa::{BinOp, InstKind, Operand};

/// `(xor x 0) → Identity(x)` — xor zero identity. Relies on
/// CommutativeCanon to land the const on the RHS.
pub struct XorZero;

impl Rewrite for XorZero {
    fn name(&self) -> &'static str {
        "xor_zero_to_identity"
    }
    fn try_apply(&self, lhs: &InstKind) -> Option<InstKind> {
        if let InstKind::BinOp(BinOp::Xor, a, Operand::ConstI64(0)) = lhs {
            return Some(InstKind::Identity(*a));
        }
        None
    }
}

/// `(or x 0) → Identity(x)` — or zero identity. Relies on
/// CommutativeCanon to land the const on the RHS.
pub struct OrZero;

impl Rewrite for OrZero {
    fn name(&self) -> &'static str {
        "or_zero_to_identity"
    }
    fn try_apply(&self, lhs: &InstKind) -> Option<InstKind> {
        if let InstKind::BinOp(BinOp::Or, a, Operand::ConstI64(0)) = lhs {
            return Some(InstKind::Identity(*a));
        }
        None
    }
}

/// `(and x -1) → Identity(x)` — bitwise all-ones identity for i64
/// (`-1` two's-complement is `0xFFFF_FFFF_FFFF_FFFF`). Relies on
/// CommutativeCanon to land the const on the RHS.
pub struct AndAllOnes;

impl Rewrite for AndAllOnes {
    fn name(&self) -> &'static str {
        "and_all_ones_to_identity"
    }
    fn try_apply(&self, lhs: &InstKind) -> Option<InstKind> {
        if let InstKind::BinOp(BinOp::And, a, Operand::ConstI64(-1)) = lhs {
            return Some(InstKind::Identity(*a));
        }
        None
    }
}

/// `(and x 0) → 0` — And-with-zero annihilator. Relies on
/// CommutativeCanon to land the const on the RHS.
pub struct AndZero;

impl Rewrite for AndZero {
    fn name(&self) -> &'static str {
        "and_zero_to_zero"
    }
    fn try_apply(&self, lhs: &InstKind) -> Option<InstKind> {
        if let InstKind::BinOp(BinOp::And, _, Operand::ConstI64(0)) = lhs {
            return Some(InstKind::Identity(Operand::ConstI64(0)));
        }
        None
    }
}

/// `(or x -1) → -1` — Or-with-all-ones annihilator. Relies on
/// CommutativeCanon to land the const on the RHS.
pub struct OrAllOnes;

impl Rewrite for OrAllOnes {
    fn name(&self) -> &'static str {
        "or_all_ones_to_all_ones"
    }
    fn try_apply(&self, lhs: &InstKind) -> Option<InstKind> {
        if let InstKind::BinOp(BinOp::Or, _, Operand::ConstI64(-1)) = lhs {
            return Some(InstKind::Identity(Operand::ConstI64(-1)));
        }
        None
    }
}

pub(crate) static XOR_ZERO: XorZero = XorZero;
pub(crate) static OR_ZERO: OrZero = OrZero;
pub(crate) static AND_ALL_ONES: AndAllOnes = AndAllOnes;
pub(crate) static AND_ZERO: AndZero = AndZero;
pub(crate) static OR_ALL_ONES: OrAllOnes = OrAllOnes;

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::ValueId;

    fn val(n: u32) -> Operand {
        Operand::Value(ValueId(n))
    }

    #[test]
    fn xor_zero_rewrites_to_identity_rhs_only_after_canon() {
        let lhs = InstKind::BinOp(BinOp::Xor, val(4), Operand::ConstI64(0));
        assert_eq!(
            XorZero.try_apply(&lhs),
            Some(InstKind::Identity(Operand::Value(ValueId(4))))
        );
        // LHS-Const direct-call no longer fires; canon swaps first.
        let lhs2 = InstKind::BinOp(BinOp::Xor, Operand::ConstI64(0), val(7));
        assert!(XorZero.try_apply(&lhs2).is_none());
        // non-zero const doesn't fire.
        let nz = InstKind::BinOp(BinOp::Xor, val(4), Operand::ConstI64(1));
        assert!(XorZero.try_apply(&nz).is_none());
    }

    #[test]
    fn or_zero_rewrites_to_identity_rhs_only_after_canon() {
        let lhs = InstKind::BinOp(BinOp::Or, val(4), Operand::ConstI64(0));
        assert_eq!(
            OrZero.try_apply(&lhs),
            Some(InstKind::Identity(Operand::Value(ValueId(4))))
        );
        let lhs2 = InstKind::BinOp(BinOp::Or, Operand::ConstI64(0), val(7));
        assert!(OrZero.try_apply(&lhs2).is_none());
    }

    #[test]
    fn and_all_ones_rewrites_to_identity_rhs_only_after_canon() {
        let lhs = InstKind::BinOp(BinOp::And, val(4), Operand::ConstI64(-1));
        assert_eq!(
            AndAllOnes.try_apply(&lhs),
            Some(InstKind::Identity(Operand::Value(ValueId(4))))
        );
        // LHS-Const direct-call no longer fires; canon swaps first.
        let lhs2 = InstKind::BinOp(BinOp::And, Operand::ConstI64(-1), val(7));
        assert!(AndAllOnes.try_apply(&lhs2).is_none());
        // `and x 0` is AndZero territory, not AndAllOnes.
        let zero = InstKind::BinOp(BinOp::And, val(4), Operand::ConstI64(0));
        assert!(AndAllOnes.try_apply(&zero).is_none());
    }

    #[test]
    fn and_zero_folds_to_zero_rhs_only_after_canon() {
        let lhs = InstKind::BinOp(BinOp::And, val(4), Operand::ConstI64(0));
        assert_eq!(
            AndZero.try_apply(&lhs),
            Some(InstKind::Identity(Operand::ConstI64(0)))
        );
        let lhs2 = InstKind::BinOp(BinOp::And, Operand::ConstI64(0), val(7));
        assert!(AndZero.try_apply(&lhs2).is_none());
        // `and x -1` is AndAllOnes territory.
        let neg1 = InstKind::BinOp(BinOp::And, val(4), Operand::ConstI64(-1));
        assert!(AndZero.try_apply(&neg1).is_none());
    }

    #[test]
    fn or_all_ones_folds_to_all_ones_rhs_only_after_canon() {
        let lhs = InstKind::BinOp(BinOp::Or, val(4), Operand::ConstI64(-1));
        assert_eq!(
            OrAllOnes.try_apply(&lhs),
            Some(InstKind::Identity(Operand::ConstI64(-1)))
        );
        let lhs2 = InstKind::BinOp(BinOp::Or, Operand::ConstI64(-1), val(7));
        assert!(OrAllOnes.try_apply(&lhs2).is_none());
        // `or x 0` is OrZero territory.
        let zero = InstKind::BinOp(BinOp::Or, val(4), Operand::ConstI64(0));
        assert!(OrAllOnes.try_apply(&zero).is_none());
    }
}
