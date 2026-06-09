//! Self-fold const-fold rules — Phase 1 chunk 9a-3.
//!
//! The RHS forms are `Identity(ConstI64(0))`; optimize.rs detects
//! the const operand and calls `set_const` on the eclass root, so
//! downstream `map_operand` propagates `0` into every use. No new
//! `InstKind` variant or codegen path is needed.
//!
//! - `(sub x x) → 0` — self-subtraction identity.
//! - `(xor x x) → 0` — self-xor identity.

use crate::rewrite::Rewrite;
use torajs_core::ssa::{BinOp, InstKind, Operand};

/// `(sub x x) → 0` — self-subtraction identity.
pub struct SubSelf;

impl Rewrite for SubSelf {
    fn name(&self) -> &'static str {
        "sub_self_to_zero"
    }
    fn try_apply(&self, lhs: &InstKind) -> Option<InstKind> {
        if let InstKind::BinOp(BinOp::Sub, a, b) = lhs
            && a == b
        {
            return Some(InstKind::Identity(Operand::ConstI64(0)));
        }
        None
    }
}

/// `(xor x x) → 0` — self-xor identity.
pub struct XorSelf;

impl Rewrite for XorSelf {
    fn name(&self) -> &'static str {
        "xor_self_to_zero"
    }
    fn try_apply(&self, lhs: &InstKind) -> Option<InstKind> {
        if let InstKind::BinOp(BinOp::Xor, a, b) = lhs
            && a == b
        {
            return Some(InstKind::Identity(Operand::ConstI64(0)));
        }
        None
    }
}

pub(crate) static SUB_SELF: SubSelf = SubSelf;
pub(crate) static XOR_SELF: XorSelf = XorSelf;

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::ValueId;

    fn val(n: u32) -> Operand {
        Operand::Value(ValueId(n))
    }

    #[test]
    fn sub_self_folds_to_const_zero() {
        let lhs = InstKind::BinOp(BinOp::Sub, val(4), val(4));
        assert_eq!(
            SubSelf.try_apply(&lhs),
            Some(InstKind::Identity(Operand::ConstI64(0)))
        );
        // sub %a %b where a != b — no fire.
        let mixed = InstKind::BinOp(BinOp::Sub, val(4), val(5));
        assert!(SubSelf.try_apply(&mixed).is_none());
        // sub const const equal — also fires (constants compare by value).
        let const_eq = InstKind::BinOp(BinOp::Sub, Operand::ConstI64(7), Operand::ConstI64(7));
        assert_eq!(
            SubSelf.try_apply(&const_eq),
            Some(InstKind::Identity(Operand::ConstI64(0)))
        );
    }

    #[test]
    fn xor_self_folds_to_const_zero() {
        let lhs = InstKind::BinOp(BinOp::Xor, val(4), val(4));
        assert_eq!(
            XorSelf.try_apply(&lhs),
            Some(InstKind::Identity(Operand::ConstI64(0)))
        );
        let mixed = InstKind::BinOp(BinOp::Xor, val(4), val(5));
        assert!(XorSelf.try_apply(&mixed).is_none());
        // wrong op doesn't fire.
        let sub = InstKind::BinOp(BinOp::Sub, val(4), val(4));
        assert!(XorSelf.try_apply(&sub).is_none());
    }
}
