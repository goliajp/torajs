//! The words `Promise.allSettled`'s sync kernel takes from its call
//! site: the record class tags and the form of the records' value slot.

use crate::ast::ExprId;
use crate::ssa::Type;
use crate::ssa_lower::LowerCtx;

/// The class tag `Promise.allSettled`'s `{status, value}` records have
/// to carry so a by-name read can find their fields.
///
/// The records used to be 48 anonymous bytes with `class_tag = 0`,
/// which is invisible to every by-name lookup: an unannotated handler
/// (parameter inferred `any`) read them as `{}`. An ordinary `{x: 1}`
/// literal is any-readable because the COMPILER stamps it out of
/// `anon_stamp_pool` and emits a layout row for it, and
/// `collect_class_field_candidates` walks that pool — so the record
/// joins the same pool rather than getting a mechanism of its own.
/// The runtime cannot mint this itself: `__torajs_class_layouts` is
/// link-emitted rodata with nothing appending to it at startup.
///
/// Two tags, packed low word / high word: the fulfilled record's
/// `{status, value}` and the rejected one's `{status, reason}`. §27.2.4.2
/// names the second field differently per outcome, and one layout cannot
/// answer to two names at the same offset — so the rejected shape is
/// interned as its own struct and stamped separately. The runtime picks
/// by state; the bytes it writes are identical either way.
///
/// `0` when the element shape is not a struct here — the anonymous
/// posture, unchanged.
pub(super) fn allsettled_record_tags(ctx: &mut LowerCtx<'_>, eid: ExprId) -> i64 {
    let Some(sid) = allsettled_record_sid(ctx, eid) else {
        return 0;
    };
    let fulfilled = ctx.anon_stamp_pool.borrow_mut().assign_or_get(sid);
    // The rejected twin: the fulfilled layout with its second field
    // renamed, so the field types (widths included) and the offsets
    // stay identical.
    let mut fields = ctx.struct_layouts[sid.0 as usize].clone();
    let [_, (second, _)] = &mut fields[..] else {
        return i64::from(fulfilled);
    };
    *second = "reason".into();
    let rsid = crate::ssa_lower_container_width::intern_struct_layout(ctx.struct_layouts, fields);
    let rejected = ctx.anon_stamp_pool.borrow_mut().assign_or_get(rsid);
    i64::from(fulfilled) | (i64::from(rejected) << 32)
}

/// The record struct, at the width every reader of this call's result
/// sees: the one an `await` of it re-interns (the width analysis joins
/// the record's `value` field with each source's value point).
fn allsettled_record_sid(ctx: &mut LowerCtx<'_>, eid: ExprId) -> Option<crate::ssa::StructId> {
    let inner = crate::ssa_lower_member_promise_value::recover_inner_ssa_ty(ctx, eid);
    let Some(Type::Arr(aid)) = ctx.widen_promise_inner_ty(inner, eid) else {
        return None;
    };
    match ctx.arr_layouts[aid.0 as usize] {
        Type::Obj(sid) => Some(sid),
        _ => None,
    }
}

/// The form each `allSettled` record's value slot has to hold — the
/// same problem `all`'s result array has, in the one other place that
/// buries a value where the awaiting site's own repr decode cannot
/// reach it: the record struct's second field.
pub(super) fn allsettled_value_repr(ctx: &mut LowerCtx<'_>, eid: ExprId) -> i64 {
    let Some(sid) = allsettled_record_sid(ctx, eid) else {
        return 0;
    };
    let Some(value_ty) = ctx.struct_layouts[sid.0 as usize].get(1).map(|(_, t)| *t) else {
        return 0;
    };
    let as_f64 = matches!(value_ty, Type::F64);
    crate::ssa_lower_promise_repr_mark::promise_value_repr(&value_ty, as_f64, false).unwrap_or(0)
}
