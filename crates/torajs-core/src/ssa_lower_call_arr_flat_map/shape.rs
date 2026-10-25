//! flatMap's product shape — whether the callback answers a scalar and
//! the element / array types the product is built with.

use crate::ast::ExprId;
use crate::ssa::Type;
use crate::ssa_lower::LowerCtx;

/// The destination's shape: whether the callback answers a scalar, and
/// the element / array types the product is built with.
///
/// Callback return can be either `Array<U>` (standard flat_map shape —
/// the inner walk pushes each element) or a scalar `U` per ES
/// §23.1.3.11 step 8.d (a non-Array result acts like `[U]` — single
/// push per outer iter, no inner walk). The scalar arm is gated by the
/// sibling `check_type_of_call_arr_flat_map_scalar` checker wedge
/// (admits `(T) => U` for primitive U).
pub(super) fn dst_shape(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    cb_ret_ty: Type,
) -> (bool, Type, Type) {
    match cb_ret_ty {
        Type::Arr(inner_arr_id) => {
            let inner = ctx.arr_layouts[inner_arr_id.0 as usize];
            // An inner `Arr<Substr>` (the callback answered a split
            // product) is copied out as owned strings — a view does
            // not leave its split block (rotation 468).
            if inner == Type::Substr {
                let dst_id =
                    crate::ssa_lower_interners::intern_arr_layout(ctx.arr_layouts, Type::Str);
                (false, Type::Str, Type::Arr(dst_id))
            } else {
                (false, inner, cb_ret_ty)
            }
        }
        // Value-less callback — every outer iteration pushes the
        // boxed-undefined sentinel (§23.1.3.11 step 8.d: a non-Array
        // result acts like `[U]`, and a void call's result is
        // `undefined`). The dst takes the Any flavor; the push value
        // is minted by the caller (`emit_undef_any_box`).
        Type::Void => {
            let dst_id = crate::ssa_lower_interners::intern_arr_layout(ctx.arr_layouts, Type::Any);
            (true, Type::Any, Type::Arr(dst_id))
        }
        Type::I64 | Type::F64 | Type::Str | Type::Substr | Type::Bool | Type::Any => {
            // RFC 20260726-array-elem-width knife 8 — the callback's
            // return is only one source of the product's elements, so
            // it cannot decide their width alone; the class the
            // analysis keys by this call's origin can be wider (a
            // fractional value reaching the product from anywhere else
            // widens it). Same ask an array literal already makes, and
            // the same one knife 1 gave map's product.
            let elem = if cb_ret_ty == Type::I64
                && ctx
                    .num_f64_slots
                    .elem_is_f64(&crate::num_width::SlotKey::Anon(eid.0))
            {
                Type::F64
            } else if cb_ret_ty == Type::Substr {
                // A scalar view answer is pushed as an owned copy
                // (rotation 468); the product is `Arr<Str>`.
                Type::Str
            } else {
                cb_ret_ty
            };
            let dst_id = crate::ssa_lower_interners::intern_arr_layout(ctx.arr_layouts, elem);
            (true, elem, Type::Arr(dst_id))
        }
        other => panic!("flatMap callback must return an array or primitive scalar, got {other:?}"),
    }
}
