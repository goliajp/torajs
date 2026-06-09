//! Strength-reduction rules — Phase 0 demo + future expansion site.
//!
//! - `(mul x 2^k) → (shl x k)` — power-of-two strength reduction.
//!
//! The Phase 0 demo rule; each `Rewrite` impl is a small struct so
//! the registry can hold `&'static dyn Rewrite`. Match patterns are
//! by-value on `InstKind` (Operand derives PartialEq + Hash;
//! integer literals compare by exact value).

use crate::rewrite::Rewrite;
use torajs_core::ssa::{BinOp, InstKind, Operand};

/// `(mul x 2^k) → (shl x k)` — strength reduction. Fires for
/// power-of-two right-hand constants only (left-hand version
/// `(mul 2^k x)` first canonicalises through `CommutativeCanon`).
///
/// Excludes c==1 (shl by 0 is a no-op — `MulOne` handles that as
/// an identity rewrite instead) so the rule registry's
/// identity-first priority doesn't get racy.
pub struct MulPow2ToShl;

impl Rewrite for MulPow2ToShl {
    fn name(&self) -> &'static str {
        "mul_pow2_to_shl"
    }
    fn try_apply(&self, lhs: &InstKind) -> Option<InstKind> {
        if let InstKind::BinOp(BinOp::Mul, x, Operand::ConstI64(c)) = lhs
            && *c > 1
            && (*c as u64).is_power_of_two()
        {
            let k = (*c as u64).trailing_zeros() as i64;
            return Some(InstKind::BinOp(BinOp::Shl, *x, Operand::ConstI64(k)));
        }
        None
    }
}

pub(crate) static MUL_POW2: MulPow2ToShl = MulPow2ToShl;

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::ValueId;

    fn val(n: u32) -> Operand {
        Operand::Value(ValueId(n))
    }

    #[test]
    fn mul_pow2_strength_reduces() {
        // mul %v 8 → shl %v 3
        let lhs = InstKind::BinOp(BinOp::Mul, val(4), Operand::ConstI64(8));
        let rhs = MulPow2ToShl.try_apply(&lhs).expect("must fire on 2^3");
        assert_eq!(
            rhs,
            InstKind::BinOp(BinOp::Shl, val(4), Operand::ConstI64(3))
        );
        // mul %v 1024 → shl %v 10
        let lhs2 = InstKind::BinOp(BinOp::Mul, val(4), Operand::ConstI64(1024));
        let rhs2 = MulPow2ToShl.try_apply(&lhs2).expect("must fire on 2^10");
        assert_eq!(
            rhs2,
            InstKind::BinOp(BinOp::Shl, val(4), Operand::ConstI64(10))
        );
        // Non-power-of-two doesn't fire.
        let lhs3 = InstKind::BinOp(BinOp::Mul, val(4), Operand::ConstI64(3));
        assert!(MulPow2ToShl.try_apply(&lhs3).is_none());
        // Zero doesn't fire (we require c > 0; mul-zero rule lands
        // in Phase 1 alongside InstKind::Identity).
        let lhs_zero = InstKind::BinOp(BinOp::Mul, val(4), Operand::ConstI64(0));
        assert!(MulPow2ToShl.try_apply(&lhs_zero).is_none());
        // Negative constant — also doesn't fire (sign isn't a
        // power-of-two by the u64-cast check; semantic mul by -8
        // would need sub/neg combination).
        let lhs_neg = InstKind::BinOp(BinOp::Mul, val(4), Operand::ConstI64(-8));
        assert!(MulPow2ToShl.try_apply(&lhs_neg).is_none());
        // Sub doesn't match the mul-pow2 rule.
        let sub = InstKind::BinOp(BinOp::Sub, val(4), Operand::ConstI64(8));
        assert!(MulPow2ToShl.try_apply(&sub).is_none());
    }
}
