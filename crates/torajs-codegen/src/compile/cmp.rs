//! ICmp + FCmp lowering — CMP/FCMP set NZCV, CSET converts to 0/1.

use torajs_core::ssa::{FPred, IPred, Inst, Operand};

use super::operand::{materialize_operand_fpr, materialize_operand_gpr};
use super::{
    FP_SCRATCH_LHS, FP_SCRATCH_RHS, OP_SCRATCH_LHS, OP_SCRATCH_RESULT_GPR, OP_SCRATCH_RHS,
    OP_SCRATCH_TMP, write_def_spill_gpr, write_u32,
};
use crate::enc::{cmp_imm, cmp_reg, cmp_w_reg, cond, cset_cond, fcmp_d, orr_reg};
use crate::regalloc::Assignment;

pub fn emit_icmp(
    bytes: &mut Vec<u8>,
    inst: &Inst,
    pred: IPred,
    lhs: &Operand,
    rhs: &Operand,
    alloc: &Assignment,
) {
    let result_vid = inst.result.expect("ICmp must have a result");
    let (dst, spill_off) = alloc.def_gpr(result_vid, OP_SCRATCH_RESULT_GPR);
    let rn = materialize_operand_gpr(bytes, lhs, OP_SCRATCH_LHS, alloc);
    // Round 5 popcount branch/const attack — `icmp x, #imm12` uses
    // the CMP-immediate alias, skipping the MOVZ rematerialization
    // of the constant into a scratch register on every compare
    // (loop headers pay this once per iteration).
    if let Operand::ConstI64(c) = rhs
        && (0..4096).contains(c)
    {
        write_u32(bytes, cmp_imm(rn, *c as u16));
        write_u32(bytes, cset_cond(dst, ipred_to_cond(pred)));
        write_def_spill_gpr(bytes, spill_off, dst);
        return;
    }
    let rm = materialize_operand_gpr(bytes, rhs, OP_SCRATCH_RHS, alloc);
    // I32-typed ICmp (e.g. refcount equality in emit_rc_dec_inline's
    // `ICmp(Eq, rc_new: I32, ConstI32(0))`) must compare only the
    // low 32 bits — the 64-bit X-reg slot's high half holds the
    // adjacent heap header u32 (type_tag/flags), which a 64-bit
    // cmp would conflate into NZCV and misroute the
    // walk-vs-cycle-buffer branch. Detect via a ConstI32 operand on
    // either side; pure I64 ICmp stays on the X-form.
    let is_i32 = matches!(lhs, Operand::ConstI32(_)) || matches!(rhs, Operand::ConstI32(_));
    if is_i32 {
        write_u32(bytes, cmp_w_reg(rn, rm));
    } else {
        write_u32(bytes, cmp_reg(rn, rm));
    }
    write_u32(bytes, cset_cond(dst, ipred_to_cond(pred)));
    write_def_spill_gpr(bytes, spill_off, dst);
}

pub fn emit_fcmp(
    bytes: &mut Vec<u8>,
    inst: &Inst,
    pred: FPred,
    lhs: &Operand,
    rhs: &Operand,
    alloc: &Assignment,
) {
    let result_vid = inst.result.expect("FCmp must have a result");
    let (dst, spill_off) = alloc.def_gpr(result_vid, OP_SCRATCH_RESULT_GPR);
    let rn = materialize_operand_fpr(bytes, lhs, FP_SCRATCH_LHS, OP_SCRATCH_LHS, alloc);
    let rm = materialize_operand_fpr(bytes, rhs, FP_SCRATCH_RHS, OP_SCRATCH_RHS, alloc);
    write_u32(bytes, fcmp_d(rn, rm));
    match pred {
        FPred::One => {
            // Ordered ≠ = `MI ∨ GT` (lt or gt), NaN → false.
            //
            // FCMP sets NZCV per ARM ARM "Floating-point conditions":
            //   ordered lt → N=1, Z=0, V=0
            //   ordered gt → N=0, Z=0, C=1, V=0  (then N==V via N=0,V=0)
            //   ordered eq → Z=1, V=0
            //   unordered  → N=0, Z=0, C=1, V=1
            //
            // CSET MI checks N=1 → only ordered lt.
            // CSET GT checks Z=0 ∧ N==V → ordered gt (rejects unordered
            // because there N=0 ≠ V=1) and rejects eq (Z=1).
            // ORR yields 1 iff ordered ne, 0 otherwise — same 3-inst
            // form clang -O2 emits for `fcmp one`.
            write_u32(bytes, cset_cond(OP_SCRATCH_TMP, cond::MI));
            write_u32(bytes, cset_cond(dst, cond::GT));
            write_u32(bytes, orr_reg(dst, dst, OP_SCRATCH_TMP));
        }
        _ => {
            write_u32(bytes, cset_cond(dst, fpred_to_cond(pred)));
        }
    }
    write_def_spill_gpr(bytes, spill_off, dst);
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
/// `FPred::One` (ordered ≠) is not a single-cond predicate — it
/// expands to CSET MI + CSET GT + ORR inline inside `emit_fcmp` and
/// never reaches this single-cond mapping table.
pub fn fpred_to_cond(p: FPred) -> u8 {
    match p {
        FPred::Oeq => cond::EQ,
        FPred::Olt => cond::MI,
        FPred::Ogt => cond::GT,
        FPred::Ole => cond::LS,
        FPred::Oge => cond::GE,
        FPred::Une => cond::NE,
        FPred::One => {
            unreachable!("FPred::One has its own 3-inst sequence in emit_fcmp")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::compile_function;
    use super::super::test_fixtures::{build_fcmp_const, build_icmp_const, words_to_le_bytes};
    use super::*;
    use crate::enc;
    use crate::reg::{Fpr, Gpr};

    /// Small-const RHS ICmps take the CMP-immediate form (Round 5
    /// popcount branch/const attack) — the constant never touches a
    /// scratch register.
    fn assert_icmp_4word(name: &str, pred: IPred, lhs: i64, rhs: i64, expected_cond: u8) {
        let func = build_icmp_const(name, pred, lhs, rhs);
        let compiled = compile_function(&func);
        let expected = words_to_le_bytes(&[
            enc::movz_imm(Gpr::X9, lhs as u16, 0),
            enc::cmp_imm(Gpr::X9, rhs as u16),
            enc::cset_cond(Gpr::X0, expected_cond),
            enc::ret(Gpr::X30),
        ]);
        assert_eq!(
            compiled.bytes, expected,
            "{name}: expected {expected:02X?}, got {:02X?}",
            compiled.bytes
        );
    }

    /// A >= 4096 const RHS keeps the register CMP form.
    #[test]
    fn icmp_large_const_keeps_reg_form() {
        let func = build_icmp_const("icmp_eq_5_5000", IPred::Eq, 5, 5000);
        let compiled = compile_function(&func);
        let expected = words_to_le_bytes(&[
            enc::movz_imm(Gpr::X9, 5, 0),
            enc::movz_imm(Gpr::X10, 5000, 0),
            enc::cmp_reg(Gpr::X9, Gpr::X10),
            enc::cset_cond(Gpr::X0, cond::EQ),
            enc::ret(Gpr::X30),
        ]);
        assert_eq!(
            compiled.bytes, expected,
            "icmp-large-const: expected {expected:02X?}, got {:02X?}",
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
        // FPred::One is dispatched inline by `emit_fcmp` and is not in
        // the single-cond table — calling `fpred_to_cond(One)` panics.
    }

    #[test]
    #[should_panic(expected = "FPred::One has its own 3-inst sequence")]
    fn fpred_to_cond_panics_on_one() {
        let _ = fpred_to_cond(FPred::One);
    }

    /// `fcmp_one_two_ne_three`: lhs=2.0, rhs=3.0 → ordered ≠ → true (1).
    ///
    /// Expected 10-instruction body (consts use MOVZ-quadrant-3
    /// MOVK chain → FMOV-d-from-x, same shape as the other FCmp tests
    /// in this file):
    ///
    /// ```text
    ///   MOVZ x9, #0          (lhs lo quadrant)
    ///   MOVK x9, #0x4000, lsl #48
    ///   FMOV d16, x9
    ///   MOVZ x10, #0         (rhs lo quadrant)
    ///   MOVK x10, #0x4008, lsl #48
    ///   FMOV d17, x10
    ///   FCMP d16, d17
    ///   CSET x11, MI         (lt → 1 only on ordered-lt)
    ///   CSET x0,  GT         (gt → 1 only on ordered-gt)
    ///   ORR  x0, x0, x11     (final: 1 iff ordered ne)
    ///   RET
    /// ```
    #[test]
    fn fcmp_one_two_ne_three_byte_equal() {
        let func = build_fcmp_const("fcmp_one_2_3", FPred::One, 2.0, 3.0);
        let compiled = compile_function(&func);
        let expected = words_to_le_bytes(&[
            enc::movz_imm(Gpr::X9, 0, 0),
            enc::movk_imm(Gpr::X9, 0x4000, 3),
            enc::fmov_d_from_x(Fpr::V16, Gpr::X9),
            enc::movz_imm(Gpr::X10, 0, 0),
            enc::movk_imm(Gpr::X10, 0x4008, 3),
            enc::fmov_d_from_x(Fpr::V17, Gpr::X10),
            enc::fcmp_d(Fpr::V16, Fpr::V17),
            enc::cset_cond(Gpr::X11, cond::MI),
            enc::cset_cond(Gpr::X0, cond::GT),
            enc::orr_reg(Gpr::X0, Gpr::X0, Gpr::X11),
            enc::ret(Gpr::X30),
        ]);
        assert_eq!(
            compiled.bytes, expected,
            "fcmp_one_2_3: expected {expected:02X?}, got {:02X?}",
            compiled.bytes
        );
    }
}
