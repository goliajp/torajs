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

use torajs_core::ssa::{BinOp, Function, InstKind, Operand, Terminator};

use crate::enc::{add_reg, movz_imm, ret};
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
                _ => todo!("S2: BinOp::{:?}", op),
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
            // S1: small-int fast path — MOVZ if fits in 16 unsigned,
            // else fall through. Full 64-bit constant emission via
            // MOVZ+MOVK chain lands in S2.
            if (0..=u16::MAX as i64).contains(c) {
                write_u32(bytes, movz_imm(scratch, *c as u16, 0));
                scratch
            } else {
                todo!("S2: ConstI64({}) outside 0..=u16::MAX", c);
            }
        }
        other => todo!("S2+: Operand::{:?}", other),
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

    /// Build the canonical S1 fixture:
    /// `fn one_plus_two() -> i64 { 1 + 2 }`
    fn build_one_plus_two() -> Function {
        let v0 = ValueId(0);
        Function {
            name: "one_plus_two".into(),
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
                    kind: InstKind::BinOp(BinOp::Add, Operand::ConstI64(1), Operand::ConstI64(2)),
                    origin: None,
                }],
                term: Terminator::Ret(Some(Operand::Value(v0))),
            }],
            current_origin: None,
        }
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
}
