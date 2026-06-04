//! BitCastF64ToI64 / BitCastI64ToF64 — single FMOV between Xn and Dn.

use torajs_core::ssa::{Inst, Operand};

use super::operand::{materialize_operand_fpr, materialize_operand_gpr};
use super::write_u32;
use super::{FP_SCRATCH_LHS, OP_SCRATCH_LHS};
use crate::enc::{fmov_d_from_x, fmov_x_from_d};
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
        // v0 → V16 (first scratch FPR), v1 → X0 (ret).
        let expected = words_to_le_bytes(&[
            enc::movz_imm(Gpr::X9, 0, 0),
            enc::movk_imm(Gpr::X9, 0x3FF8, 3),
            enc::fmov_d_from_x(Fpr::V16, Gpr::X9),
            enc::movz_imm(Gpr::X10, 0, 0),
            enc::movk_imm(Gpr::X10, 0x4004, 3),
            enc::fmov_d_from_x(Fpr::V17, Gpr::X10),
            enc::fadd_d(Fpr::V16, Fpr::V16, Fpr::V17),
            enc::fmov_x_from_d(Gpr::X0, Fpr::V16),
            enc::ret(Gpr::X30),
        ]);
        assert_eq!(compiled.bytes, expected);
        assert_eq!(compiled.bytes.len(), 36);
    }
}
