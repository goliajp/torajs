//! The value fit in front of the typed struct field store — moved out
//! of [`super`] whole when the store's any-lane route went in.

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_checked_cast::Admits;

/// Fit `v` to `field_ty`, answering whether the fit MINTED — handed
/// back a reference of its own rather than the source expression's.
///
/// The caller's retain is keyed on whether the VALUE EXPRESSION
/// transfers ownership, and that question is about the wrong thing
/// once a coercion has replaced the value: a borrowed `any` ident
/// said "does not transfer", so an any→string fit was retained a
/// second time and stranded one string cell per write (every
/// `new Error(msg)` — the built-in ctor is `this.message = message`
/// with `message: any`). Same `(Operand, minted)` shape
/// `ssa_lower_binop_inner::add_str::coerce_to_str` already uses.
pub(super) fn fit_to_field(
    ctx: &mut LowerCtx<'_>,
    field_ty: Type,
    v: Operand,
    value: ExprId,
    (obj, field): (ExprId, &str),
) -> (Operand, bool) {
    if let Some(cast) = ctx.cast_stored_obj(field_ty, v, value, obj, field) {
        return (cast, true);
    }
    // W4 — align the stored value with the field width (mirrors the
    // index-assign site; the reverse direction means the width
    // analysis missed this write).
    let v_ty = ctx.operand_ty(&v);
    if let Some(cast) = ctx.checked_cast(
        v,
        field_ty,
        crate::ssa_lower_checked_cast::CastSlot::Store,
        crate::ssa_lower_checked_cast::CastScope::Inferred,
        Admits::Exact,
    ) {
        return (cast, field_ty == Type::Str);
    }
    // an optional `number` member keeps an f64 slot whose `undefined`
    // is the sentinel (a `null` or an any-lane value never reaches
    // here: `number_slot_cannot_spell`)
    if field_ty == Type::F64
        && matches!(v, Operand::ConstPtrNull)
        && crate::ssa_lower_object_lit::absent::optional_number_slot(
            ctx,
            Some(obj),
            field,
            field_ty,
        )
    {
        return (crate::ssa_lower_object_lit::absent::f64_undef(), false);
    }
    match (field_ty, v_ty) {
        (Type::F64, Type::I64) => (ctx.coerce_to_f64(v), false),
        (Type::I64, Type::F64) => panic!(
            "ssa-lower: f64 value into an i64 struct field — \
             container width analysis missed this write"
        ),
        // RFC 20260710 C4 — a declared-Any slot (`__nullable(
        // number|boolean)` optional field, plain `any` field) takes a
        // NaN-box: box the scalar / nullish-literal write (expr-aware
        // — an undefined literal boxes to ANY_UNDEF, a null literal
        // to ANY_NULL). The box is an rc-inert immediate for these
        // payloads, so the caller's share inc no-ops through
        // __torajs_rc_inc's NaN-box gate. Heap sources keep their
        // pre-RFC raw store — their ownership story is a separate
        // face.
        (Type::Any, Type::I64 | Type::I32 | Type::F64 | Type::Bool) => {
            (ctx.box_to_any_from_expr(value, v), false)
        }
        (Type::Any, Type::Ptr) if matches!(v, Operand::ConstPtrNull) => {
            (ctx.box_to_any_from_expr(value, v), false)
        }
        // …and a heap source whose slot may be holding the generic
        // immortal cell. The raw store is right for a live heap
        // value — the slot reads a pointer back and the reader tags
        // it — but the cell is a pointer too, so `o.v = xs.find(p)`
        // on a miss left `typeof o.v` answering "object" for a value
        // that IS undefined. The same expr-aware box the two arms
        // above take settles it, branchlessly (tag = 4 + (val ==
        // sentinel)); a live value comes back out of the box as
        // itself. Nothing is minted — the box wraps the source
        // expression's own reference, which is why the caller's
        // retain still answers the right question.
        (Type::Any, _) if crate::ssa_lower_nullable_guard::holds_undef_cell(ctx, value, v_ty) => {
            (ctx.box_to_any_from_expr(value, v), false)
        }
        // S2.26 (RFC 20260727-dstr-assignment 刀 4) — the reverse
        // direction: an Any value into a declared scalar / Str field
        // unboxes through the same kernels every other any→typed sink
        // uses (coerce_for_local / coerce_for_global). The
        // fall-through used to store the NaN-box bits raw — `o.k =
        // (v: any) 9` read back as NaN. Bool / heap-typed fields
        // still fall through (no established unbox kernel carries
        // their guard story) — the registered S2.26 remainder.
        //
        // The number fits answer `false` because their results are
        // Copy; the string fit MINTS, and that is the whole reason
        // this function reports it.
        (Type::F64 | Type::I64, Type::Any) => (ctx.coerce_any_to_number(v, field_ty), false),
        (Type::Str, Type::Any) => (ctx.coerce_to_str(v, Type::Any), true),
        // A view outlives nothing it borrows from once it sits in a
        // field (a split element's storage is its array's): the field
        // takes an owned copy, and a fresh view is released after it.
        (Type::Str, Type::Substr) => {
            let owned = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.substr_to_owned, vec![v]),
                Type::Str,
                None,
            );
            if ctx.expr_transfers_ownership(value) {
                ctx.emit_drop_value(v, Type::Substr);
            }
            (Operand::Value(owned), true)
        }
        _ if crate::ssa_lower_assign_slot_fit::arr_views_into_owned_slot(ctx, field_ty, v_ty) => {
            if ctx.expr_transfers_ownership(value) {
                let m = crate::ssa_lower_assign_slot_fit::materialize_fresh_views(ctx, field_ty, v);
                (m, false)
            } else {
                (
                    crate::ssa_lower_assign_slot_fit::copy_views_out(ctx, field_ty, v),
                    true,
                )
            }
        }
        _ => (v, false),
    }
}
