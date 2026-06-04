//! Function-level compile driver.
//!
//! Walks `torajs_core::ssa::Function` once, dispatches each `InstKind`
//! to the matching aarch64 instruction sequence via `enc::*`, and
//! emits the little-endian byte stream + any `Reloc` descriptors.
//!
//! S1 coverage: `InstKind::BinOp(BinOp::Add, Operand, Operand)` with
//! both operands as ConstI64 or Value, and `Terminator::Ret`. Every
//! other InstKind variant panics with `todo!("S2+: ...")` — when the
//! S2 sub-step adds the matching arm, the panic becomes the build-up
//! signal pointing right at the open case.
//!
//! S2-A coverage: full integer `BinOp` surface (Sub/Mul/SDiv/SRem +
//! And/Or/Xor + Shl/AShr/LShr) plus 64-bit `ConstI64` materialization
//! via MOVZ + up-to-3 MOVK quadrant chain (handles negative i64 as
//! `as u64` bit pattern). SREM emits as `SDIV` + `MSUB` pair.

use torajs_core::ssa::{BinOp, Function, InstKind, Operand, Terminator};

use crate::enc::{
    add_reg, and_reg, asrv_reg, eor_reg, lslv_reg, lsrv_reg, movk_imm, movz_imm, msub_reg, mul_reg,
    orr_reg, ret, sdiv_reg, sub_reg,
};
use crate::frame::FrameLayout;
use crate::reg::Gpr;
use crate::regalloc::{Assignment, allocate_trivial};
use crate::reloc::Reloc;

/// Output of `compile_function` — raw aarch64 bytes + per-function
/// reloc table, ready to hand off to torajs-obj (#7).
#[derive(Debug, Clone)]
pub struct CompiledFunction {
    pub name: String,
    pub bytes: Vec<u8>,
    pub relocs: Vec<Reloc>,
    pub frame: FrameLayout,
}

/// Scratch registers used to materialize Operand constants and
/// arrange them into the GPRs the encoder needs. Picked from the
/// AAPCS64 caller-saved pool, deliberately at the *low* end so the
/// trivial allocator (which assigns from the same pool starting at
/// `aapcs64::CALLER_SAVED_SCRATCH[0]`) doesn't collide for the S1
/// surface (1+2 has v0 → x0 so x9/x10 stay free).
///
/// S3+ adds proper Linear Scan; the operand-scratch story becomes
/// unnecessary because every Operand::Value carries its own assigned
/// register and ConstI64 immediates fold into ADD imm12 when small.
const OP_SCRATCH_LHS: Gpr = Gpr::X9;
const OP_SCRATCH_RHS: Gpr = Gpr::X10;
/// Tertiary scratch for compound ops that emit more than one
/// instruction (e.g. `BinOp::SRem` lowers to SDIV + MSUB and needs
/// a temp for the quotient).
const OP_SCRATCH_TMP: Gpr = Gpr::X11;

/// Compile one SSA Function to aarch64 bytes.
pub fn compile_function(func: &Function) -> CompiledFunction {
    let alloc = allocate_trivial(func);
    let frame = FrameLayout::leaf_no_spill();
    let mut bytes: Vec<u8> = Vec::new();
    let relocs: Vec<Reloc> = Vec::new();

    // Prologue (empty for trivial frame).
    if !frame.is_trivial() {
        todo!("S3+: emit AAPCS64 prologue");
    }

    for block in &func.blocks {
        for inst in &block.insts {
            emit_inst(&mut bytes, inst, &alloc);
        }
        emit_terminator(&mut bytes, &block.term);
    }

    CompiledFunction {
        name: func.name.clone(),
        bytes,
        relocs,
        frame,
    }
}

fn emit_inst(bytes: &mut Vec<u8>, inst: &torajs_core::ssa::Inst, alloc: &Assignment) {
    match &inst.kind {
        InstKind::BinOp(op, lhs, rhs) => {
            let dst = inst
                .result
                .map(|v| alloc.of(v))
                .expect("BinOp must have a result ValueId");
            let rn = materialize_operand(bytes, lhs, OP_SCRATCH_LHS, alloc);
            let rm = materialize_operand(bytes, rhs, OP_SCRATCH_RHS, alloc);
            match op {
                BinOp::Add => write_u32(bytes, add_reg(dst, rn, rm)),
                BinOp::Sub => write_u32(bytes, sub_reg(dst, rn, rm)),
                BinOp::Mul => write_u32(bytes, mul_reg(dst, rn, rm)),
                BinOp::SDiv => write_u32(bytes, sdiv_reg(dst, rn, rm)),
                BinOp::SRem => {
                    // rem = a - (a / b) * b
                    //   SDIV  tmp, rn, rm
                    //   MSUB  dst, tmp, rm, rn
                    write_u32(bytes, sdiv_reg(OP_SCRATCH_TMP, rn, rm));
                    write_u32(bytes, msub_reg(dst, OP_SCRATCH_TMP, rm, rn));
                }
                BinOp::And => write_u32(bytes, and_reg(dst, rn, rm)),
                BinOp::Or => write_u32(bytes, orr_reg(dst, rn, rm)),
                BinOp::Xor => write_u32(bytes, eor_reg(dst, rn, rm)),
                BinOp::Shl => write_u32(bytes, lslv_reg(dst, rn, rm)),
                BinOp::AShr => write_u32(bytes, asrv_reg(dst, rn, rm)),
                BinOp::LShr => write_u32(bytes, lsrv_reg(dst, rn, rm)),
                BinOp::FAdd | BinOp::FSub | BinOp::FMul | BinOp::FDiv | BinOp::FRem => {
                    todo!("S2-B: float BinOp::{:?}", op)
                }
            }
        }
        other => todo!("S2+: InstKind::{:?}", other),
    }
}

/// Place `op`'s value into `scratch` (if it's a constant) or return
/// the assigned register (if it's a Value). Returns the GPR holding
/// the operand on entry to the consuming instruction.
fn materialize_operand(bytes: &mut Vec<u8>, op: &Operand, scratch: Gpr, alloc: &Assignment) -> Gpr {
    match op {
        Operand::Value(v) => alloc.of(*v),
        Operand::ConstI64(c) => {
            materialize_const_i64(bytes, scratch, *c);
            scratch
        }
        other => todo!("S2+: Operand::{:?}", other),
    }
}

/// Emit MOVZ + up to 3 MOVK to materialize an arbitrary i64 into
/// `dst`. Negative i64 is handled via its `as u64` bit pattern
/// (all-ones quadrants → 4-instruction chain). MOVN-shortcut for
/// dense-ones constants is left for S3+ (RFC L3b "const-pool /
/// peephole" backlog).
///
/// Always emits at least one MOVZ (for v == 0 that single MOVZ #0
/// is correct). MOVK for higher quadrants only when their 16-bit
/// slice is non-zero — `MOVZ scratch, #0` followed by `MOVK scratch,
/// #0, LSL #16` would be a no-op but a wasted instruction.
fn materialize_const_i64(bytes: &mut Vec<u8>, dst: Gpr, value: i64) {
    let u = value as u64;
    let q0 = (u & 0xFFFF) as u16;
    let q1 = ((u >> 16) & 0xFFFF) as u16;
    let q2 = ((u >> 32) & 0xFFFF) as u16;
    let q3 = ((u >> 48) & 0xFFFF) as u16;
    write_u32(bytes, movz_imm(dst, q0, 0));
    if q1 != 0 {
        write_u32(bytes, movk_imm(dst, q1, 1));
    }
    if q2 != 0 {
        write_u32(bytes, movk_imm(dst, q2, 2));
    }
    if q3 != 0 {
        write_u32(bytes, movk_imm(dst, q3, 3));
    }
}

fn emit_terminator(bytes: &mut Vec<u8>, term: &Terminator) {
    match term {
        Terminator::Ret(_) => write_u32(bytes, ret(Gpr::X30)),
        other => todo!("S5: Terminator::{:?}", other),
    }
}

fn write_u32(bytes: &mut Vec<u8>, word: u32) {
    bytes.extend_from_slice(&word.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{Block, BlockId, Function, Inst, Type, ValueId, ValueInfo};

    /// Build a single-block binary-op fixture:
    /// `fn <name>() -> i64 { <lhs> <op> <rhs> }`
    fn build_binop_const(name: &str, op: BinOp, lhs: i64, rhs: i64) -> Function {
        let v0 = ValueId(0);
        Function {
            name: name.into(),
            params: Vec::new(),
            ret: Type::I64,
            values: vec![ValueInfo {
                ty: Type::I64,
                name: Some("v0".into()),
            }],
            blocks: vec![Block {
                id: BlockId(0),
                insts: vec![Inst {
                    result: Some(v0),
                    kind: InstKind::BinOp(op, Operand::ConstI64(lhs), Operand::ConstI64(rhs)),
                    origin: None,
                }],
                term: Terminator::Ret(Some(Operand::Value(v0))),
            }],
            current_origin: None,
        }
    }

    /// S1 canonical fixture.
    fn build_one_plus_two() -> Function {
        build_binop_const("one_plus_two", BinOp::Add, 1, 2)
    }

    /// Concatenate u32 instruction words into a little-endian byte
    /// stream, matching what `write_u32` emits in production.
    fn words_to_le_bytes(words: &[u32]) -> Vec<u8> {
        let mut out = Vec::with_capacity(words.len() * 4);
        for w in words {
            out.extend_from_slice(&w.to_le_bytes());
        }
        out
    }

    /// S1 acceptance — `fn one_plus_two() -> i64 { 1 + 2 }` shape
    /// compiles end-to-end to the hand-encoded 16-byte reference:
    ///
    /// ```text
    ///   MOVZ x9, #1       0xD2800029
    ///   MOVZ x10, #2      0xD280004A
    ///   ADD x0, x9, x10   0x8B0A0120
    ///   RET               0xD65F03C0
    /// ```
    #[test]
    fn one_plus_two_byte_equal_reference() {
        let func = build_one_plus_two();
        let compiled = compile_function(&func);

        let expected_words: [u32; 4] = [0xD280_0029, 0xD280_004A, 0x8B0A_0120, 0xD65F_03C0];
        let mut expected_bytes = Vec::with_capacity(16);
        for w in expected_words {
            expected_bytes.extend_from_slice(&w.to_le_bytes());
        }

        assert_eq!(
            compiled.bytes, expected_bytes,
            "byte stream mismatch — expected {:02X?}, got {:02X?}",
            expected_bytes, compiled.bytes
        );
        assert_eq!(compiled.name, "one_plus_two");
        assert!(compiled.relocs.is_empty(), "S1 has no relocs");
        assert!(
            compiled.frame.is_trivial(),
            "leaf fn must have trivial frame"
        );
    }

    #[test]
    fn one_plus_two_emits_exactly_16_bytes() {
        let func = build_one_plus_two();
        let compiled = compile_function(&func);
        assert_eq!(compiled.bytes.len(), 16, "4 instructions × 4 bytes");
    }

    // --- S2-A: integer BinOp surface ---

    /// MOVZ x9,#lhs / MOVZ x10,#rhs / <op> x0,x9,x10 / RET
    /// Helper to assert that a `lhs <op> rhs` fixture lowers to the
    /// 4-word reference sequence given a single-instruction op word.
    fn assert_binop_4word(name: &str, op: BinOp, lhs: i64, rhs: i64, op_word: u32) {
        let func = build_binop_const(name, op, lhs, rhs);
        let compiled = compile_function(&func);
        let expected = words_to_le_bytes(&[
            crate::enc::movz_imm(Gpr::X9, lhs as u16, 0),
            crate::enc::movz_imm(Gpr::X10, rhs as u16, 0),
            op_word,
            crate::enc::ret(Gpr::X30),
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
            "five_minus_three",
            BinOp::Sub,
            5,
            3,
            crate::enc::sub_reg(Gpr::X0, Gpr::X9, Gpr::X10),
        );
    }

    #[test]
    fn two_times_three() {
        assert_binop_4word(
            "two_times_three",
            BinOp::Mul,
            2,
            3,
            crate::enc::mul_reg(Gpr::X0, Gpr::X9, Gpr::X10),
        );
    }

    #[test]
    fn ten_div_three() {
        assert_binop_4word(
            "ten_div_three",
            BinOp::SDiv,
            10,
            3,
            crate::enc::sdiv_reg(Gpr::X0, Gpr::X9, Gpr::X10),
        );
    }

    #[test]
    fn ff_and_f0() {
        assert_binop_4word(
            "ff_and_f0",
            BinOp::And,
            0xFF,
            0xF0,
            crate::enc::and_reg(Gpr::X0, Gpr::X9, Gpr::X10),
        );
    }

    #[test]
    fn zero_f_or_f0() {
        assert_binop_4word(
            "zero_f_or_f0",
            BinOp::Or,
            0x0F,
            0xF0,
            crate::enc::orr_reg(Gpr::X0, Gpr::X9, Gpr::X10),
        );
    }

    #[test]
    fn aa_xor_55() {
        assert_binop_4word(
            "aa_xor_55",
            BinOp::Xor,
            0xAA,
            0x55,
            crate::enc::eor_reg(Gpr::X0, Gpr::X9, Gpr::X10),
        );
    }

    #[test]
    fn one_shl_three() {
        assert_binop_4word(
            "one_shl_three",
            BinOp::Shl,
            1,
            3,
            crate::enc::lslv_reg(Gpr::X0, Gpr::X9, Gpr::X10),
        );
    }

    #[test]
    fn sixteen_ashr_two() {
        assert_binop_4word(
            "sixteen_ashr_two",
            BinOp::AShr,
            16,
            2,
            crate::enc::asrv_reg(Gpr::X0, Gpr::X9, Gpr::X10),
        );
    }

    #[test]
    fn sixteen_lshr_two() {
        assert_binop_4word(
            "sixteen_lshr_two",
            BinOp::LShr,
            16,
            2,
            crate::enc::lsrv_reg(Gpr::X0, Gpr::X9, Gpr::X10),
        );
    }

    /// SRem lowers to SDIV scratch + MSUB Rd — emits *5* words total,
    /// not 4 like the simple BinOps. Verify byte-equal.
    #[test]
    fn ten_rem_three_compound() {
        let func = build_binop_const("ten_rem_three", BinOp::SRem, 10, 3);
        let compiled = compile_function(&func);
        let expected = words_to_le_bytes(&[
            crate::enc::movz_imm(Gpr::X9, 10, 0),
            crate::enc::movz_imm(Gpr::X10, 3, 0),
            crate::enc::sdiv_reg(Gpr::X11, Gpr::X9, Gpr::X10),
            crate::enc::msub_reg(Gpr::X0, Gpr::X11, Gpr::X10, Gpr::X9),
            crate::enc::ret(Gpr::X30),
        ]);
        assert_eq!(
            compiled.bytes, expected,
            "SRem expected 5-word SDIV+MSUB+RET, got {:02X?}",
            compiled.bytes
        );
        assert_eq!(compiled.bytes.len(), 20, "SRem fixture emits 5 inst");
    }

    // --- S2-A: 64-bit ConstI64 emission via MOVZ + MOVK chain ---

    #[test]
    fn const_zero_emits_single_movz() {
        let mut bytes = Vec::new();
        materialize_const_i64(&mut bytes, Gpr::X9, 0);
        assert_eq!(bytes.len(), 4, "ConstI64(0) is single MOVZ");
        let word = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
        // MOVZ x9, #0 = 0xD280_0009
        assert_eq!(word, 0xD280_0009);
    }

    #[test]
    fn const_low_quadrant_only_emits_single_movz() {
        let mut bytes = Vec::new();
        materialize_const_i64(&mut bytes, Gpr::X9, 0xABCD);
        assert_eq!(bytes.len(), 4, "ConstI64(0xABCD) fits in q0");
        let word = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
        // MOVZ x9, #0xABCD = 0xD280_0000 | (0xABCD << 5) | 9 = 0xD2815_79A9
        // 0xABCD << 5 = 0x1579A0 → | 9 → 0x1579A9
        // OR 0xD2800000 → 0xD2815_79A9 (5 nibbles in upper word).
        assert_eq!(word, 0xD295_79A9);
    }

    #[test]
    fn const_large_emits_movz_plus_three_movk() {
        let mut bytes = Vec::new();
        materialize_const_i64(&mut bytes, Gpr::X9, 0x1234_5678_9ABC_DEF0_i64);
        assert_eq!(bytes.len(), 16, "all 4 quadrants non-zero → 4 inst");
        let w0 = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
        let w1 = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
        let w2 = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
        let w3 = u32::from_le_bytes(bytes[12..16].try_into().unwrap());
        assert_eq!(w0, crate::enc::movz_imm(Gpr::X9, 0xDEF0, 0));
        assert_eq!(w1, crate::enc::movk_imm(Gpr::X9, 0x9ABC, 1));
        assert_eq!(w2, crate::enc::movk_imm(Gpr::X9, 0x5678, 2));
        assert_eq!(w3, crate::enc::movk_imm(Gpr::X9, 0x1234, 3));
    }

    #[test]
    fn const_neg_one_emits_full_chain() {
        // -1_i64 as u64 = 0xFFFF_FFFF_FFFF_FFFF — all 4 quadrants
        // are 0xFFFF, all non-zero → 4 instructions. MOVN-shortcut
        // is a future S3+ peephole (RFC L3b).
        let mut bytes = Vec::new();
        materialize_const_i64(&mut bytes, Gpr::X9, -1);
        assert_eq!(bytes.len(), 16);
    }

    #[test]
    fn const_skips_zero_quadrants() {
        // value = 0x0000_5678_0000_DEF0 — only q0 and q2 set.
        // Emit MOVZ q0 + MOVK q2 → 2 inst total (q1=0 and q3=0
        // skipped).
        let mut bytes = Vec::new();
        materialize_const_i64(&mut bytes, Gpr::X9, 0x0000_5678_0000_DEF0_i64);
        assert_eq!(bytes.len(), 8, "q1=0 and q3=0 emit nothing");
        let w0 = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
        let w1 = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
        assert_eq!(w0, crate::enc::movz_imm(Gpr::X9, 0xDEF0, 0));
        assert_eq!(w1, crate::enc::movk_imm(Gpr::X9, 0x5678, 2));
    }
}
