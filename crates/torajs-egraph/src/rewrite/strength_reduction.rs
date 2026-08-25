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

/// `(fmul 2.0 x)` / `(fmul x 2.0)` → `(fadd x x)` — float doubling
/// strength reduction (mandelbrot decomposition D4).
///
/// Exact for every f64 input: multiplication by 2.0 only bumps the
/// exponent (no rounding, denormals included), and `x + x` computes
/// the identical value — signed zeros ((-0)+(-0) = -0 = (-0)*2.0),
/// infinities, and NaN (both sides yield NaN) all agree. Both operand
/// orders match here because FAdd/FMul are excluded from
/// `CommutativeCanon` (canon.rs keeps FP operand order), so a source
/// `2 * zr` keeps its constant on the left.
///
/// The profit is not the multiply itself (fadd and fmul cost the
/// same) but the 2.0 operand: codegen materializes an FP constant at
/// every use (3-inst mov/movk/fmov + a GPR→FPR domain crossing —
/// decomposition D1), and rewriting to `x + x` deletes the operand.
pub struct FMulTwoToAdd;

impl Rewrite for FMulTwoToAdd {
    fn name(&self) -> &'static str {
        "fmul_two_to_add"
    }
    fn try_apply(&self, lhs: &InstKind) -> Option<InstKind> {
        match lhs {
            InstKind::BinOp(BinOp::FMul, x, Operand::ConstF64(c))
            | InstKind::BinOp(BinOp::FMul, Operand::ConstF64(c), x)
                if *c == 2.0 =>
            {
                Some(InstKind::BinOp(BinOp::FAdd, *x, *x))
            }
            _ => None,
        }
    }
}

pub(crate) static FMUL_TWO: FMulTwoToAdd = FMulTwoToAdd;

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::ValueId;

    fn val(n: u32) -> Operand {
        Operand::Value(ValueId(n))
    }

    #[test]
    fn fmul_two_strength_reduces() {
        // 2.0 * %v → %v + %v (constant on the left — no FP canon).
        let lhs = InstKind::BinOp(BinOp::FMul, Operand::ConstF64(2.0), val(4));
        assert_eq!(
            FMulTwoToAdd.try_apply(&lhs),
            Some(InstKind::BinOp(BinOp::FAdd, val(4), val(4)))
        );
        // %v * 2.0 → %v + %v (constant on the right).
        let rhs_side = InstKind::BinOp(BinOp::FMul, val(4), Operand::ConstF64(2.0));
        assert_eq!(
            FMulTwoToAdd.try_apply(&rhs_side),
            Some(InstKind::BinOp(BinOp::FAdd, val(4), val(4)))
        );
        // Other constants / negated doubling don't fire.
        for c in [4.0, -2.0, 0.5, f64::NAN] {
            let l = InstKind::BinOp(BinOp::FMul, val(4), Operand::ConstF64(c));
            assert!(FMulTwoToAdd.try_apply(&l).is_none());
        }
        // Integer Mul doesn't match the float rule.
        let int_mul = InstKind::BinOp(BinOp::Mul, val(4), Operand::ConstI64(2));
        assert!(FMulTwoToAdd.try_apply(&int_mul).is_none());
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
