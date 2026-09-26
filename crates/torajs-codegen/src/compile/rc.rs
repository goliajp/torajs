//! Inline refcount retain: `InstKind::RcInc`.

use core::mem::offset_of;

use torajs_core::ssa::Operand;
use torajs_rc::{FLAG_STATIC_LITERAL, HeapHeader};

use super::{OP_SCRATCH_LHS, OP_SCRATCH_RHS, OP_SCRATCH_TMP, materialize_operand_gpr, write_u32};
use crate::enc::{add_w_imm, cbz_x, ldr_x_imm12, str_w_imm12, tbnz};
use crate::regalloc::Assignment;

// the header is read as one little-endian u64 so a single load yields
// both the flags and the count
const _: () = assert!(offset_of!(HeapHeader, refcount) == 0);
const STATIC_BIT: u32 =
    offset_of!(HeapHeader, flags) as u32 * 8 + FLAG_STATIC_LITERAL.trailing_zeros();

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
}
