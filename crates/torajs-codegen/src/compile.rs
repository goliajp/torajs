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
//!
//! S2-B2 coverage: float `BinOp::F{Add,Sub,Mul,Div}` via D-form
//! aarch64 FP ops. `Operand::ConstF64` materializes by writing the
//! `to_bits()` u64 to a GPR scratch via MOVZ+MOVK chain, then FMOV-d-
//! from-x into an FPR scratch. F64-typed return values go to V0/D0
//! per AAPCS64 §5.1.2. `InstKind::BitCastF64ToI64` and the inverse
//! both lower to a single FMOV between Xn and Dn — no bit conversion
//! happens, just a reinterpret. FRem still routes to `todo!("S2-D")`
//! pending the Call-surface work that the libm `fmod` lowering needs.

use torajs_core::ssa::{BinOp, Function, InstKind, Operand, Terminator};

use crate::enc::{
    add_reg, and_reg, asrv_reg, eor_reg, fadd_d, fdiv_d, fmov_d_from_x, fmov_x_from_d, fmul_d,
    fsub_d, lslv_reg, lsrv_reg, movk_imm, movz_imm, msub_reg, mul_reg, orr_reg, ret, sdiv_reg,
    sub_reg,
};
use crate::frame::FrameLayout;
use crate::reg::{Fpr, Gpr};
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

/// FPR scratches for materializing `Operand::ConstF64` and feeding
/// FP binops. Same convention as the GPR scratches: LHS / RHS for
/// the two operands of a 2-source op.
const FP_SCRATCH_LHS: Fpr = Fpr::V16;
const FP_SCRATCH_RHS: Fpr = Fpr::V17;

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
        InstKind::BinOp(op, lhs, rhs) => emit_binop(bytes, inst, op, lhs, rhs, alloc),
        InstKind::BitCastF64ToI64(src) => {
            let dst = inst
                .result
                .map(|v| alloc.of(v).as_gpr())
                .expect("BitCastF64ToI64 must have a result");
            let src_fpr =
                materialize_operand_fpr(bytes, src, FP_SCRATCH_LHS, OP_SCRATCH_LHS, alloc);
            write_u32(bytes, fmov_x_from_d(dst, src_fpr));
        }
        InstKind::BitCastI64ToF64(src) => {
            let dst = inst
                .result
                .map(|v| alloc.of(v).as_fpr())
                .expect("BitCastI64ToF64 must have a result");
            let src_gpr = materialize_operand_gpr(bytes, src, OP_SCRATCH_LHS, alloc);
            write_u32(bytes, fmov_d_from_x(dst, src_gpr));
        }
        other => todo!("S2+: InstKind::{:?}", other),
    }
}

fn emit_binop(
    bytes: &mut Vec<u8>,
    inst: &torajs_core::ssa::Inst,
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
            let dst = inst
                .result
                .map(|v| alloc.of(v).as_gpr())
                .expect("int BinOp must have a result ValueId");
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
        }
        // --- f64 ops (FPR) ---
        BinOp::FAdd | BinOp::FSub | BinOp::FMul | BinOp::FDiv => {
            let dst = inst
                .result
                .map(|v| alloc.of(v).as_fpr())
                .expect("fp BinOp must have a result ValueId");
            let rn = materialize_operand_fpr(bytes, lhs, FP_SCRATCH_LHS, OP_SCRATCH_LHS, alloc);
            let rm = materialize_operand_fpr(bytes, rhs, FP_SCRATCH_RHS, OP_SCRATCH_RHS, alloc);
            match op {
                BinOp::FAdd => write_u32(bytes, fadd_d(dst, rn, rm)),
                BinOp::FSub => write_u32(bytes, fsub_d(dst, rn, rm)),
                BinOp::FMul => write_u32(bytes, fmul_d(dst, rn, rm)),
                BinOp::FDiv => write_u32(bytes, fdiv_d(dst, rn, rm)),
                _ => unreachable!(),
            }
        }
        BinOp::FRem => todo!("S2-D: BinOp::FRem lowers to a libm `fmod` call"),
    }
}

/// Place `op`'s value into `scratch` (if it's an int constant) or
/// return the assigned register (if it's a Value already in a GPR).
fn materialize_operand_gpr(
    bytes: &mut Vec<u8>,
    op: &Operand,
    scratch: Gpr,
    alloc: &Assignment,
) -> Gpr {
    match op {
        Operand::Value(v) => alloc.of(*v).as_gpr(),
        Operand::ConstI64(c) => {
            materialize_const_i64(bytes, scratch, *c);
            scratch
        }
        Operand::ConstI32(c) => {
            materialize_const_i64(bytes, scratch, *c as i64);
            scratch
        }
        Operand::ConstBool(b) => {
            materialize_const_i64(bytes, scratch, *b as i64);
            scratch
        }
        Operand::ConstPtrNull => {
            materialize_const_i64(bytes, scratch, 0);
            scratch
        }
        Operand::ConstF64(_) => panic!(
            "GPR materialization can't hold an f64 — use BitCastF64ToI64 in SSA or call materialize_operand_fpr"
        ),
    }
}

/// Place `op`'s f64 value into an FPR. Constants land via the GPR
/// path (MOVZ+MOVK the bit pattern then FMOV-d-from-x).
fn materialize_operand_fpr(
    bytes: &mut Vec<u8>,
    op: &Operand,
    fpr_scratch: Fpr,
    gpr_scratch: Gpr,
    alloc: &Assignment,
) -> Fpr {
    match op {
        Operand::Value(v) => alloc.of(*v).as_fpr(),
        Operand::ConstF64(c) => {
            // `f64::to_bits()` gives the IEEE 754 binary64 word; the
            // FMOV-d-from-x reinterprets those bits as an f64. No NaN
            // canonicalization happens — the bit pattern survives
            // intact, matching what the LLVM backend currently does.
            let bits = c.to_bits();
            materialize_const_i64(bytes, gpr_scratch, bits as i64);
            write_u32(bytes, fmov_d_from_x(fpr_scratch, gpr_scratch));
            fpr_scratch
        }
        other => panic!("FPR materialization can't hold {other:?}"),
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

    // --- S2-B2: f64 BinOp end-to-end ---

    /// FMul-shape fixture with F64 return type — verifies the ret
    /// value lands in V0/D0 (AAPCS64 §5.1.2) and the operand chain
    /// MOVZ+MOVK → FMOV-d-from-x → FMUL works end-to-end.
    #[test]
    fn fp_mul_two_point_five_times_two_returns_d0() {
        let v0 = ValueId(0);
        let func = Function {
            name: "fp_mul_25_times_2".into(),
            params: Vec::new(),
            ret: Type::F64,
            values: vec![ValueInfo {
                ty: Type::F64,
                name: None,
            }],
            blocks: vec![Block {
                id: BlockId(0),
                insts: vec![Inst {
                    result: Some(v0),
                    kind: InstKind::BinOp(
                        BinOp::FMul,
                        Operand::ConstF64(2.5),
                        Operand::ConstF64(2.0),
                    ),
                    origin: None,
                }],
                term: Terminator::Ret(Some(Operand::Value(v0))),
            }],
            current_origin: None,
        };
        let compiled = compile_function(&func);

        // 2.5_f64.to_bits() = 0x4004_0000_0000_0000 (only q3 non-zero)
        // 2.0_f64.to_bits() = 0x4000_0000_0000_0000 (only q3 non-zero)
        let expected = words_to_le_bytes(&[
            crate::enc::movz_imm(Gpr::X9, 0, 0),
            crate::enc::movk_imm(Gpr::X9, 0x4004, 3),
            crate::enc::fmov_d_from_x(Fpr::V16, Gpr::X9),
            crate::enc::movz_imm(Gpr::X10, 0, 0),
            crate::enc::movk_imm(Gpr::X10, 0x4000, 3),
            crate::enc::fmov_d_from_x(Fpr::V17, Gpr::X10),
            crate::enc::fmul_d(Fpr::V0, Fpr::V16, Fpr::V17),
            crate::enc::ret(Gpr::X30),
        ]);
        assert_eq!(
            compiled.bytes, expected,
            "fp_mul: expected {expected:02X?}, got {:02X?}",
            compiled.bytes
        );
    }

    /// Mixed-type fixture exercising BitCastF64ToI64 — FAdd produces
    /// an f64, then BitCast moves the bit pattern verbatim to an i64
    /// ret slot via FMOV-x-from-d. Catches the FPR → GPR alloc path.
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
        // v0 → V16 (scratch FPR, first non-ret F64),
        // v1 → X0 (ret slot for I64).
        let expected = words_to_le_bytes(&[
            // FAdd lhs
            crate::enc::movz_imm(Gpr::X9, 0, 0),
            crate::enc::movk_imm(Gpr::X9, 0x3FF8, 3),
            crate::enc::fmov_d_from_x(Fpr::V16, Gpr::X9),
            // FAdd rhs
            crate::enc::movz_imm(Gpr::X10, 0, 0),
            crate::enc::movk_imm(Gpr::X10, 0x4004, 3),
            crate::enc::fmov_d_from_x(Fpr::V17, Gpr::X10),
            // FAdd dst (= V16 = scratch lhs slot — RAW handled by aarch64 within-inst aliasing)
            crate::enc::fadd_d(Fpr::V16, Fpr::V16, Fpr::V17),
            // BitCastF64ToI64 — FMOV x0, d16
            crate::enc::fmov_x_from_d(Gpr::X0, Fpr::V16),
            crate::enc::ret(Gpr::X30),
        ]);
        assert_eq!(
            compiled.bytes, expected,
            "fp_add+bitcast: expected {expected:02X?}, got {:02X?}",
            compiled.bytes
        );
        assert_eq!(compiled.bytes.len(), 36, "9 inst × 4 bytes");
    }
}
