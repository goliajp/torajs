//! A whole-object reader's view of one declared field (a spread, a
//! serializer, `Object.values`). Moved out of [`super`].

use super::{owned_slot_read, receiver_may_be_exotic};
use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// A whole-object reader's view of one declared field (a spread, a
/// serializer, `Object.values`): the value, its type, and whether it is
/// OWNED (the guarded read) or borrowed out of the slot (the bare
/// load). A field a write of the program can shadow answers as an
/// owned any-lane value (`shadow_read`), so the type is the value's,
/// not always the slot's.
pub(crate) fn slot_value(
    ctx: &mut LowerCtx<'_>,
    obj: Option<ExprId>,
    obj_val: Operand,
    (offset, field_ty): (u64, Type),
    name: &str,
) -> (Operand, Type, bool) {
    let shadowed = super::super::shadow_read::applies(ctx, obj, name, field_ty)
        || super::super::shadow_read::mistyped_field(ctx, name, field_ty);
    if shadowed && receiver_may_be_exotic(ctx, obj) {
        let v = super::super::shadow_read::boxed_load(ctx, None, obj_val, (offset, field_ty), name);
        return (v, Type::Any, true);
    }
    if let Some(v) = owned_slot_read(ctx, obj, obj_val, (offset, field_ty), name) {
        return (v, field_ty, true);
    }
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(field_ty, obj_val, offset),
        field_ty,
        None,
    );
    (Operand::Value(v), field_ty, false)
}

/// [`slot_value`] taken as an owned value: a borrowed one takes a
/// share.
pub(crate) fn owned_field_or_any(
    ctx: &mut LowerCtx<'_>,
    obj: Option<ExprId>,
    obj_val: Operand,
    slot: (u64, Type),
    name: &str,
) -> (Operand, Type) {
    let (v, ty, owned) = slot_value(ctx, obj, obj_val, slot, name);
    if !owned && ty.is_refcounted() {
        ctx.emit_rc_inc(v);
    }
    (v, ty)
}
