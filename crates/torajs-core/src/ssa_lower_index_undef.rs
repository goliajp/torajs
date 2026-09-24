//! The `undefined` a typed element read answers where there is no
//! element — moved verbatim out of `ssa_lower_index.rs`
//! (`lower_typed_index_checked`'s out-of-range exit) when that file
//! reached its 500-line limit.

use crate::ssa::{BlockId, InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_binop_null_undef::UNDEF_CELL_SYM;
use crate::ssa_lower_intrinsics_str_b::STR_UNDEF_CELL_SYM;
use crate::ssa_lower_undef_f64_source::F64_UNDEF_SENTINEL_BITS;

/// The value an out-of-range read of an `elem_ty` element stores into
/// its result slot, emitted into `oob_blk`, and whether that exit
/// records a RangeError instead (an integer element has no spelling
/// of `undefined`).
pub(crate) fn oob_undef_value(
    ctx: &mut LowerCtx<'_>,
    oob_blk: BlockId,
    elem_ty: Type,
) -> (Operand, bool) {
    let mut oob_throws = false;
    let oob_val: Operand = match elem_ty {
        Type::Str => {
            let sentinel = ctx.f.append_inst(
                oob_blk,
                InstKind::GlobalRef(STR_UNDEF_CELL_SYM.to_string()),
                Type::Str,
                None,
            );
            Operand::Value(sentinel)
        }
        Type::F64 => Operand::ConstF64(f64::from_bits(F64_UNDEF_SENTINEL_BITS)),
        // A pointer-shaped element spells `undefined` with the generic
        // immortal cell, which is how a `find` / `pop` miss on the same
        // array has answered since RFC 20260722 chunk B. Reading past
        // the end is the same question (§10.4.2.1), so it takes the
        // same exit rather than the RangeError below — `q.find(miss)`
        // answered `undefined` while `q[5]` on that very array threw.
        // Static cell: the result slot's scope drop no-ops.
        //
        // Emitted into `oob_blk` rather than through
        // `str_undef_sentinel_for`, which appends at `cur_block`: a
        // constant out-of-range index folds the first compare away and
        // takes that block with it, leaving the reference with no
        // definition (`nested[-1]` aborted the compile).
        t if t.spells_undef_with_generic_cell() => {
            let sentinel = ctx.f.append_inst(
                oob_blk,
                InstKind::GlobalRef(UNDEF_CELL_SYM.to_string()),
                elem_ty,
                None,
            );
            Operand::Value(sentinel)
        }
        // The promoted bool read spells `undefined` as the tag every
        // tagged value already uses for it.
        Type::Bool => {
            let undef = ctx.f.append_inst(
                oob_blk,
                InstKind::Call(
                    ctx.intrinsics.any_box,
                    vec![Operand::ConstI64(5), Operand::ConstI64(0)],
                ),
                Type::Any,
                None,
            );
            Operand::Value(undef)
        }
        _ => {
            oob_throws = true;
            ctx.f.append_void(
                oob_blk,
                InstKind::Call(ctx.intrinsics.arr_oob_throw, vec![]),
            );
            match elem_ty {
                Type::I64 | Type::I32 => Operand::ConstI64(0),
                _ => Operand::ConstPtrNull,
            }
        }
    };
    (oob_val, oob_throws)
}
