//! A heap value headed for an `any` field of [`super`]'s literal.

use crate::ast::ExprId;
use crate::ssa::{Operand, Type};
use crate::ssa_lower::LowerCtx;

/// Box a heap field value whose slot is `any`: the declared layout
/// spells the slot `any`, or nothing is declared and the checker typed
/// the value `any` (`[1, 2] as any` passes a heap value through the
/// cast wearing its static type).
///
/// The literal's layout is named after its lowered values, so left as
/// is such a member interned a twin layout whose slot said `number[]`
/// under a type that says `any`. The bits agree — a heap pointer is its
/// own box — but the layout table is what an `any`-receiver write
/// reads: it found a typed slot it could not prove the new value fits
/// and kept the value in the shadow, where a read through the `any`
/// field never looks.
///
/// A slot declared with another type keeps the value as lowered: every
/// reader of that layout loads the slot at its declared type.
/// The box is a pure encoding; the stake stays where it was.
pub(super) fn box_heap_value(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    v: Operand,
    declared_field_ty: Option<Type>,
) -> Operand {
    let ty = ctx.operand_ty(&v);
    if !ty.is_refcounted() || matches!(ty, Type::Any | Type::Substr | Type::Closure(_)) {
        return v;
    }
    let any_slot = match declared_field_ty {
        Some(declared) => declared == Type::Any,
        None => matches!(ctx.expr_types.get(&eid), Some(crate::check::Type::Any)),
    };
    if any_slot { ctx.box_to_any(v) } else { v }
}
