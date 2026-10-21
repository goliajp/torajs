//! Operand materialization — int / fp constants → register, or
//! Value(ValueId) → looked-up assigned register / LDR-from-spill.

use torajs_core::ssa::Operand;

use super::{FP_SCRATCH_LHS, FP_SCRATCH_RHS, OP_SCRATCH_LHS, OP_SCRATCH_RHS, write_u32};
use crate::enc::{
    fcmp_d, fcmp_d_zero, fmov_d_from_x, logical_imm, movk_imm, movn_imm, movz_imm, orr_imm,
};
use crate::reg::{Fpr, Gpr, Reg};
use crate::regalloc::Assignment;

/// Classify an Operand as f64 vs 64-bit GPR-shaped.
///
/// Store / call-arg passing don't carry a `Type` in `InstKind` — the
/// type comes from the value operand. `ConstF64` is trivially f64;
/// for `Operand::Value(v)` we ask the allocator which register class
/// holds the SSA value (`Fpr` / `SpillFpr` = f64). Other constants
/// (int / bool / ptr-null) stay on the GPR path.
pub(super) fn operand_is_f64(op: &Operand, alloc: &Assignment) -> bool {
    match op {
        Operand::ConstF64(_) => true,
        Operand::Value(v) => matches!(alloc.of(*v), Reg::Fpr(_) | Reg::SpillFpr(_)),
        _ => false,
    }
}

/// Place `op`'s value into `scratch` (if it's an int constant or a
/// spilled SSA value) or return the assigned register (if it's a
/// Value already in a real GPR).
///
/// LS-3 spill path: when `Operand::Value(v)` resolves to
/// `Reg::SpillGpr(off)`, emits `LDR scratch, [SP, #off]` and returns
/// `scratch` — or, for an alloca result, recomputes its slot address
/// (`ADD scratch, SP, #slot`), which [`super::mem::emit_alloca`]
/// never stored. The caller's previously-materialized scratches are
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
                match alloc.alloca_slot(*v) {
                    Some(slot) => super::slot::sp_add(bytes, scratch, slot),
                    None => super::slot::sp_ldr_x(bytes, scratch, off),
                }
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

/// Set NZCV from `lhs` vs `rhs`, both f64. A zero on the right takes
/// `fcmp dn, #0.0`: nothing to carry into an FP scratch, and a -0.0
/// there compares exactly as +0.0 does.
pub fn compare_fpr(bytes: &mut Vec<u8>, lhs: &Operand, rhs: &Operand, alloc: &Assignment) {
    let rn = materialize_operand_fpr(bytes, lhs, FP_SCRATCH_LHS, OP_SCRATCH_LHS, alloc);
    if is_f64_zero(rhs) {
        write_u32(bytes, fcmp_d_zero(rn));
    } else {
        let rm = materialize_operand_fpr(bytes, rhs, FP_SCRATCH_RHS, OP_SCRATCH_RHS, alloc);
        write_u32(bytes, fcmp_d(rn, rm));
    }
}

pub fn is_f64_zero(op: &Operand) -> bool {
    matches!(op, Operand::ConstF64(z) if *z == 0.0)
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
                super::slot::sp_ldr_d(bytes, fpr_scratch, off);
                fpr_scratch
            }
            other => panic!(
                "materialize_operand_fpr called on ValueId({}) holding {other:?}",
                v.0
            ),
        },
        Operand::ConstF64(c) => {
            // +0.0 moves from xzr; fmov #imm stays unused, it measured slower
            if c.to_bits() == 0 {
                write_u32(bytes, fmov_d_from_x(fpr_scratch, Gpr::XZR));
                return fpr_scratch;
            }
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

/// Materialize an arbitrary i64 into `dst` as one MOVZ or MOVN
/// followed by a MOVK per quadrant the first instruction leaves
/// wrong. Whichever of the all-zero and all-ones quadrants are more
/// numerous picks the base — MOVZ fills the rest with zeros, MOVN
/// with ones — so `-1` and other dense-ones constants are one
/// instruction, the same choice LLVM's `expandMOVImm` makes; the tie
/// goes to MOVZ, and `0` is `MOVZ dst, #0`. A constant that would take
/// two or more of those and is a bitmask immediate (a repeating lane
/// pattern such as `0x7f7f..`) is one `ORR dst, XZR, #imm` instead,
/// as LLVM also prefers.
pub fn materialize_const_i64(bytes: &mut Vec<u8>, dst: Gpr, value: i64) {
    let u = value as u64;
    let quads: [u16; 4] = core::array::from_fn(|i| (u >> (16 * i)) as u16);
    let zeros = quads.iter().filter(|&&q| q == 0).count();
    let ones = quads.iter().filter(|&&q| q == 0xFFFF).count();
    if zeros.max(ones) < 3
        && let Some(f) = logical_imm(u)
    {
        write_u32(bytes, orr_imm(dst, Gpr::XZR, f));
        return;
    }
    let (skip, first): (u16, fn(Gpr, u16, u8) -> u32) = if ones > zeros {
        (0xFFFF, |rd, q, hw| movn_imm(rd, !q, hw))
    } else {
        (0, movz_imm)
    };
    let mut placed = false;
    for (hw, &q) in quads.iter().enumerate() {
        if q == skip {
            continue;
        }
        if placed {
            write_u32(bytes, movk_imm(dst, q, hw as u8));
        } else {
            write_u32(bytes, first(dst, q, hw as u8));
            placed = true;
        }
    }
    if !placed {
        write_u32(bytes, first(dst, skip, 0));
    }
}

/// A copy the allocator coalesced: both sides share one home, spill
/// slot included, so it emits nothing.
pub(crate) fn copy_emits_nothing(inst: &torajs_core::ssa::Inst, alloc: &Assignment) -> bool {
    use torajs_core::ssa::{InstKind, Type};
    match (&inst.kind, inst.result) {
        (InstKind::Copy(ty, Operand::Value(v)), Some(r)) => {
            !matches!(ty, Type::Vec(_)) && alloc.contains(*v) && alloc.of(*v) == alloc.of(r)
        }
        _ => false,
    }
}

/// `%v = copy <ty> <op>` — materialize the source and move it into
/// the result's assigned home. mem2reg's φ destruction emits several
/// `Copy`s defining the SAME result ValueId on different predecessor
/// blocks; liveness merges those defs into one interval, so they all
/// resolve to one home here and the move semantics are exactly a
/// virtual-register write (LLVM PHIElimination's mov shape). `ty`
/// picks the register class: `F64` rides the FPR path, everything
/// else (i64 / ptr-shaped handles) the GPR path.
pub fn emit_copy(
    bytes: &mut Vec<u8>,
    inst: &torajs_core::ssa::Inst,
    ty: torajs_core::ssa::Type,
    op: &Operand,
    alloc: &Assignment,
) {
    use super::{
        FP_SCRATCH_LHS, FP_SCRATCH_RESULT, OP_SCRATCH_LHS, OP_SCRATCH_RESULT_GPR,
        write_def_spill_fpr, write_def_spill_gpr,
    };
    use crate::enc::mov_d_reg;
    use crate::enc::mov_x_reg;
    let result_vid = inst.result.expect("Copy must have a result ValueId");
    if copy_emits_nothing(inst, alloc) {
        return;
    }
    if matches!(ty, torajs_core::ssa::Type::F64) {
        let (dst, spill_off) = alloc.def_fpr(result_vid, FP_SCRATCH_RESULT);
        let src = materialize_operand_fpr(bytes, op, FP_SCRATCH_LHS, OP_SCRATCH_LHS, alloc);
        // A spilled result's home is its slot: store the source there
        // rather than moving it through the scratch register first.
        if spill_off.is_some() {
            write_def_spill_fpr(bytes, spill_off, src);
            return;
        }
        if src != dst {
            write_u32(bytes, mov_d_reg(dst, src));
        }
    } else {
        let (dst, spill_off) = alloc.def_gpr(result_vid, OP_SCRATCH_RESULT_GPR);
        // a register result is written by this copy alone, so a constant
        // or a spilled source lands in it directly instead of passing
        // through a scratch register and a move
        let scratch = if spill_off.is_some() {
            OP_SCRATCH_LHS
        } else {
            dst
        };
        let src = materialize_operand_gpr(bytes, op, scratch, alloc);
        if spill_off.is_some() {
            write_def_spill_gpr(bytes, spill_off, src);
            return;
        }
        if src != dst {
            write_u32(bytes, mov_x_reg(dst, src));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::compile_function;
    use super::*;
    use crate::enc;
    use torajs_core::ssa::{
        Block, BlockId, Function, Inst, InstKind, Terminator, Type, ValueId, ValueInfo,
    };

    fn copy_ret_fixture(ty: Type, op: Operand) -> Function {
        let v0 = ValueId(0);
        Function {
            name: "copy_fix".into(),
            params: Vec::new(),
            ret: ty,
            values: vec![ValueInfo { ty, name: None }],
            blocks: vec![Block {
                id: BlockId(0),
                insts: vec![Inst {
                    result: Some(v0),
                    kind: InstKind::Copy(ty, op),
                    origin: None,
                }],
                term: Terminator::Ret(Some(Operand::Value(v0))),
            }],
            current_origin: None,
        }
    }

    #[test]
    fn copy_const_i64_emits_movz_into_its_home() {
        // %0 = copy i64 #5; ret %0 → MOVZ x0,#5 / RET
        let compiled = compile_function(&copy_ret_fixture(Type::I64, Operand::ConstI64(5)));
        let expected: Vec<u8> = [enc::movz_imm(Gpr::X0, 5, 0), enc::ret(Gpr::X30)]
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect();
        assert_eq!(compiled.bytes, expected);
    }

    #[test]
    fn copy_const_f64_rides_fpr_path() {
        // %0 = copy f64 #1.5; ret %0 — 1.5 bits = 0x3FF8 << 48:
        // MOVZ x9,#0x3FF8,lsl#48 / FMOV d16,x9 / FMOV d0,d16 / RET
        let compiled = compile_function(&copy_ret_fixture(Type::F64, Operand::ConstF64(1.5)));
        let expected: Vec<u8> = [
            enc::movz_imm(Gpr::X9, 0x3FF8, 3),
            enc::fmov_d_from_x(Fpr::V16, Gpr::X9),
            enc::mov_d_reg(Fpr::V0, Fpr::V16),
            enc::ret(Gpr::X30),
        ]
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect();
        assert_eq!(compiled.bytes, expected);
    }

    #[test]
    fn copy_const_f64_zero_moves_from_the_zero_register() {
        // +0.0: FMOV d16,xzr, no MOVZ; -0.0 keeps the MOVZ (sign bit)
        let words = |c: f64| -> Vec<u32> {
            let compiled = compile_function(&copy_ret_fixture(Type::F64, Operand::ConstF64(c)));
            (compiled.bytes.as_chunks::<4>().0.iter())
                .map(|c| u32::from_le_bytes(*c))
                .collect()
        };
        assert_eq!(words(0.0)[0], enc::fmov_d_from_x(Fpr::V16, Gpr::XZR));
        assert_eq!(words(-0.0)[0], enc::movz_imm(Gpr::X9, 0x8000, 3));
    }

    #[test]
    fn const_bitmask_pattern_emits_single_orr() {
        // 0x7f7f.. would take MOVZ + 3 MOVK; as a bitmask immediate it is
        // one ORR x9, xzr, #imm
        let mut bytes = Vec::new();
        materialize_const_i64(&mut bytes, Gpr::X9, 0x7f7f_7f7f_7f7f_7f7f);
        assert_eq!(bytes.len(), 4);
        let word = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
        let f = enc::logical_imm(0x7f7f_7f7f_7f7f_7f7f).unwrap();
        assert_eq!(word, enc::orr_imm(Gpr::X9, Gpr::XZR, f));
        // a pattern one MOVZ already covers keeps the MOVZ
        let mut bytes = Vec::new();
        materialize_const_i64(&mut bytes, Gpr::X9, 0x8000_0000_0000_0000_u64 as i64);
        assert_eq!(bytes.len(), 4);
        let word = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
        assert_eq!(word, enc::movz_imm(Gpr::X9, 0x8000, 3));
    }

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

    /// The value the MOVZ / MOVN / MOVK words in `bytes` leave in the
    /// register, as the core computes it.
    #[allow(clippy::unusual_byte_groupings)] // sf_opc_opcode groups mirror the A64 encoding
    fn decode_mov_chain(bytes: &[u8]) -> u64 {
        let mut acc = 0u64;
        for w in bytes.chunks(4) {
            let word = u32::from_le_bytes(w.try_into().unwrap());
            let imm = ((word >> 5) & 0xFFFF) as u64;
            let shift = ((word >> 21) & 3) * 16;
            match word >> 23 {
                0b1_00_100101 => acc = !(imm << shift),
                0b1_10_100101 => acc = imm << shift,
                0b1_11_100101 => acc = (acc & !(0xFFFF << shift)) | (imm << shift),
                _ => panic!("not a wide-immediate move: {word:#010x}"),
            }
        }
        acc
    }

    #[test]
    fn const_picks_the_denser_base_and_decodes_back() {
        for (value, words) in [
            (-1i64, 1usize),
            (-2, 1),
            (-0x1_0000, 1),
            (0xFFFF_FFFF_0000_FFFF_u64 as i64, 1),
            (0xFFFF_0000_FFFF_FFFF_u64 as i64, 1),
            (0xFFFF_0000_FFFF_1234_u64 as i64, 2),
            (0x0000_FFFF_0000_1234_u64 as i64, 2),
            (0x1234_FFFF_0000_0000_u64 as i64, 2),
            (0x1234_5678_9ABC_DEF0, 4),
            (0, 1),
            (0xABCD, 1),
            (i64::MIN, 1),
        ] {
            let mut bytes = Vec::new();
            materialize_const_i64(&mut bytes, Gpr::X9, value);
            assert_eq!(bytes.len(), 4 * words, "{value:#x}");
            assert_eq!(decode_mov_chain(&bytes), value as u64, "{value:#x}");
        }
    }

    #[test]
    fn const_neg_one_is_one_movn() {
        let mut bytes = Vec::new();
        materialize_const_i64(&mut bytes, Gpr::X9, -1);
        assert_eq!(bytes, movn_imm(Gpr::X9, 0, 0).to_le_bytes());
    }

    #[test]
    fn const_high_quadrant_only_is_one_shifted_movz() {
        let mut bytes = Vec::new();
        materialize_const_i64(&mut bytes, Gpr::X9, 0x3FF8_0000_0000_0000);
        assert_eq!(bytes, movz_imm(Gpr::X9, 0x3FF8, 3).to_le_bytes());
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
