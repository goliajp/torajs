//! Phase 5 of [`super`]'s object-literal lowering, the part before the
//! stores: each lowered value brought into the shape its resolved slot
//! holds. Moved out whole.

use std::collections::HashSet;

use crate::ast::{ExprId, PropKey};
use crate::ssa::{Operand, Type};
use crate::ssa_lower::LowerCtx;

/// Re-materialize `field_vals` for `layout`: the slot's own undefined
/// where the literal spelled `undefined` or left the member out
/// (`undef_fields`), a NaN-box behind an `any` slot, an unbox in front
/// of a typed slot taking an `any` value.
pub(super) fn fit_slot_values(
    ctx: &mut LowerCtx<'_>,
    layout: &[(PropKey, Type)],
    fields: &[(PropKey, ExprId)],
    undef_fields: &HashSet<PropKey>,
    absent: &[(usize, PropKey)],
    field_vals: &mut [Operand],
) {
    for (i, (fname, slot_ty)) in layout.iter().enumerate() {
        if undef_fields.contains(fname)
            && let Some(sentinel) = ctx.str_undef_sentinel_for(*slot_ty)
        {
            field_vals[i] = sentinel;
        }
        // an optional `number` member keeps an f64 slot: `undefined`
        // (spelled or left out) is the slot's sentinel, and a `null`
        // takes the any-lane crossing, which refuses it loudly
        if *slot_ty == Type::F64 && matches!(field_vals[i], Operand::ConstPtrNull) {
            field_vals[i] = match undef_fields.contains(fname) {
                true => super::absent::f64_undef(),
                false => {
                    let boxed = ctx.box_to_any(field_vals[i]);
                    let cross = crate::ssa_lower_member_obj_field::exotic_read::optional_number;
                    cross(ctx, boxed, fname.lossy().as_ref())
                }
            };
        }
        // RFC 20260710 C4 — a declared-Any slot (`__nullable(number|
        // boolean)` optional field, plain `any` field) takes a
        // NaN-box, never a raw scalar: resolve_objlit_layout only
        // retyped the slot, the value boxes here. The expr-aware
        // variant keeps null vs undefined distinct (ANY_NULL vs
        // ANY_UNDEF from the same lowered ConstPtrNull). Sources
        // outside the scalar/nullish set (heap values behind an
        // `any` field) keep their pre-RFC raw store — their
        // ownership story is a separate face.
        if *slot_ty == Type::Any {
            let src_ty = ctx.operand_ty(&field_vals[i]);
            let boxable = matches!(src_ty, Type::I64 | Type::I32 | Type::F64 | Type::Bool)
                || (src_ty == Type::Ptr && matches!(field_vals[i], Operand::ConstPtrNull));
            if boxable {
                let veid = fields
                    .iter()
                    .rev()
                    .find(|(n, _)| n == fname)
                    .map(|(_, veid)| *veid);
                field_vals[i] = match veid {
                    Some(e) => ctx.box_to_any_from_expr(e, field_vals[i]),
                    None if absent.iter().any(|(_, n)| n == fname) => super::absent::any_undef(ctx),
                    // spread-unfolded field — no source expr; the
                    // plain box maps ConstPtrNull to ANY_NULL.
                    None => ctx.box_to_any(field_vals[i]),
                };
            }
        } else if ctx.operand_ty(&field_vals[i]) == Type::Any {
            // Unbox mirror — a typed slot taking an Any-valued init:
            // a declared layout (let-decl / return-type hint, or a
            // generator step struct's `value: T`) pins the slot to T
            // while the field expr carries a NaN-box. Pre-fix the box
            // bits stored raw and read back as garbage (`{ value: x }`
            // with `x: any` under a `{ value: number }` return type
            // answered NaN). Same helpers as the as-cast unbox arms;
            // str / bigint mint fresh owned values the slot takes
            // over like any other owned init.
            field_vals[i] = match slot_ty {
                Type::F64 | Type::I64 => ctx.coerce_any_to_number(field_vals[i], *slot_ty),
                Type::Str => ctx.coerce_to_str(field_vals[i], Type::Any),
                Type::Bool => ctx.coerce_to_bool(field_vals[i]),
                Type::BigInt => ctx.coerce_any_to_bigint(field_vals[i]),
                _ => field_vals[i],
            };
        }
    }
}
