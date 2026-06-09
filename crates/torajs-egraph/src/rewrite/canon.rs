//! Commutative canonicalisation — Phase 2 chunk 10a.
//!
//! Strictly-directed single-shot swap. For the five commutative
//! integer binops (Add/Mul/And/Or/Xor), if LHS is ConstI64 and RHS
//! is not ConstI64, swap so the const lands on the right. The
//! "RHS is not Const" guard makes the rule self-disarm after one
//! fire — no commutativity explosion. Two-const cases (e.g.
//! `add 3 5`) are not rewritten here; a future const-fold rule
//! computes them.
//!
//! Floating-point FAdd/FMul are excluded — IEEE 754 commutativity
//! is non-strict around NaN sign-bit propagation; revisit under a
//! fast-math flag.
//!
//! Sequenced **first** in `BUILTIN_RULES` so the identity /
//! const-fold rules see normalised input.

use crate::rewrite::Rewrite;
use torajs_core::ssa::{BinOp, InstKind, Operand};

/// Canonicalise commutative integer binops by moving any ConstI64
/// operand to the RHS.
pub struct CommutativeCanon;

impl Rewrite for CommutativeCanon {
    fn name(&self) -> &'static str {
        "commutative_canon"
    }
    fn try_apply(&self, lhs: &InstKind) -> Option<InstKind> {
        if let InstKind::BinOp(op, a, b) = lhs
            && matches!(
                op,
                BinOp::Add | BinOp::Mul | BinOp::And | BinOp::Or | BinOp::Xor
            )
            && matches!(a, Operand::ConstI64(_))
            && !matches!(b, Operand::ConstI64(_))
        {
            return Some(InstKind::BinOp(*op, *b, *a));
        }
        None
    }
}

pub(crate) static COMMUTATIVE_CANON: CommutativeCanon = CommutativeCanon;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rewrite::apply_rewrites_recursive;
    use torajs_core::ssa::ValueId;

    fn val(n: u32) -> Operand {
        Operand::Value(ValueId(n))
    }

    #[test]
    fn commutative_canon_swaps_const_to_rhs_for_all_five_ops() {
        for op in [BinOp::Add, BinOp::Mul, BinOp::And, BinOp::Or, BinOp::Xor] {
            let lhs = InstKind::BinOp(op, Operand::ConstI64(7), val(4));
            let rhs = CommutativeCanon
                .try_apply(&lhs)
                .unwrap_or_else(|| panic!("CommutativeCanon must fire on {op:?}"));
            assert_eq!(rhs, InstKind::BinOp(op, val(4), Operand::ConstI64(7)));
        }
    }

    #[test]
    fn commutative_canon_already_canonical_no_change() {
        // const already on RHS — rule must not fire.
        let lhs = InstKind::BinOp(BinOp::Add, val(4), Operand::ConstI64(7));
        assert_eq!(CommutativeCanon.try_apply(&lhs), None);
    }

    #[test]
    fn commutative_canon_skips_non_commutative_ops() {
        // Sub/SDiv/Shl etc. must NOT swap even when LHS is const.
        for op in [BinOp::Sub, BinOp::SDiv, BinOp::Shl, BinOp::AShr] {
            let lhs = InstKind::BinOp(op, Operand::ConstI64(7), val(4));
            assert_eq!(
                CommutativeCanon.try_apply(&lhs),
                None,
                "must not swap {op:?}"
            );
        }
    }

    #[test]
    fn commutative_canon_skips_two_const_operands() {
        // `add 3 5` is a const-fold candidate, not a canon candidate.
        let lhs = InstKind::BinOp(BinOp::Add, Operand::ConstI64(3), Operand::ConstI64(5));
        assert_eq!(CommutativeCanon.try_apply(&lhs), None);
    }

    #[test]
    fn commutative_canon_skips_two_value_operands() {
        // Both Value — no const to canonicalise; rule must not fire.
        let lhs = InstKind::BinOp(BinOp::Add, val(4), val(9));
        assert_eq!(CommutativeCanon.try_apply(&lhs), None);
    }

    #[test]
    fn commutative_canon_self_disarms_on_recursive_apply() {
        // apply_rewrites_recursive must reach a fixed point in exactly
        // one step — no swap/swap/swap loop within REWRITE_LIMIT.
        let lhs = InstKind::BinOp(BinOp::Mul, Operand::ConstI64(5), val(4));
        let canon = vec![&COMMUTATIVE_CANON as &dyn Rewrite];
        let out = apply_rewrites_recursive(&canon, &lhs);
        assert_eq!(
            out,
            InstKind::BinOp(BinOp::Mul, val(4), Operand::ConstI64(5))
        );
    }
}
