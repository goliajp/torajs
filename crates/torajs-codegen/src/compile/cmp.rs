//! ICmp + FCmp lowering — CMP/FCMP set NZCV, CSET converts to 0/1.

use torajs_core::ssa::{FPred, IPred, Inst, Operand};

use super::operand::{materialize_operand_fpr, materialize_operand_gpr};
use super::write_u32;
use super::{FP_SCRATCH_LHS, FP_SCRATCH_RHS, OP_SCRATCH_LHS, OP_SCRATCH_RHS};
use crate::enc::{cmp_reg, cond, cset_cond, fcmp_d};
use crate::regalloc::Assignment;

pub fn emit_icmp(
    bytes: &mut Vec<u8>,
    inst: &Inst,
    pred: IPred,
    lhs: &Operand,
    rhs: &Operand,
    alloc: &Assignment,
) {
    let dst = inst
        .result
        .map(|v| alloc.of(v).as_gpr())
        .expect("ICmp must have a result");
    let rn = materialize_operand_gpr(bytes, lhs, OP_SCRATCH_LHS, alloc);
    let rm = materialize_operand_gpr(bytes, rhs, OP_SCRATCH_RHS, alloc);
    write_u32(bytes, cmp_reg(rn, rm));
    write_u32(bytes, cset_cond(dst, ipred_to_cond(pred)));
}

pub fn emit_fcmp(
    bytes: &mut Vec<u8>,
    inst: &Inst,
    pred: FPred,
    lhs: &Operand,
    rhs: &Operand,
    alloc: &Assignment,
) {
    let dst = inst
        .result
        .map(|v| alloc.of(v).as_gpr())
        .expect("FCmp must have a result");
    let rn = materialize_operand_fpr(bytes, lhs, FP_SCRATCH_LHS, OP_SCRATCH_LHS, alloc);
    let rm = materialize_operand_fpr(bytes, rhs, FP_SCRATCH_RHS, OP_SCRATCH_RHS, alloc);
    write_u32(bytes, fcmp_d(rn, rm));
    write_u32(bytes, cset_cond(dst, fpred_to_cond(pred)));
}

/// Map an SSA integer predicate to the aarch64 condition code that
/// makes CSET produce 1 when the comparison holds (after CMP set
/// NZCV).
pub fn ipred_to_cond(p: IPred) -> u8 {
    match p {
        IPred::Eq => cond::EQ,
        IPred::Ne => cond::NE,
        IPred::Slt => cond::LT,
        IPred::Sgt => cond::GT,
        IPred::Sle => cond::LE,
        IPred::Sge => cond::GE,
    }
}

/// Map an SSA floating-point predicate to an aarch64 condition code.
/// Each choice was picked so that the unordered NZCV pattern
/// (N=0,Z=0,C=1,V=1) — what FCMP sets when either operand is NaN —
/// yields 0 for the ordered (`O*`) predicates and 1 for `Une`.
///
/// Mapping rationale (per ARM ARM "Floating-point conditions" table):
///   Oeq → EQ  (Z=1; unordered Z=0 → false)
///   Olt → MI  (N=1; unordered N=0 → false)
///   Ogt → GT  (Z=0 ∧ N=V; unordered N≠V → false)
///   Ole → LS  (C=0 ∨ Z=1; unordered C=1,Z=0 → false)
///   Oge → GE  (N=V; unordered N=0,V=1 → false)
///   Une → NE  (Z=0; ordered-equal Z=1 → false)
///
/// `FPred::One` (ordered ≠) needs `Z=0 ∧ V=0` which is not a single
/// cond — typically lowered as CSET-NE + CSET-VC + AND or via CCMP.
/// Routed to `todo!("S2-D")` for now.
pub fn fpred_to_cond(p: FPred) -> u8 {
    match p {
        FPred::Oeq => cond::EQ,
        FPred::Olt => cond::MI,
        FPred::Ogt => cond::GT,
        FPred::Ole => cond::LS,
        FPred::Oge => cond::GE,
        FPred::Une => cond::NE,
        FPred::One => todo!("S2-D: FPred::One (ordered ≠) needs CCMP fold or NE+VC+AND"),
    }
}

#[cfg(test)]
mod tests {
    use super::super::compile_function;
    use super::super::test_fixtures::{build_fcmp_const, build_icmp_const, words_to_le_bytes};
    use super::*;
    use crate::enc;
    use crate::reg::{Fpr, Gpr};

    fn assert_icmp_4word(name: &str, pred: IPred, lhs: i64, rhs: i64, expected_cond: u8) {
        let func = build_icmp_const(name, pred, lhs, rhs);
        let compiled = compile_function(&func);
        let expected = words_to_le_bytes(&[
            enc::movz_imm(Gpr::X9, lhs as u16, 0),
            enc::movz_imm(Gpr::X10, rhs as u16, 0),
            enc::cmp_reg(Gpr::X9, Gpr::X10),
            enc::cset_cond(Gpr::X0, expected_cond),
            enc::ret(Gpr::X30),
        ]);
        assert_eq!(
            compiled.bytes, expected,
            "{name}: expected {expected:02X?}, got {:02X?}",
            compiled.bytes
        );
    }

    #[test]
    fn icmp_eq_five_equals_five() {
        assert_icmp_4word("icmp_eq_5_5", IPred::Eq, 5, 5, cond::EQ);
    }

    #[test]
    fn icmp_slt_five_lt_three() {
        assert_icmp_4word("icmp_slt_5_3", IPred::Slt, 5, 3, cond::LT);
    }

    #[test]
    fn icmp_sge_three_ge_five() {
        assert_icmp_4word("icmp_sge_3_5", IPred::Sge, 3, 5, cond::GE);
    }

    #[test]
    fn icmp_sle_two_le_two() {
        assert_icmp_4word("icmp_sle_2_2", IPred::Sle, 2, 2, cond::LE);
    }

    #[test]
    fn ipred_mapping_covers_all_six_variants() {
        assert_eq!(ipred_to_cond(IPred::Eq), cond::EQ);
        assert_eq!(ipred_to_cond(IPred::Ne), cond::NE);
        assert_eq!(ipred_to_cond(IPred::Slt), cond::LT);
        assert_eq!(ipred_to_cond(IPred::Sgt), cond::GT);
        assert_eq!(ipred_to_cond(IPred::Sle), cond::LE);
        assert_eq!(ipred_to_cond(IPred::Sge), cond::GE);
    }

    /// `lhs_q3` / `rhs_q3` are the upper 16 bits of `to_bits()` for
    /// fp consts where only that quadrant is non-zero (covers the
    /// 2.0, 2.5, 3.0 family).
    fn assert_fcmp_q3_only(
        name: &str,
        pred: FPred,
        lhs: f64,
        rhs: f64,
        lhs_q3: u16,
        rhs_q3: u16,
        expected_cond: u8,
    ) {
        let func = build_fcmp_const(name, pred, lhs, rhs);
        let compiled = compile_function(&func);
        let expected = words_to_le_bytes(&[
            enc::movz_imm(Gpr::X9, 0, 0),
            enc::movk_imm(Gpr::X9, lhs_q3, 3),
            enc::fmov_d_from_x(Fpr::V16, Gpr::X9),
            enc::movz_imm(Gpr::X10, 0, 0),
            enc::movk_imm(Gpr::X10, rhs_q3, 3),
            enc::fmov_d_from_x(Fpr::V17, Gpr::X10),
            enc::fcmp_d(Fpr::V16, Fpr::V17),
            enc::cset_cond(Gpr::X0, expected_cond),
            enc::ret(Gpr::X30),
        ]);
        assert_eq!(
            compiled.bytes, expected,
            "{name}: expected {expected:02X?}, got {:02X?}",
            compiled.bytes
        );
    }

    #[test]
    fn fcmp_oeq_two_equals_two() {
        assert_fcmp_q3_only(
            "fcmp_oeq_2_2",
            FPred::Oeq,
            2.0,
            2.0,
            0x4000,
            0x4000,
            cond::EQ,
        );
    }

    #[test]
    fn fcmp_olt_two_lt_three() {
        assert_fcmp_q3_only(
            "fcmp_olt_2_3",
            FPred::Olt,
            2.0,
            3.0,
            0x4000,
            0x4008,
            cond::MI,
        );
    }

    #[test]
    fn fcmp_ogt_three_gt_two() {
        assert_fcmp_q3_only(
            "fcmp_ogt_3_2",
            FPred::Ogt,
            3.0,
            2.0,
            0x4008,
            0x4000,
            cond::GT,
        );
    }

    #[test]
    fn fcmp_ole_two_le_two() {
        assert_fcmp_q3_only(
            "fcmp_ole_2_2",
            FPred::Ole,
            2.0,
            2.0,
            0x4000,
            0x4000,
            cond::LS,
        );
    }

    #[test]
    fn fpred_mapping_covers_six_ordered_plus_une() {
        assert_eq!(fpred_to_cond(FPred::Oeq), cond::EQ);
        assert_eq!(fpred_to_cond(FPred::Olt), cond::MI);
        assert_eq!(fpred_to_cond(FPred::Ogt), cond::GT);
        assert_eq!(fpred_to_cond(FPred::Ole), cond::LS);
        assert_eq!(fpred_to_cond(FPred::Oge), cond::GE);
        assert_eq!(fpred_to_cond(FPred::Une), cond::NE);
        // FPred::One is `todo!("S2-D")` and not exercised here.
    }
}
