//! Inline refcount retain: `InstKind::RcInc` / `InstKind::RcIncAny`.

use core::mem::offset_of;

use torajs_core::ssa::Operand;
use torajs_rc::{FLAG_STATIC_LITERAL, HeapHeader};

use super::{OP_SCRATCH_LHS, OP_SCRATCH_RHS, OP_SCRATCH_TMP, materialize_operand_gpr, write_u32};
use crate::enc::{add_w_imm, cbnz_x, cbz_x, ldr_x_imm12, lsr_imm, str_w_imm12, tbnz};
use crate::regalloc::Assignment;

// the header is read as one little-endian u64 so a single load yields
// both the flags and the count
const _: () = assert!(offset_of!(HeapHeader, refcount) == 0);
const STATIC_BIT: u32 =
    offset_of!(HeapHeader, flags) as u32 * 8 + FLAG_STATIC_LITERAL.trailing_zeros();

// NaN-box cell test (`torajs_rc::ffi::nan_box_is_cell_like`): every
// non-cell encoding sets a bit in the top 16 or TAG_BIT_TYPE_OTHER
const NON_CELL_TOP_SHIFT: u32 = 48;
const TAG_BIT_TYPE_OTHER: u32 = 1;

/// `HeapHeader::inc_ref` with the null check the runtime helper does:
///
/// ```text
///     cbz   xP, done
///     ldr   x10, [xP]
///     tbnz  x10, #STATIC_BIT, done
///     add   w11, w10, #1
///     str   w11, [xP]
/// done:
/// ```
///
/// The count is added and stored as 32 bits, so it wraps at 2^32 like
/// the runtime's `u32` and never carries into the tag / flags bytes.
/// A static cell is never written: string literals live in `__TEXT`.
pub fn emit_rc_inc(bytes: &mut Vec<u8>, ptr: &Operand, alloc: &Assignment) {
    if matches!(ptr, Operand::ConstPtrNull) {
        return;
    }
    let p = materialize_operand_gpr(bytes, ptr, OP_SCRATCH_LHS, alloc);
    emit_inc_cell(bytes, p);
}

/// [`emit_rc_inc`] behind the NaN-box cell test, so immediates fall
/// through untouched:
///
/// ```text
///     lsr   x10, xV, #48
///     cbnz  x10, done
///     tbnz  xV, #1, done
///     <emit_rc_inc sequence>
/// done:
/// ```
pub fn emit_rc_inc_any(bytes: &mut Vec<u8>, v: &Operand, alloc: &Assignment) {
    let Operand::Value(_) = v else {
        // constants are immediates or null: nothing to retain
        return;
    };
    let p = materialize_operand_gpr(bytes, v, OP_SCRATCH_LHS, alloc);
    write_u32(bytes, lsr_imm(OP_SCRATCH_RHS, p, NON_CELL_TOP_SHIFT));
    write_u32(bytes, cbnz_x(OP_SCRATCH_RHS, 28));
    write_u32(bytes, tbnz(p, TAG_BIT_TYPE_OTHER, 24));
    emit_inc_cell(bytes, p);
}

/// The five-instruction tail both forms share; every branch lands just
/// past its last instruction.
fn emit_inc_cell(bytes: &mut Vec<u8>, p: crate::reg::Gpr) {
    write_u32(bytes, cbz_x(p, 20));
    write_u32(bytes, ldr_x_imm12(OP_SCRATCH_RHS, p, 0));
    write_u32(bytes, tbnz(OP_SCRATCH_RHS, STATIC_BIT, 12));
    write_u32(bytes, add_w_imm(OP_SCRATCH_TMP, OP_SCRATCH_RHS, 1));
    write_u32(bytes, str_w_imm12(OP_SCRATCH_TMP, p, 0));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_bit_is_bit_50_of_the_header_word() {
        assert_eq!(STATIC_BIT, 50);
    }

    // the emitted test (`v >> 48 == 0 && bit 1 clear && v != 0`) must
    // answer what the runtime helper answers
    #[test]
    fn cell_test_matches_the_runtime_predicate() {
        let emitted =
            |v: u64| v >> NON_CELL_TOP_SHIFT == 0 && v & (1 << TAG_BIT_TYPE_OTHER) == 0 && v != 0;
        for v in [
            0u64,
            0x0000_0001_0000_1000,
            0x0000_0001_0000_1002,
            0x0001_0000_0000_0041,
            0xFFFE_0000_0000_0005,
            0x0007_FF00_0000_0000,
            0x0000_0000_0000_000A,
            u64::MAX,
        ] {
            assert_eq!(
                emitted(v),
                torajs_rc::ffi::nan_box_is_cell_like(v as *mut core::ffi::c_void),
                "{v:#x}"
            );
        }
    }
}
