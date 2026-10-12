//! The adapter's result: the body's typed return boxed into an
//! `AnyValue` (split out of `build`).

use crate::ssa::{self, InstKind, Operand, Type};

use super::BoxedEntryIntrinsics;

/// Box the body's typed return into the adapter's `AnyValue` result.
///
/// The heap arm is where a TYPED ARRAY has to declare its element
/// kind: the cell crosses into the any world here, and without the
/// stamp every slot an any-lane reader touches decodes as a NaN box
/// (RFC 20260823-proxy-substrate 刀 4b).
pub(super) fn box_return(
    f: &mut ssa::Function,
    entry: ssa::BlockId,
    r: ssa::ValueId,
    ret_ty: Type,
    intr: &BoxedEntryIntrinsics,
    ret_arr_kind: Option<i64>,
) -> ssa::ValueId {
    match ret_ty {
        Type::Any => r,
        Type::F64 => {
            let bits = f.append_inst(
                entry,
                InstKind::BitCastF64ToI64(Operand::Value(r)),
                Type::I64,
                None,
            );
            f.append_inst(
                entry,
                InstKind::Call(
                    intr.any_box,
                    vec![Operand::ConstI64(3), Operand::Value(bits)],
                ),
                Type::Any,
                None,
            )
        }
        Type::Bool => {
            let z = f.append_inst(
                entry,
                InstKind::ZExtBoolToI64(Operand::Value(r)),
                Type::I64,
                None,
            );
            f.append_inst(
                entry,
                InstKind::Call(intr.any_box, vec![Operand::ConstI64(1), Operand::Value(z)]),
                Type::Any,
                None,
            )
        }
        Type::I64 | Type::I32 => f.append_inst(
            entry,
            InstKind::Call(intr.any_box, vec![Operand::ConstI64(2), Operand::Value(r)]),
            Type::Any,
            None,
        ),
        // A string slot spells undefined with its own sentinel cell;
        // the slot box maps it to `undefined` (a live Str transfers
        // its +1 through the box like the heap arm below).
        Type::Str => f.append_inst(
            entry,
            InstKind::Call(intr.anyv_box_str_slot, vec![Operand::Value(r)]),
            Type::Any,
            None,
        ),
        // Heap-typed returns arrive +1 owned; the box transfers
        // that reference to the returned AnyValue.
        _ => {
            // A TYPED array crossing into the any world carries
            // its element-kind chain, or every slot the any lane
            // reads decodes as a NaN box (RFC 20260823 刀 4b).
            if let Some(chain) = ret_arr_kind {
                f.append_void(
                    entry,
                    InstKind::Call(
                        intr.arr_mark_kind,
                        vec![Operand::Value(r), Operand::ConstI64(chain)],
                    ),
                );
            }
            f.append_inst(
                entry,
                InstKind::Call(intr.any_box, vec![Operand::ConstI64(4), Operand::Value(r)]),
                Type::Any,
                None,
            )
        }
    }
}
