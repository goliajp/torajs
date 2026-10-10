//! The coercion into a module-global slot, split out of `ssa_lower_assign_slot_fit`.

use super::*;

pub(crate) fn coerce_for_global(
    ctx: &mut LowerCtx<'_>,
    slot_ty: Type,
    v_ty: Type,
    v: Operand,
    value: ExprId,
) -> Operand {
    if arr_views_into_owned_slot(ctx, slot_ty, v_ty) {
        coerce_arr_views_to_owned(ctx, slot_ty, v_ty, v, value)
    } else if arr_typed_into_any_slot(ctx, slot_ty, v_ty) {
        share_typed_into_any_slot(ctx, v_ty, v, value)
    } else if let Some(kind) = arr_slot_takes_any(ctx, slot_ty, v_ty) {
        coerce_any_into_arr_slot(ctx, slot_ty, kind, v)
    } else if arr_any_slot_takes_any(ctx, slot_ty, v_ty) {
        share_any_into_arr_any_slot(ctx, slot_ty, v)
    } else if super::fn_pointer_into_closure_slot(slot_ty, v_ty) {
        crate::ssa_lower_call_fn_cell::fn_value_cell(ctx, slot_ty, value, v)
    } else if slot_ty == Type::F64 && v_ty == Type::I64 {
        ctx.coerce_to_f64(v)
    } else if let Some(cast) = ctx.checked_cast(
        v,
        slot_ty,
        crate::ssa_lower_checked_cast::CastSlot::Assign,
        crate::ssa_lower_checked_cast::CastScope::Inferred,
        Admits::Exact,
    ) {
        cast
    } else if matches!(slot_ty, Type::I64 | Type::F64) && v_ty == Type::Any {
        ctx.coerce_any_to_number(v, slot_ty)
    } else if slot_ty == Type::Bool && v_ty == Type::Any {
        ctx.coerce_to_bool(v)
    } else if slot_ty == Type::Str && v_ty == Type::Any {
        ctx.coerce_to_str(v, Type::Any)
    } else if slot_ty == Type::Str && v_ty == Type::Substr {
        let cur_block = ctx.cur_block;
        let owned = ctx.f.append_inst(
            cur_block,
            InstKind::Call(ctx.intrinsics.substr_to_owned, vec![v]),
            Type::Str,
            None,
        );
        if ctx.expr_transfers_ownership(value) {
            ctx.emit_drop_value(v, Type::Substr);
        }
        Operand::Value(owned)
    } else {
        v
    }
}
