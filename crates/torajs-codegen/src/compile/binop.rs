//! BinOp dispatcher — full integer + f64 surface.

use torajs_core::ssa::{BinOp, Inst, Operand};

use super::operand::{materialize_operand_fpr, materialize_operand_gpr};
use super::{
    FP_SCRATCH_LHS, FP_SCRATCH_RESULT, FP_SCRATCH_RHS, OP_SCRATCH_LHS, OP_SCRATCH_RESULT_GPR,
    OP_SCRATCH_RHS, OP_SCRATCH_TMP, write_def_spill_fpr, write_def_spill_gpr, write_u32,
};
use crate::enc::{
    add_reg, and_reg, asrv_reg, bl_imm26, eor_reg, fadd_d, fdiv_d, fmov_d_to_d, fmul_d, fsub_d,
    lslv_reg, lsrv_reg, msub_reg, mul_reg, orr_reg, sdiv_reg, sub_reg,
};
use crate::reg::aapcs64;
use crate::regalloc::Assignment;
use crate::reloc::{CallTarget, Reloc, RelocKind};

/// P-OPT Phase 2 chunk 11b — emit `%v = neg %op` as the aarch64
/// `NEG Xd, Xn` alias of `SUB Xd, XZR, Xn` (ARM ARM C7.2.273).
/// Emitted in place of `sub 0 x` by the egraph `SubNegate` rule.
pub fn emit_neg(bytes: &mut Vec<u8>, inst: &Inst, op: &Operand, alloc: &Assignment) {
    use crate::reg::Gpr;
    let result_vid = inst.result.expect("Neg must have a result ValueId");
    let (dst, spill_off) = alloc.def_gpr(result_vid, OP_SCRATCH_RESULT_GPR);
    let rn = materialize_operand_gpr(bytes, op, OP_SCRATCH_LHS, alloc);
    write_u32(bytes, sub_reg(dst, Gpr::XZR, rn));
    write_def_spill_gpr(bytes, spill_off, dst);
}

pub fn emit_binop(
    bytes: &mut Vec<u8>,
    relocs: &mut Vec<Reloc>,
    inst: &Inst,
    op: &BinOp,
    lhs: &Operand,
    rhs: &Operand,
    alloc: &Assignment,
) {
    match op {
        // --- integer ops (GPR) ---
        BinOp::Add
        | BinOp::Sub
        | BinOp::Mul
        | BinOp::SDiv
        | BinOp::SRem
        | BinOp::And
        | BinOp::Or
        | BinOp::Xor
        | BinOp::Shl
        | BinOp::AShr
        | BinOp::LShr => {
            let result_vid = inst.result.expect("int BinOp must have a result ValueId");
            let (dst, spill_off) = alloc.def_gpr(result_vid, OP_SCRATCH_RESULT_GPR);
            let rn = materialize_operand_gpr(bytes, lhs, OP_SCRATCH_LHS, alloc);
            let rm = materialize_operand_gpr(bytes, rhs, OP_SCRATCH_RHS, alloc);
            match op {
                BinOp::Add => write_u32(bytes, add_reg(dst, rn, rm)),
                BinOp::Sub => write_u32(bytes, sub_reg(dst, rn, rm)),
                BinOp::Mul => write_u32(bytes, mul_reg(dst, rn, rm)),
                BinOp::SDiv => write_u32(bytes, sdiv_reg(dst, rn, rm)),
                BinOp::SRem => {
                    write_u32(bytes, sdiv_reg(OP_SCRATCH_TMP, rn, rm));
                    write_u32(bytes, msub_reg(dst, OP_SCRATCH_TMP, rm, rn));
                }
                BinOp::And => write_u32(bytes, and_reg(dst, rn, rm)),
                BinOp::Or => write_u32(bytes, orr_reg(dst, rn, rm)),
                BinOp::Xor => write_u32(bytes, eor_reg(dst, rn, rm)),
                BinOp::Shl => write_u32(bytes, lslv_reg(dst, rn, rm)),
                BinOp::AShr => write_u32(bytes, asrv_reg(dst, rn, rm)),
                BinOp::LShr => write_u32(bytes, lsrv_reg(dst, rn, rm)),
                _ => unreachable!(),
            }
            write_def_spill_gpr(bytes, spill_off, dst);
        }
        // --- f64 ops (FPR) ---
        BinOp::FAdd | BinOp::FSub | BinOp::FMul | BinOp::FDiv => {
            let result_vid = inst.result.expect("fp BinOp must have a result ValueId");
            let (dst, spill_off) = alloc.def_fpr(result_vid, FP_SCRATCH_RESULT);
            let rn = materialize_operand_fpr(bytes, lhs, FP_SCRATCH_LHS, OP_SCRATCH_LHS, alloc);
            let rm = materialize_operand_fpr(bytes, rhs, FP_SCRATCH_RHS, OP_SCRATCH_RHS, alloc);
            match op {
                BinOp::FAdd => write_u32(bytes, fadd_d(dst, rn, rm)),
                BinOp::FSub => write_u32(bytes, fsub_d(dst, rn, rm)),
                BinOp::FMul => write_u32(bytes, fmul_d(dst, rn, rm)),
                BinOp::FDiv => write_u32(bytes, fdiv_d(dst, rn, rm)),
                _ => unreachable!(),
            }
            write_def_spill_fpr(bytes, spill_off, dst);
        }
        BinOp::FRem => {
            // aarch64 has no FREM instruction. Lower to a `BL _fmod`
            // libm call per the C99 / IEEE 754 remainder semantics that
            // LLVM `frem` and JS `%` on floats both inherit.
            //
            // AAPCS64 §5.1.2: f64 args land in D0..D7, f64 ret in D0.
            // Frame prologue already saved FP/LR because `regalloc::
            // inst_emits_bl` marks FRem as a BL site (uses_calls=true).
            //
            // Sequence (5 inst):
            //   FMOV  d0, <lhs_reg>   (skip if lhs already in d0)
            //   FMOV  d1, <rhs_reg>   (skip if rhs already in d1)
            //   BL    _fmod           (CallSite/Extern reloc)
            //   FMOV  <dst>, d0       (skip if dst is d0 — typical ret)
            //
            // Direct BL + extern reloc (the textbook clang/LLVM form) —
            // not ADRP+BLR through a stub slot — so the BL's 26-bit
            // displacement targets the lazy-binding stub directly.
            let result_vid = inst.result.expect("FRem must have a result ValueId");
            let (dst, spill_off) = alloc.def_fpr(result_vid, FP_SCRATCH_RESULT);

            let lhs_reg =
                materialize_operand_fpr(bytes, lhs, FP_SCRATCH_LHS, OP_SCRATCH_LHS, alloc);
            let rhs_reg =
                materialize_operand_fpr(bytes, rhs, FP_SCRATCH_RHS, OP_SCRATCH_RHS, alloc);

            let d0 = aapcs64::FP_ARG_RET[0];
            let d1 = aapcs64::FP_ARG_RET[1];
            if lhs_reg != d0 {
                write_u32(bytes, fmov_d_to_d(d0, lhs_reg));
            }
            if rhs_reg != d1 {
                write_u32(bytes, fmov_d_to_d(d1, rhs_reg));
            }

            let bl_byte_offset = bytes.len() as u32;
            relocs.push(Reloc {
                byte_offset: bl_byte_offset,
                kind: RelocKind::CallSite {
                    target: CallTarget::Extern("_fmod".to_string()),
                },
            });
            write_u32(bytes, bl_imm26(0));

            if dst != d0 {
                write_u32(bytes, fmov_d_to_d(dst, d0));
            }
            write_def_spill_fpr(bytes, spill_off, dst);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::compile_function;
    use super::super::test_fixtures::{build_binop_const, build_fp_binop_const, words_to_le_bytes};
    use crate::enc;
    use crate::reg::{Fpr, Gpr};
    use torajs_core::ssa::BinOp;

    /// MOVZ x9,#lhs / MOVZ x10,#rhs / <op> x0,x9,x10 / RET — assert
    /// the 4-word reference matches.
    fn assert_binop_4word(name: &str, op: BinOp, lhs: i64, rhs: i64, op_word: u32) {
        let func = build_binop_const(name, op, lhs, rhs);
        let compiled = compile_function(&func);
        let expected = words_to_le_bytes(&[
            enc::movz_imm(Gpr::X9, lhs as u16, 0),
            enc::movz_imm(Gpr::X10, rhs as u16, 0),
            op_word,
            enc::ret(Gpr::X30),
        ]);
        assert_eq!(
            compiled.bytes, expected,
            "{name}: mismatch — expected {expected:02X?}, got {:02X?}",
            compiled.bytes
        );
    }

    #[test]
    fn five_minus_three() {
        assert_binop_4word(
            "sub",
            BinOp::Sub,
            5,
            3,
            enc::sub_reg(Gpr::X0, Gpr::X9, Gpr::X10),
        );
    }

    #[test]
    fn two_times_three() {
        assert_binop_4word(
            "mul",
            BinOp::Mul,
            2,
            3,
            enc::mul_reg(Gpr::X0, Gpr::X9, Gpr::X10),
        );
    }

    #[test]
    fn ten_div_three() {
        assert_binop_4word(
            "sdiv",
            BinOp::SDiv,
            10,
            3,
            enc::sdiv_reg(Gpr::X0, Gpr::X9, Gpr::X10),
        );
    }

    #[test]
    fn ff_and_f0() {
        assert_binop_4word(
            "and",
            BinOp::And,
            0xFF,
            0xF0,
            enc::and_reg(Gpr::X0, Gpr::X9, Gpr::X10),
        );
    }

    #[test]
    fn zero_f_or_f0() {
        assert_binop_4word(
            "or",
            BinOp::Or,
            0x0F,
            0xF0,
            enc::orr_reg(Gpr::X0, Gpr::X9, Gpr::X10),
        );
    }

    #[test]
    fn aa_xor_55() {
        assert_binop_4word(
            "xor",
            BinOp::Xor,
            0xAA,
            0x55,
            enc::eor_reg(Gpr::X0, Gpr::X9, Gpr::X10),
        );
    }

    #[test]
    fn one_shl_three() {
        assert_binop_4word(
            "shl",
            BinOp::Shl,
            1,
            3,
            enc::lslv_reg(Gpr::X0, Gpr::X9, Gpr::X10),
        );
    }

    #[test]
    fn sixteen_ashr_two() {
        assert_binop_4word(
            "ashr",
            BinOp::AShr,
            16,
            2,
            enc::asrv_reg(Gpr::X0, Gpr::X9, Gpr::X10),
        );
    }

    #[test]
    fn sixteen_lshr_two() {
        assert_binop_4word(
            "lshr",
            BinOp::LShr,
            16,
            2,
            enc::lsrv_reg(Gpr::X0, Gpr::X9, Gpr::X10),
        );
    }

    /// SRem lowers to SDIV scratch + MSUB Rd — emits *5* words.
    #[test]
    fn ten_rem_three_compound() {
        let func = build_binop_const("ten_rem_three", BinOp::SRem, 10, 3);
        let compiled = compile_function(&func);
        let expected = words_to_le_bytes(&[
            enc::movz_imm(Gpr::X9, 10, 0),
            enc::movz_imm(Gpr::X10, 3, 0),
            enc::sdiv_reg(Gpr::X11, Gpr::X9, Gpr::X10),
            enc::msub_reg(Gpr::X0, Gpr::X11, Gpr::X10, Gpr::X9),
            enc::ret(Gpr::X30),
        ]);
        assert_eq!(compiled.bytes, expected);
        assert_eq!(compiled.bytes.len(), 20);
    }

    /// FRem fixture — `fn fp_rem_5_mod_3() -> f64 { 5.0 % 3.0 }`.
    /// Verifies the full S2-D-2 sequence: regalloc marks the SSA inst
    /// as a BL site (uses_calls=true → STP/MOV prologue), the FP args
    /// land in D0/D1 via the AAPCS64 lanes, the BL site records a
    /// CallSite/Extern("_fmod") reloc, and the result stays in D0 for
    /// the ret value.
    ///
    /// Expected 12-inst body (consts use q3-only MOVZ+MOVK chain,
    /// same shape as the other FP fixtures in this file).
    #[test]
    fn fp_rem_5_mod_3_byte_equal_and_records_extern_reloc() {
        use crate::reloc::{CallTarget, RelocKind};
        let func = build_fp_binop_const("fp_rem_5_mod_3", BinOp::FRem, 5.0, 3.0);
        let compiled = compile_function(&func);
        // 5.0_f64.to_bits() = 0x4014_0000_0000_0000 (q3 = 0x4014)
        // 3.0_f64.to_bits() = 0x4008_0000_0000_0000 (q3 = 0x4008)
        let expected = words_to_le_bytes(&[
            // prologue: BL site → uses_calls=true → STP+MOV (no SUB).
            enc::stp_pre_index(Gpr::X29, Gpr::X30, Gpr::SP, -16),
            enc::add_imm(Gpr::X29, Gpr::SP, 0),
            // lhs 5.0 → V16.
            enc::movz_imm(Gpr::X9, 0, 0),
            enc::movk_imm(Gpr::X9, 0x4014, 3),
            enc::fmov_d_from_x(Fpr::V16, Gpr::X9),
            // rhs 3.0 → V17.
            enc::movz_imm(Gpr::X10, 0, 0),
            enc::movk_imm(Gpr::X10, 0x4008, 3),
            enc::fmov_d_from_x(Fpr::V17, Gpr::X10),
            // arg setup: V16/V17 → D0/D1 (AAPCS64 §5.1.2 FP args).
            enc::fmov_d_to_d(Fpr::V0, Fpr::V16),
            enc::fmov_d_to_d(Fpr::V1, Fpr::V17),
            // BL _fmod (CallSite reloc patched by torajs-link).
            enc::bl_imm26(0),
            // dst is ret → V0/D0, no post-call FMOV.
            // epilogue: LDP + RET.
            enc::ldp_post_index(Gpr::X29, Gpr::X30, Gpr::SP, 16),
            enc::ret(Gpr::X30),
        ]);
        assert_eq!(
            compiled.bytes, expected,
            "fp_rem_5_mod_3: expected {expected:02X?}, got {:02X?}",
            compiled.bytes
        );

        // BL site lives at the start of the 11th instruction (after
        // 2-inst prologue + 8-inst arg setup = 10 inst × 4 = 40 bytes).
        assert_eq!(compiled.relocs.len(), 1, "exactly one extern reloc");
        let reloc = &compiled.relocs[0];
        assert_eq!(reloc.byte_offset, 40);
        match &reloc.kind {
            RelocKind::CallSite {
                target: CallTarget::Extern(sym),
            } => assert_eq!(sym, "_fmod"),
            other => panic!("expected CallSite/Extern(_fmod), got {other:?}"),
        }

        // Frame: uses_calls=true (regalloc::inst_emits_bl(FRem)),
        // alloca_bytes=0 → not trivial.
        assert!(!compiled.frame.is_trivial());
        assert_eq!(compiled.frame.alloca_bytes, 0);
        assert!(compiled.frame.uses_calls);
    }

    /// FMul-shape fixture with F64 return type — verifies the ret
    /// value lands in V0/D0 (AAPCS64 §5.1.2) end-to-end.
    #[test]
    fn fp_mul_two_point_five_times_two_returns_d0() {
        let func = build_fp_binop_const("fp_mul_25_times_2", BinOp::FMul, 2.5, 2.0);
        let compiled = compile_function(&func);

        // 2.5_f64.to_bits() = 0x4004_0000_0000_0000 (q3 = 0x4004)
        // 2.0_f64.to_bits() = 0x4000_0000_0000_0000 (q3 = 0x4000)
        let expected = words_to_le_bytes(&[
            enc::movz_imm(Gpr::X9, 0, 0),
            enc::movk_imm(Gpr::X9, 0x4004, 3),
            enc::fmov_d_from_x(Fpr::V16, Gpr::X9),
            enc::movz_imm(Gpr::X10, 0, 0),
            enc::movk_imm(Gpr::X10, 0x4000, 3),
            enc::fmov_d_from_x(Fpr::V17, Gpr::X10),
            enc::fmul_d(Fpr::V0, Fpr::V16, Fpr::V17),
            enc::ret(Gpr::X30),
        ]);
        assert_eq!(compiled.bytes, expected);
    }
}
