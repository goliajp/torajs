//! `InstKind::BoxAny(tag, v)` — the NaN-box encode of a value whose
//! `AnySlotTag` is known at compile time, in place of a call to
//! `__torajs_anyv_box_from_pair`, whose table [`encode_const`] follows
//! for a constant operand.
//!
//! Hot arms (`v` the operand, `d` the result, X10 / X11 / V16 scratch;
//! `d` may share `v`'s register, so every sequence reads `v` before it
//! writes `d`):
//!
//! ```text
//! int (i64)   cmp v, wv, sxtw ; b.ne wide ; mov w10, wv ; orr d, x10, #TAG ; b done
//!       wide: scvtf d16, v ; fmov x10, d16 ; movz x11, #7, lsl 48 ; add d, x10, x11
//! int (i32)   mov w10, wv ; orr d, x10, #TAG
//! double      movz x10, #7, lsl 48 ; add d, v, x10 ; lsr x10, d, #49 ; cbz x10, nan
//!             eor x11, d, #TAG ; lsr x11, x11, #48 ; cbz x11, nan ; b done
//!        nan: movz d, #0x7FFF, lsl 48
//! heap        cmp v, #0 ; movz x10, #NULL ; csel d, x10, v, eq
//! bool        cmp v, #0 ; cset x10, ne ; orr d, x10, #FALSE
//! null/undef  movz d, #NULL / #UNDEFINED
//! ```
//!
//! An integer converted to a double is never a NaN, so the int arm's
//! wide path needs no collision check; the double arm's predicate is
//! `is_double` bit for bit (top 15 bits not all clear, top 16 bits not
//! the number tag) and a miss is the canonical NaN.

use torajs_anyvalue::nanbox::{
    DOUBLE_ENCODE_OFFSET, TAG_TYPE_NUMBER, VALUE_FALSE, VALUE_NULL, VALUE_UNDEFINED, box_bool,
    box_double,
};
use torajs_core::ssa::{Function, Inst, Operand, Type};

use super::operand::{materialize_const_i64, materialize_operand_gpr};
use super::{
    FP_SCRATCH_LHS, OP_SCRATCH_LHS, OP_SCRATCH_RESULT_GPR, OP_SCRATCH_RHS, OP_SCRATCH_TMP,
    write_def_spill_gpr, write_u32,
};
use crate::enc::{
    add_reg, b_cond_imm19, b_imm26, cbz_x, cmp_imm, cmp_x_w_sxtw, cond, csel_cond, cset_cond,
    eor_imm, fmov_x_from_d, logical_imm, lsr_imm, mov_w_reg, movz_imm, orr_imm, scvtf_d_x,
};
use crate::regalloc::Assignment;

const TAG_BOOL: i64 = 1;
const TAG_INT: i64 = 2;
const TAG_DOUBLE: i64 = 3;
const TAG_HEAP: i64 = 4;

pub fn emit_box_any(
    bytes: &mut Vec<u8>,
    func: &Function,
    inst: &Inst,
    tag: i64,
    op: &Operand,
    alloc: &Assignment,
) {
    let result = inst.result.expect("box_any defines a value");
    let (d, spill_off) = alloc.def_gpr(result, OP_SCRATCH_RESULT_GPR);
    if let Some(bits) = const_payload(op) {
        materialize_const_i64(bytes, d, encode_const(tag, bits) as i64);
        write_def_spill_gpr(bytes, spill_off, d);
        return;
    }
    let tag_imm = logical_imm(TAG_TYPE_NUMBER).expect("the number tag is a bitmask immediate");
    match tag {
        TAG_INT => {
            let v = materialize_operand_gpr(bytes, op, OP_SCRATCH_LHS, alloc);
            let narrow = matches!(op, Operand::Value(x) if func.value_type(*x) == Type::I32);
            if !narrow {
                write_u32(bytes, cmp_x_w_sxtw(v, v));
                write_u32(bytes, b_cond_imm19(cond::NE, 16));
            }
            write_u32(bytes, mov_w_reg(OP_SCRATCH_RHS, v));
            write_u32(bytes, orr_imm(d, OP_SCRATCH_RHS, tag_imm));
            if !narrow {
                write_u32(bytes, b_imm26(20));
                write_u32(bytes, scvtf_d_x(FP_SCRATCH_LHS, v));
                write_u32(bytes, fmov_x_from_d(OP_SCRATCH_RHS, FP_SCRATCH_LHS));
                write_u32(bytes, movz_imm(OP_SCRATCH_TMP, offset_hi16(), 3));
                write_u32(bytes, add_reg(d, OP_SCRATCH_RHS, OP_SCRATCH_TMP));
            }
        }
        TAG_DOUBLE => {
            let v = materialize_operand_gpr(bytes, op, OP_SCRATCH_LHS, alloc);
            write_u32(bytes, movz_imm(OP_SCRATCH_RHS, offset_hi16(), 3));
            write_u32(bytes, add_reg(d, v, OP_SCRATCH_RHS));
            write_u32(bytes, lsr_imm(OP_SCRATCH_RHS, d, 49));
            write_u32(bytes, cbz_x(OP_SCRATCH_RHS, 20));
            write_u32(bytes, eor_imm(OP_SCRATCH_TMP, d, tag_imm));
            write_u32(bytes, lsr_imm(OP_SCRATCH_TMP, OP_SCRATCH_TMP, 48));
            write_u32(bytes, cbz_x(OP_SCRATCH_TMP, 8));
            write_u32(bytes, b_imm26(8));
            write_u32(bytes, movz_imm(d, canonical_nan_hi16(), 3));
        }
        TAG_HEAP => {
            let v = materialize_operand_gpr(bytes, op, OP_SCRATCH_LHS, alloc);
            write_u32(bytes, cmp_imm(v, 0));
            write_u32(bytes, movz_imm(OP_SCRATCH_RHS, VALUE_NULL as u16, 0));
            write_u32(bytes, csel_cond(d, OP_SCRATCH_RHS, v, cond::EQ));
        }
        TAG_BOOL => {
            let v = materialize_operand_gpr(bytes, op, OP_SCRATCH_LHS, alloc);
            let false_imm = logical_imm(VALUE_FALSE).expect("false is a bitmask immediate");
            write_u32(bytes, cmp_imm(v, 0));
            write_u32(bytes, cset_cond(OP_SCRATCH_RHS, cond::NE));
            write_u32(bytes, orr_imm(d, OP_SCRATCH_RHS, false_imm));
        }
        // null, undefined and every other tag ignore the payload
        _ => materialize_const_i64(bytes, d, encode_const(tag, 0) as i64),
    }
    write_def_spill_gpr(bytes, spill_off, d);
}

/// `__torajs_anyv_box_from_pair(tag, bits)`, arm for arm.
fn encode_const(tag: i64, bits: i64) -> u64 {
    match tag {
        TAG_BOOL => box_bool(bits != 0),
        TAG_INT => torajs_anyvalue::__torajs_anyv_box_i64(bits),
        TAG_DOUBLE => box_double(f64::from_bits(bits as u64)),
        TAG_HEAP if bits != 0 => bits as u64,
        5 => VALUE_UNDEFINED,
        _ => VALUE_NULL,
    }
}

fn const_payload(op: &Operand) -> Option<i64> {
    match op {
        Operand::ConstI64(c) => Some(*c),
        Operand::ConstI32(c) => Some(*c as i64),
        Operand::ConstBool(b) => Some(*b as i64),
        Operand::ConstPtrNull => Some(0),
        Operand::ConstF64(f) => Some(f.to_bits() as i64),
        _ => None,
    }
}

const fn offset_hi16() -> u16 {
    (DOUBLE_ENCODE_OFFSET >> 48) as u16
}

const fn canonical_nan_hi16() -> u16 {
    // box_double's miss: the default NaN's bits plus the offset, whose
    // low 48 bits are clear
    ((f64::NAN.to_bits().wrapping_add(DOUBLE_ENCODE_OFFSET)) >> 48) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_encode_like_the_runtime_table() {
        assert_eq!(encode_const(TAG_INT, 7), TAG_TYPE_NUMBER | 7);
        assert_eq!(encode_const(TAG_INT, -1), TAG_TYPE_NUMBER | 0xFFFF_FFFF);
        assert_eq!(
            encode_const(TAG_INT, 1 << 40),
            box_double((1u64 << 40) as f64)
        );
        assert_eq!(encode_const(TAG_HEAP, 0), VALUE_NULL);
        assert_eq!(encode_const(TAG_BOOL, 2), box_bool(true));
        assert_eq!(encode_const(5, 99), VALUE_UNDEFINED);
    }

    #[test]
    fn canonical_nan_matches_box_double() {
        let nan = torajs_anyvalue::nanbox::box_double(f64::NAN);
        assert_eq!(nan, (canonical_nan_hi16() as u64) << 48);
        assert_eq!(nan & 0xFFFF_FFFF_FFFF, 0);
        assert_eq!(DOUBLE_ENCODE_OFFSET & 0xFFFF_FFFF_FFFF, 0);
    }
}
