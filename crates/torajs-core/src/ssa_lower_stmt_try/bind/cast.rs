//! The typed catch slots that take the thrown value through the checked
//! cast of an annotated slot (`ssa_lower_checked_cast`) rather than a
//! tag test of their own.

use super::ir::append;
use super::raise;
use crate::ssa::{InstKind, Operand, Type, ValueId};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_checked_cast::CastSlot;

/// Whether a slot of `e_ty` takes the thrown value through the checked
/// cast rather than a tag test of its own: an object type other than a
/// declared non-generic class, an array, or a heap family a tag names.
pub(super) fn by_cast(ctx: &LowerCtx, e_ty: Type, ann: &str) -> bool {
    use crate::ssa_lower_checked_cast::plan::cell_tag;
    match e_ty {
        Type::Obj(_) => crate::ssa_lower_instanceof::tags::class_tags_for_catch(ctx, ann).is_none(),
        Type::Arr(_) | Type::Closure(_) => true,
        t => cell_tag(t).is_some(),
    }
}

/// The checked cast of the thrown value into a slot of `e_ty`: boxed,
/// cast as an annotated slot casts an `any` value, the box released on
/// both edges. A slot no cast tests rejects every value.
pub(super) fn take_cast(
    ctx: &mut LowerCtx,
    p: &str,
    (tag, v): (ValueId, ValueId),
    (e_ty, ann): (Type, &str),
) -> Operand {
    let pair = vec![Operand::Value(tag), Operand::Value(v)];
    let boxed = Operand::Value(append(
        ctx,
        InstKind::Call(ctx.intrinsics.any_box, pair),
        Type::Any,
    ));
    if matches!(e_ty, Type::Closure(_)) {
        return take_fn(ctx, p, boxed);
    }
    let parked = ctx.push_throw_temp(boxed, Type::Any);
    let out = cast(ctx, boxed, e_ty, ann).unwrap_or_else(|| {
        raise(ctx, p, ann);
        Operand::ConstPtrNull
    });
    ctx.pop_throw_temp(parked);
    ctx.emit_drop_value(boxed, Type::Any);
    out
}

/// A function slot takes any function object as it is, the check of a
/// function slot an `any` value enters (`checked_cast::check_fn_slot`):
/// no tag tells one face from another, so the parameter's calls go
/// through the boxed entry, which adapts to the stored value's own face.
/// The box is the value the slot holds, as a `let` of a function type
/// holds an `any` initializer.
fn take_fn(ctx: &mut LowerCtx, p: &str, boxed: Operand) -> Operand {
    check_fn(ctx, boxed);
    ctx.variadic_locals.insert(p.to_string());
    boxed
}

/// The TypeError of a function slot for a boxed value that is no
/// function object, routed like any throw out of the catch.
pub(super) fn check_fn(ctx: &mut LowerCtx, boxed: Operand) {
    let parked = ctx.push_throw_temp(boxed, Type::Any);
    let site = crate::ssa_lower_checked_cast::site_line(CastSlot::Assign, "function");
    let site = Operand::Value(ctx.intern_string_literal(site.as_str()));
    let check = ctx.fn_table[crate::ssa_lower_intrinsics_any_cast::CHECK_FN];
    let call = InstKind::Call(check, vec![boxed, site]);
    ctx.f.append_void(ctx.cur_block, call);
    ctx.emit_throw_check(None);
    ctx.pop_throw_temp(parked);
}

/// The cast's answer, after its throw check; `None` for a slot no cast
/// tests.
fn cast(ctx: &mut LowerCtx, boxed: Operand, e_ty: Type, ann: &str) -> Option<Operand> {
    let slot = CastSlot::Assign;
    let cast = match e_ty {
        Type::Obj(sid) => ctx.checked_cast_obj(boxed, sid, ann, slot),
        Type::Arr(_) if ctx.casts_obj_arr_slot(e_ty, ann) => {
            ctx.checked_cast_obj_arr(boxed, e_ty, ann, slot)
        }
        Type::Arr(_) => crate::ssa_lower_assign_slot_fit::arr_slot_takes_any(ctx, e_ty, Type::Any)
            .map(|kind| {
                let fid = ctx.fn_table["__torajs_arr_from_any_typed"];
                let argv = vec![boxed, Operand::ConstI64(kind)];
                Operand::Value(append(ctx, InstKind::Call(fid, argv), e_ty))
            }),
        t => crate::ssa_lower_checked_cast::plan::cell_tag(t).map(|tag| {
            let site = crate::ssa_lower_checked_cast::site_line(slot, ann);
            let site = Operand::Value(ctx.intern_string_literal(site.as_str()));
            let argv = vec![boxed, Operand::ConstI64(tag as i64), site];
            Operand::Value(append(
                ctx,
                InstKind::Call(ctx.intrinsics.anyv_cast_cell, argv),
                t,
            ))
        }),
    };
    // `checked_cast_obj*` check their own; the other two raise through
    // the pending slot like any kernel
    if cast.is_some() && !matches!(e_ty, Type::Obj(_)) && !ctx.casts_obj_arr_slot(e_ty, ann) {
        ctx.emit_throw_check(None);
    }
    cast
}
