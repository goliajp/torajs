//! Cast lowering — BitCast (S2-B2), int↔fp (SCVTF/FCVTZS), ZExt
//! Bool/I32→I64 (AND-imm-#1 / MOV Wd,Wn auto-zero-extend), Trunc
//! I64→Bool (AND-imm-#1), and Ptr↔Int (MOV Xd,Xn).
//!
//! Each cast emits exactly one machine instruction after the source
//! operand is materialized to its required register class.

use torajs_core::ssa::{Inst, Operand};

use super::operand::{materialize_operand_fpr, materialize_operand_gpr};
use super::write_u32;
use super::{FP_SCRATCH_LHS, OP_SCRATCH_LHS};
use crate::enc::{
    and_imm_one, fcvtzs_x_d, fmov_d_from_x, fmov_x_from_d, mov_w_reg, mov_x_reg, scvtf_d_x,
};
use crate::regalloc::Assignment;

pub fn emit_bitcast_f64_to_i64(
    bytes: &mut Vec<u8>,
    inst: &Inst,
    src: &Operand,
    alloc: &Assignment,
) {
    let dst = inst
        .result
        .map(|v| alloc.of(v).as_gpr())
        .expect("BitCastF64ToI64 must have a result");
    let src_fpr = materialize_operand_fpr(bytes, src, FP_SCRATCH_LHS, OP_SCRATCH_LHS, alloc);
    write_u32(bytes, fmov_x_from_d(dst, src_fpr));
}

pub fn emit_bitcast_i64_to_f64(
    bytes: &mut Vec<u8>,
    inst: &Inst,
    src: &Operand,
    alloc: &Assignment,
) {
    let dst = inst
        .result
        .map(|v| alloc.of(v).as_fpr())
        .expect("BitCastI64ToF64 must have a result");
    let src_gpr = materialize_operand_gpr(bytes, src, OP_SCRATCH_LHS, alloc);
    write_u32(bytes, fmov_d_from_x(dst, src_gpr));
}

/// SiToFp Operand → Fpr: SCVTF Dd, Xn (signed int64 → f64).
pub fn emit_si_to_fp(bytes: &mut Vec<u8>, inst: &Inst, src: &Operand, alloc: &Assignment) {
    let dst = inst
        .result
        .map(|v| alloc.of(v).as_fpr())
        .expect("SiToFp must have a result");
    let src_gpr = materialize_operand_gpr(bytes, src, OP_SCRATCH_LHS, alloc);
    write_u32(bytes, scvtf_d_x(dst, src_gpr));
}

/// FpToSi Operand → Gpr: FCVTZS Xd, Dn (f64 → int64, round toward 0).
pub fn emit_fp_to_si(bytes: &mut Vec<u8>, inst: &Inst, src: &Operand, alloc: &Assignment) {
    let dst = inst
        .result
        .map(|v| alloc.of(v).as_gpr())
        .expect("FpToSi must have a result");
    let src_fpr = materialize_operand_fpr(bytes, src, FP_SCRATCH_LHS, OP_SCRATCH_LHS, alloc);
    write_u32(bytes, fcvtzs_x_d(dst, src_fpr));
}

/// ZExtBoolToI64: AND Xd, Xn, #1 — keep the low bit, zero the rest.
pub fn emit_zext_bool_to_i64(bytes: &mut Vec<u8>, inst: &Inst, src: &Operand, alloc: &Assignment) {
    let dst = inst
        .result
        .map(|v| alloc.of(v).as_gpr())
        .expect("ZExtBoolToI64 must have a result");
    let src_gpr = materialize_operand_gpr(bytes, src, OP_SCRATCH_LHS, alloc);
    write_u32(bytes, and_imm_one(dst, src_gpr));
}

/// ZExtI32ToI64: MOV Wd, Wn — W-form write auto-clears top 32 bits
/// of Xd. Textbook ZExt encoding (no explicit AND #0xFFFF_FFFF mask).
pub fn emit_zext_i32_to_i64(bytes: &mut Vec<u8>, inst: &Inst, src: &Operand, alloc: &Assignment) {
    let dst = inst
        .result
        .map(|v| alloc.of(v).as_gpr())
        .expect("ZExtI32ToI64 must have a result");
    let src_gpr = materialize_operand_gpr(bytes, src, OP_SCRATCH_LHS, alloc);
    write_u32(bytes, mov_w_reg(dst, src_gpr));
}

/// TruncI64ToBool: AND Xd, Xn, #1 — LLVM-style trunc takes the low
/// bit, matching the existing SSA semantics.
pub fn emit_trunc_i64_to_bool(bytes: &mut Vec<u8>, inst: &Inst, src: &Operand, alloc: &Assignment) {
    let dst = inst
        .result
        .map(|v| alloc.of(v).as_gpr())
        .expect("TruncI64ToBool must have a result");
    let src_gpr = materialize_operand_gpr(bytes, src, OP_SCRATCH_LHS, alloc);
    write_u32(bytes, and_imm_one(dst, src_gpr));
}

/// PtrToInt: MOV Xd, Xn — machine-level no-op (Ptr and I64 share the
/// 64-bit GPR representation) but a copy is needed when the trivial
/// allocator places src and dst in different physical registers.
pub fn emit_ptr_to_int(bytes: &mut Vec<u8>, inst: &Inst, src: &Operand, alloc: &Assignment) {
    let dst = inst
        .result
        .map(|v| alloc.of(v).as_gpr())
        .expect("PtrToInt must have a result");
    let src_gpr = materialize_operand_gpr(bytes, src, OP_SCRATCH_LHS, alloc);
    write_u32(bytes, mov_x_reg(dst, src_gpr));
}

/// IntToPtr: MOV Xd, Xn — symmetric inverse of PtrToInt.
pub fn emit_int_to_ptr(bytes: &mut Vec<u8>, inst: &Inst, src: &Operand, alloc: &Assignment) {
    let dst = inst
        .result
        .map(|v| alloc.of(v).as_gpr())
        .expect("IntToPtr must have a result");
    let src_gpr = materialize_operand_gpr(bytes, src, OP_SCRATCH_LHS, alloc);
    write_u32(bytes, mov_x_reg(dst, src_gpr));
}

#[cfg(test)]
mod tests {
    use super::super::compile_function;
    use super::super::test_fixtures::words_to_le_bytes;
    use crate::enc;
    use crate::reg::{Fpr, Gpr};
    use torajs_core::ssa::{
        BinOp, Block, BlockId, Function, Inst, InstKind, Operand, Terminator, Type, ValueId,
        ValueInfo,
    };

    /// Single-inst cast fixture: `fn cast() -> RetTy { CAST src }`.
    fn build_cast_const_i64(name: &str, ret_ty: Type, kind: InstKind) -> Function {
        let v0 = ValueId(0);
        Function {
            name: name.into(),
            params: Vec::new(),
            ret: ret_ty.clone(),
            values: vec![ValueInfo {
                ty: ret_ty,
                name: Some("v0".into()),
            }],
            blocks: vec![Block {
                id: BlockId(0),
                insts: vec![Inst {
                    result: Some(v0),
                    kind,
                    origin: None,
                }],
                term: Terminator::Ret(Some(Operand::Value(v0))),
            }],
            current_origin: None,
        }
    }

    /// Mixed-type fixture: FAdd (f64) → BitCastF64ToI64 → ret i64.
    /// Catches FPR ↔ GPR alloc routing + FMOV-x-from-d at the cast.
    #[test]
    fn fp_add_then_bitcast_to_i64() {
        let v0 = ValueId(0); // F64 — FAdd result
        let v1 = ValueId(1); // I64 — BitCast result, ret
        let func = Function {
            name: "fp_add_15_plus_25_bitcast".into(),
            params: Vec::new(),
            ret: Type::I64,
            values: vec![
                ValueInfo {
                    ty: Type::F64,
                    name: Some("v0".into()),
                },
                ValueInfo {
                    ty: Type::I64,
                    name: Some("v1".into()),
                },
            ],
            blocks: vec![Block {
                id: BlockId(0),
                insts: vec![
                    Inst {
                        result: Some(v0),
                        kind: InstKind::BinOp(
                            BinOp::FAdd,
                            Operand::ConstF64(1.5),
                            Operand::ConstF64(2.5),
                        ),
                        origin: None,
                    },
                    Inst {
                        result: Some(v1),
                        kind: InstKind::BitCastF64ToI64(Operand::Value(v0)),
                        origin: None,
                    },
                ],
                term: Terminator::Ret(Some(Operand::Value(v1))),
            }],
            current_origin: None,
        };
        let compiled = compile_function(&func);

        // 1.5: 0x3FF8_0000_0000_0000   2.5: 0x4004_0000_0000_0000
        // v0 (FAdd result) → V19 (first scratch FPR after LS-3
        // reserved V18 for FP_SCRATCH_RESULT), v1 → X0 (ret).
        // FP_SCRATCH_LHS/RHS (V16/V17) materialize the ConstF64 args.
        let expected = words_to_le_bytes(&[
            enc::movz_imm(Gpr::X9, 0, 0),
            enc::movk_imm(Gpr::X9, 0x3FF8, 3),
            enc::fmov_d_from_x(Fpr::V16, Gpr::X9),
            enc::movz_imm(Gpr::X10, 0, 0),
            enc::movk_imm(Gpr::X10, 0x4004, 3),
            enc::fmov_d_from_x(Fpr::V17, Gpr::X10),
            enc::fadd_d(Fpr::V19, Fpr::V16, Fpr::V17),
            enc::fmov_x_from_d(Gpr::X0, Fpr::V19),
            enc::ret(Gpr::X30),
        ]);
        assert_eq!(compiled.bytes, expected);
        assert_eq!(compiled.bytes.len(), 36);
    }

    /// SiToFp: `fn() -> f64 { 42 as f64 }`.
    /// Sequence: MOVZ x9, #42 (materialize ConstI64);
    ///           SCVTF d0, x9 (convert);
    ///           RET.
    #[test]
    fn si_to_fp_const_42_byte_equal() {
        let func = build_cast_const_i64(
            "si_to_fp_42",
            Type::F64,
            InstKind::SiToFp(Operand::ConstI64(42)),
        );
        let compiled = compile_function(&func);
        let expected = words_to_le_bytes(&[
            enc::movz_imm(Gpr::X9, 42, 0),
            enc::scvtf_d_x(Fpr::V0, Gpr::X9),
            enc::ret(Gpr::X30),
        ]);
        assert_eq!(compiled.bytes, expected);
    }

    /// ZExtBoolToI64: `fn() -> i64 { (true as i64) & 1 }` — AND-imm.
    #[test]
    fn zext_bool_true_byte_equal() {
        let func = build_cast_const_i64(
            "zext_bool_true",
            Type::I64,
            InstKind::ZExtBoolToI64(Operand::ConstBool(true)),
        );
        let compiled = compile_function(&func);
        let expected = words_to_le_bytes(&[
            enc::movz_imm(Gpr::X9, 1, 0),
            enc::and_imm_one(Gpr::X0, Gpr::X9),
            enc::ret(Gpr::X30),
        ]);
        assert_eq!(compiled.bytes, expected);
    }

    /// ZExtI32ToI64: MOV Wd, Wn — auto-clears top 32 bits.
    #[test]
    fn zext_i32_to_i64_byte_equal() {
        let func = build_cast_const_i64(
            "zext_i32_to_i64",
            Type::I64,
            InstKind::ZExtI32ToI64(Operand::ConstI32(42)),
        );
        let compiled = compile_function(&func);
        let expected = words_to_le_bytes(&[
            enc::movz_imm(Gpr::X9, 42, 0),
            enc::mov_w_reg(Gpr::X0, Gpr::X9),
            enc::ret(Gpr::X30),
        ]);
        assert_eq!(compiled.bytes, expected);
    }

    /// PtrToInt: MOV Xd, Xn — full 64-bit move.
    #[test]
    fn ptr_to_int_null_byte_equal() {
        let func = build_cast_const_i64(
            "ptr_to_int_null",
            Type::I64,
            InstKind::PtrToInt(Operand::ConstPtrNull),
        );
        let compiled = compile_function(&func);
        // ConstPtrNull → materialize 0 → MOVZ x9, #0 → MOV x0, x9
        let expected = words_to_le_bytes(&[
            enc::movz_imm(Gpr::X9, 0, 0),
            enc::mov_x_reg(Gpr::X0, Gpr::X9),
            enc::ret(Gpr::X30),
        ]);
        assert_eq!(compiled.bytes, expected);
    }
}
