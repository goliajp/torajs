//! Operand materialization — int / fp constants → register, or
//! Value(ValueId) → looked-up assigned register / LDR-from-spill.

use torajs_core::ssa::Operand;

use super::write_u32;
use crate::enc::{fmov_d_from_x, ldr_d_imm12, ldr_x_imm12, movk_imm, movz_imm};
use crate::reg::{Fpr, Gpr, Reg};
use crate::regalloc::Assignment;

/// Place `op`'s value into `scratch` (if it's an int constant or a
/// spilled SSA value) or return the assigned register (if it's a
/// Value already in a real GPR).
///
/// LS-3 spill path: when `Operand::Value(v)` resolves to
/// `Reg::SpillGpr(off)`, emits `LDR scratch, [SP, #off]` and returns
/// `scratch`. The caller's previously-materialized scratches are
/// expected to be disjoint (e.g. binop calls
/// `materialize_operand_gpr(lhs, OP_SCRATCH_LHS, ...)` then
/// `materialize_operand_gpr(rhs, OP_SCRATCH_RHS, ...)` so an LDR
/// into one doesn't clobber the other).
pub fn materialize_operand_gpr(
    bytes: &mut Vec<u8>,
    op: &Operand,
    scratch: Gpr,
    alloc: &Assignment,
) -> Gpr {
    match op {
        Operand::Value(v) => match alloc.of(*v) {
            Reg::Gpr(g) => g,
            Reg::SpillGpr(off) => {
                write_u32(bytes, ldr_x_imm12(scratch, Gpr::SP, off));
                scratch
            }
            other => panic!(
                "materialize_operand_gpr called on ValueId({}) holding {other:?}",
                v.0
            ),
        },
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
/// path (MOVZ+MOVK the bit pattern then FMOV-d-from-x). Spilled
/// values land via `LDR Dscratch, [SP, #off]`.
pub fn materialize_operand_fpr(
    bytes: &mut Vec<u8>,
    op: &Operand,
    fpr_scratch: Fpr,
    gpr_scratch: Gpr,
    alloc: &Assignment,
) -> Fpr {
    match op {
        Operand::Value(v) => match alloc.of(*v) {
            Reg::Fpr(f) => f,
            Reg::SpillFpr(off) => {
                write_u32(bytes, ldr_d_imm12(fpr_scratch, Gpr::SP, off));
                fpr_scratch
            }
            other => panic!(
                "materialize_operand_fpr called on ValueId({}) holding {other:?}",
                v.0
            ),
        },
        Operand::ConstF64(c) => {
            // `f64::to_bits()` gives the IEEE 754 binary64 word; the
            // FMOV-d-from-x reinterprets those bits as an f64. No NaN
            // canonicalization — the bit pattern survives intact,
            // matching the LLVM-backend behavior.
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
pub fn materialize_const_i64(bytes: &mut Vec<u8>, dst: Gpr, value: i64) {
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

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(w0, movz_imm(Gpr::X9, 0xDEF0, 0));
        assert_eq!(w1, movk_imm(Gpr::X9, 0x9ABC, 1));
        assert_eq!(w2, movk_imm(Gpr::X9, 0x5678, 2));
        assert_eq!(w3, movk_imm(Gpr::X9, 0x1234, 3));
    }

    #[test]
    fn const_neg_one_emits_full_chain() {
        let mut bytes = Vec::new();
        materialize_const_i64(&mut bytes, Gpr::X9, -1);
        assert_eq!(bytes.len(), 16);
    }

    #[test]
    fn const_skips_zero_quadrants() {
        let mut bytes = Vec::new();
        materialize_const_i64(&mut bytes, Gpr::X9, 0x0000_5678_0000_DEF0_i64);
        assert_eq!(bytes.len(), 8, "q1=0 and q3=0 emit nothing");
        let w0 = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
        let w1 = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
        assert_eq!(w0, movz_imm(Gpr::X9, 0xDEF0, 0));
        assert_eq!(w1, movk_imm(Gpr::X9, 0x5678, 2));
    }
}
