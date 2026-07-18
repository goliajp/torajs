//! ICmp + FCmp lowering — CMP/FCMP set NZCV, CSET converts to 0/1.

use torajs_core::ssa::{FPred, IPred, Inst, Operand, Type};

use super::operand::{materialize_operand_fpr, materialize_operand_gpr};
use super::{
    FP_SCRATCH_LHS, FP_SCRATCH_RESULT, FP_SCRATCH_RHS, OP_SCRATCH_LHS, OP_SCRATCH_RESULT_GPR,
    OP_SCRATCH_RHS, OP_SCRATCH_TMP, write_def_spill_fpr, write_def_spill_gpr, write_u32,
};
use crate::enc::{
    cmp_imm, cmp_reg, cmp_w_reg, cond, csel_cond, cset_cond, fcmp_d, fcsel_d, orr_reg,
};
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
    emit_compare_nzcv(bytes, lhs, rhs, alloc);
    write_u32(bytes, cset_cond(dst, ipred_to_cond(pred)));
    write_def_spill_gpr(bytes, spill_off, dst);
}

/// The CMP half of an integer compare — set NZCV, no CSET. Shared by
/// `emit_icmp` and the branch-fuse CondBr path (compile/brfuse.rs).
pub(crate) fn emit_compare_nzcv(
    bytes: &mut Vec<u8>,
    lhs: &Operand,
    rhs: &Operand,
    alloc: &Assignment,
) {
    let rn = materialize_operand_gpr(bytes, lhs, OP_SCRATCH_LHS, alloc);
    // Round 5 popcount branch/const attack — `icmp x, #imm12` uses
    // the CMP-immediate alias, skipping the MOVZ rematerialization
    // of the constant into a scratch register on every compare
    // (loop headers pay this once per iteration).
    if let Operand::ConstI64(c) = rhs
        && (0..4096).contains(c)
    {
        write_u32(bytes, cmp_imm(rn, *c as u16));
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
}

/// `InstKind::Select(ty, cond, then, else)` — branchless conditional
/// move. Materialize all three operands first (mov/movz/ldr reloads
/// never touch NZCV), then `cmp cond, #0; csel dst, then, else, NE`.
/// An F64 result takes the FCSEL twin: register class follows the
/// *value's* declared type (regalloc.rs:256), so the condition still
/// rides a GPR and only the value arms live in FPRs.
pub fn emit_select(
    bytes: &mut Vec<u8>,
    inst: &Inst,
    ty: &Type,
    cond_op: &Operand,
    then_op: &Operand,
    else_op: &Operand,
    alloc: &Assignment,
) {
    let result_vid = inst.result.expect("Select must have a result");
    if *ty == Type::F64 {
        let (dst, spill_off) = alloc.def_fpr(result_vid, FP_SCRATCH_RESULT);
        let rn = materialize_operand_fpr(bytes, then_op, FP_SCRATCH_LHS, OP_SCRATCH_LHS, alloc);
        let rm = materialize_operand_fpr(bytes, else_op, FP_SCRATCH_RHS, OP_SCRATCH_RHS, alloc);
        let rc = materialize_operand_gpr(bytes, cond_op, OP_SCRATCH_TMP, alloc);
        write_u32(bytes, cmp_imm(rc, 0));
        write_u32(bytes, fcsel_d(dst, rn, rm, cond::NE));
        write_def_spill_fpr(bytes, spill_off, dst);
        return;
    }
    let (dst, spill_off) = alloc.def_gpr(result_vid, OP_SCRATCH_RESULT_GPR);
    let rn = materialize_operand_gpr(bytes, then_op, OP_SCRATCH_LHS, alloc);
    let rm = materialize_operand_gpr(bytes, else_op, OP_SCRATCH_RHS, alloc);
    let rc = materialize_operand_gpr(bytes, cond_op, OP_SCRATCH_TMP, alloc);
    write_u32(bytes, cmp_imm(rc, 0));
    write_u32(bytes, csel_cond(dst, rn, rm, cond::NE));
    write_def_spill_gpr(bytes, spill_off, dst);
}

/// Fused `icmp; select` — the compare re-emits directly before the
/// CSEL so the predicate rides NZCV instead of a CSET'd register
/// (drops cset + cmp #0, two instructions per pair). Value operands
/// materialize first; mov/movz/ldr reloads never touch NZCV.
pub(crate) fn emit_select_fused(
    bytes: &mut Vec<u8>,
    inst: &Inst,
    ty: &Type,
    fc: &super::brfuse::FusedCmp,
    then_op: &Operand,
    else_op: &Operand,
    alloc: &Assignment,
) {
    let result_vid = inst.result.expect("Select must have a result");
    if *ty == Type::F64 {
        // FPR value arms never collide with emit_compare_nzcv, which
        // only touches GPR scratches. The GPR carrier a ConstF64 rides
        // into its FPR is dead by the time the compare reuses it.
        let (dst, spill_off) = alloc.def_fpr(result_vid, FP_SCRATCH_RESULT);
        let rn = materialize_operand_fpr(bytes, then_op, FP_SCRATCH_LHS, OP_SCRATCH_TMP, alloc);
        let rm = materialize_operand_fpr(bytes, else_op, FP_SCRATCH_RHS, OP_SCRATCH_TMP, alloc);
        emit_compare_nzcv(bytes, &fc.lhs, &fc.rhs, alloc);
        write_u32(bytes, fcsel_d(dst, rn, rm, ipred_to_cond(fc.pred)));
        write_def_spill_fpr(bytes, spill_off, dst);
        return;
    }
    let (dst, spill_off) = alloc.def_gpr(result_vid, OP_SCRATCH_RESULT_GPR);
    // value arms take TMP / RESULT scratches — emit_compare_nzcv owns
    // LHS/RHS for the compare operands and would clobber them. A
    // spilled dst aliasing rm's scratch is fine: CSEL reads its
    // sources before writing Rd.
    let rn = materialize_operand_gpr(bytes, then_op, OP_SCRATCH_TMP, alloc);
    let rm = materialize_operand_gpr(bytes, else_op, OP_SCRATCH_RESULT_GPR, alloc);
    emit_compare_nzcv(bytes, &fc.lhs, &fc.rhs, alloc);
    write_u32(bytes, csel_cond(dst, rn, rm, ipred_to_cond(fc.pred)));
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

    /// `%0 = icmp sgt 7, 5 ; %1 = select i64 %0, 7, 5 ; ret %1` —
    /// max-shape `icmp; select` with a single-use cond fuses: the
    /// compare re-emits before the CSEL on its own predicate, and the
    /// CSET + cmp #0 detour disappears.
    #[test]
    fn adjacent_single_use_select_fuses_cmp_csel() {
        let func = select_max_func(false);
        let words = select_words(&func);
        let csel_at = words
            .iter()
            .position(|w| (w >> 24) == 0x9A && (w >> 10) & 0b11 == 0)
            .expect("csel word emitted");
        // no CSET anywhere (0x9A9F prefix = CSINC Xd, XZR, XZR alias)
        assert!(
            !words.iter().any(|w| (w >> 16) == 0x9A9F),
            "cset must be absorbed by the fuse"
        );
        // preceding word is the re-emitted `cmp x, #5` (SUBS XZR
        // imm12 form on the original operands), not `cmp cond, #0`.
        let prev = words[csel_at - 1];
        assert_eq!(
            prev & 0xFFFF_FC1F,
            0xF100_141F,
            "cmp #5 must immediately precede csel, got {prev:08X}"
        );
    }

    /// a second consumer of the cond kills the fuse — the ICmp keeps
    /// its CSET and the Select falls back to `cmp cond, #0; csel ne`.
    #[test]
    fn multi_use_cond_keeps_cmp_zero_path() {
        let func = select_max_func(true);
        let words = select_words(&func);
        let csel_at = words
            .iter()
            .position(|w| (w >> 24) == 0x9A && (w >> 10) & 0b11 == 0)
            .expect("csel word emitted");
        let prev = words[csel_at - 1];
        assert_eq!(
            prev & 0xFFFF_FC1F,
            0xF100_001F,
            "cmp #0 must immediately precede csel, got {prev:08X}"
        );
    }

    /// `%0 = icmp sgt 7, 5 ; %1 = select i64 %0, 7, 5` (+ optional
    /// second use of %0) returning %1.
    fn select_max_func(extra_use: bool) -> torajs_core::ssa::Function {
        use torajs_core::ssa::{
            Block, BlockId, Function, InstKind, Terminator, ValueId, ValueInfo,
        };
        let mk = |result: u32, kind: InstKind| Inst {
            result: Some(ValueId(result)),
            kind,
            origin: None,
        };
        let vi = |ty: Type| ValueInfo { ty, name: None };
        let mut insts = vec![
            mk(
                0,
                InstKind::ICmp(IPred::Sgt, Operand::ConstI64(7), Operand::ConstI64(5)),
            ),
            mk(
                1,
                InstKind::Select(
                    Type::I64,
                    Operand::Value(ValueId(0)),
                    Operand::ConstI64(7),
                    Operand::ConstI64(5),
                ),
            ),
        ];
        let mut values = vec![vi(Type::Bool), vi(Type::I64)];
        if extra_use {
            insts.push(mk(2, InstKind::ZExtBoolToI64(Operand::Value(ValueId(0)))));
            values.push(vi(Type::I64));
        }
        Function {
            name: "select_max".into(),
            params: Vec::new(),
            ret: Type::I64,
            values,
            blocks: vec![Block {
                id: BlockId(0),
                insts,
                term: Terminator::Ret(Some(Operand::Value(ValueId(1)))),
            }],
            current_origin: None,
        }
    }

    fn select_words(func: &torajs_core::ssa::Function) -> Vec<u32> {
        compile_function(func)
            .bytes
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
            .collect()
    }
}
