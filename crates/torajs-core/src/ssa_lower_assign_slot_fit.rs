//! Slot-fit helpers for [`super::ssa_lower_assign_ident`] — "does this
//! value fit the slot, and how do we make it fit". Split out of that
//! file (r381) when the mismatch-message hint pushed it past the
//! 500-line limit; the four functions here answer one question and the
//! rest of that file answers a different one (how an assignment
//! lowers).

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx};
/// An `Arr<Substr>` value (a split product, or an alias of one) meeting
/// an `Arr<Str>` slot — a local or global being assigned, a parameter,
/// a setter's value. The views cannot live in that slot — its readers
/// decode owned cells — so the value is converted at the boundary:
/// [`materialize_fresh_views`] for a product the site owns outright,
/// [`copy_views_out`] for one it only borrows (rotation 469, plan-state
/// 468-02 / 469-01; the assignment and call-argument faces of the
/// rotation-468 copy-out rule).
pub(crate) fn arr_views_into_owned_slot(ctx: &LowerCtx<'_>, slot_ty: Type, v_ty: Type) -> bool {
    matches!((slot_ty, v_ty), (Type::Arr(s), Type::Arr(v))
        if ctx.arr_layouts[s.0 as usize] == Type::Str
            && ctx.arr_layouts[v.0 as usize] == Type::Substr)
}

/// A fresh split product (the expression transfers ownership — the
/// site holds the only reference) is materialized in place: every
/// view becomes an owned string and the same cell is stored. Answers
/// that cell under the slot's `Arr<Str>` type.
pub(crate) fn materialize_fresh_views(
    ctx: &mut LowerCtx<'_>,
    slot_ty: Type,
    v: Operand,
) -> Operand {
    let cur_block = ctx.cur_block;
    let owned = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.arr_substr_materialize_owned, vec![v]),
        slot_ty,
        None,
    );
    Operand::Value(owned)
}

/// A borrowed view array is copied out through the slice kernel and
/// the copy adopted — views never leave their split block — and the
/// fresh `Arr<Str>` copy is answered. The caller decides what happens
/// to the source's stake (an assignment hands its borrow inc back; a
/// call releases nothing it did not take) and who releases the copy.
pub(crate) fn copy_views_out(ctx: &mut LowerCtx<'_>, slot_ty: Type, v: Operand) -> Operand {
    let cur_block = ctx.cur_block;
    let len = ctx.f.append_inst(
        cur_block,
        InstKind::Load(Type::I64, v.clone(), ARR_LEN_OFF),
        Type::I64,
        None,
    );
    let copy = ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.arr_slice,
            vec![v, Operand::ConstI64(0), Operand::Value(len)],
        ),
        slot_ty,
        None,
    );
    ctx.emit_adopt_copied_range(
        Operand::Value(copy),
        Type::Substr,
        Operand::ConstI64(0),
        Operand::Value(len),
    );
    Operand::Value(copy)
}

/// The assignment face: a fresh rhs is materialized in place; a
/// borrowed one (an alias the borrow inc above staked) is copied out,
/// then the stake on the source is handed back, since the slot holds
/// the copy.
pub(super) fn coerce_arr_views_to_owned(
    ctx: &mut LowerCtx<'_>,
    slot_ty: Type,
    v_ty: Type,
    v: Operand,
    value: ExprId,
) -> Operand {
    if ctx.expr_transfers_ownership(value) {
        return materialize_fresh_views(ctx, slot_ty, v);
    }
    let copy = copy_views_out(ctx, slot_ty, v.clone());
    ctx.emit_drop_value(v, v_ty);
    copy
}

pub(super) fn global_coercion_compatible(ctx: &LowerCtx<'_>, slot_ty: Type, v_ty: Type) -> bool {
    v_ty == slot_ty
        || arr_views_into_owned_slot(ctx, slot_ty, v_ty)
        // chunk 809 — an Any slot admits everything (boxed at the caller)
        || slot_ty == Type::Any
        || (slot_ty == Type::F64 && v_ty == Type::I64)
        || (slot_ty == Type::I64 && v_ty == Type::I32)
        || (slot_ty == Type::I32 && v_ty == Type::I64)
        || slot_ty == Type::Ptr
        || v_ty == Type::Ptr
        || (matches!(slot_ty, Type::I64 | Type::F64) && v_ty == Type::Any)
        || (slot_ty == Type::Str && v_ty == Type::Any)
        // L3b #4 — shorter-arity closures fit wider fn-typed slots
        // (see check_local_coercion's Closure arm).
        || (matches!(slot_ty, Type::Closure(_)) && matches!(v_ty, Type::Closure(_)))
}

pub(super) fn coerce_for_global(
    ctx: &mut LowerCtx<'_>,
    slot_ty: Type,
    v_ty: Type,
    v: Operand,
    value: ExprId,
) -> Operand {
    if arr_views_into_owned_slot(ctx, slot_ty, v_ty) {
        coerce_arr_views_to_owned(ctx, slot_ty, v_ty, v, value)
    } else if slot_ty == Type::F64 && v_ty == Type::I64 {
        ctx.coerce_to_f64(v)
    } else if matches!(slot_ty, Type::I64 | Type::F64) && v_ty == Type::Any {
        ctx.coerce_any_to_number(v, slot_ty)
    } else if slot_ty == Type::Str && v_ty == Type::Any {
        ctx.coerce_to_str(v, Type::Any)
    } else {
        v
    }
}

pub(super) fn check_local_coercion(ctx: &LowerCtx<'_>, name: &str, snap_ty: Type, v_ty: Type) {
    if snap_ty == Type::Any && v_ty != Type::Any {
        return;
    }
    let direct_compatible = v_ty == snap_ty
        || (snap_ty == Type::F64 && v_ty == Type::I64)
        || (matches!(snap_ty, Type::I64 | Type::F64) && v_ty == Type::Any)
        || (snap_ty == Type::Str && v_ty == Type::Any)
        || (snap_ty == Type::Str && v_ty == Type::Substr)
        || arr_views_into_owned_slot(ctx, snap_ty, v_ty);
    if direct_compatible {
        return;
    }
    let int_or_ptr_lax = (snap_ty == Type::I64 && v_ty == Type::I32)
        || (snap_ty == Type::I32 && v_ty == Type::I64)
        || snap_ty == Type::Ptr
        || v_ty == Type::Ptr;
    if int_or_ptr_lax {
        return;
    }
    // L3b #4 — a shorter-arity closure fits a wider fn-typed slot
    // (the checker's callback-subtype lattice is the sole admit
    // gate): both are env cells, the call site pushes the SLOT sig's
    // args and the callee reads its own shorter prefix, extra arg
    // registers are simply never read.
    if matches!(snap_ty, Type::Closure(_)) && matches!(v_ty, Type::Closure(_)) {
        return;
    }
    panic!(
        "ssa-lower: assignment to `{name}` mismatch — slot is {snap_ty:?} but value is {v_ty:?}{}",
        mismatch_hint(&snap_ty, &v_ty),
    );
}

/// The advice half of an assignment-mismatch message.
///
/// r381 — this used to be appended unconditionally, so an `any` value
/// meeting a struct slot was told to "use `>>` for integer divide or
/// annotate the slot as the appropriate numeric width". That advice is
/// only true when BOTH sides are numeric and the two widths disagree;
/// everywhere else it sent the reader looking for a division they never
/// wrote. The slot and value types are already named in the message, so
/// saying nothing is the honest tail.
pub(super) fn mismatch_hint(slot: &Type, val: &Type) -> &'static str {
    let numeric = |t: &Type| matches!(t, Type::I64 | Type::I32 | Type::F64);
    if numeric(slot) && numeric(val) {
        "; use `>>` for integer divide or annotate the slot as the appropriate numeric width"
    } else {
        ""
    }
}

pub(super) fn coerce_for_local(
    ctx: &mut LowerCtx<'_>,
    snap_ty: Type,
    v_ty: Type,
    v: Operand,
    value: ExprId,
) -> Operand {
    if arr_views_into_owned_slot(ctx, snap_ty, v_ty) {
        coerce_arr_views_to_owned(ctx, snap_ty, v_ty, v, value)
    } else if snap_ty == Type::Any && v_ty != Type::Any {
        // S2.28 (RFC 20260727-dstr-assignment) — the eid-aware box,
        // same as the global lane and the let-init lane: a typed OOB
        // element read (`b = t[i]` past the end) carries the
        // undefined sentinel, and the eid-blind `box_to_any` was
        // boxing its raw bits as a Number (`let b: any = 0;
        // b = t[1];` answered NaN). The expr gate also keeps
        // `undefined` / `null` rhs tags apart, as the global path
        // has since chunk 809.
        ctx.box_to_any_from_expr(value, v)
    } else if snap_ty == Type::F64 && v_ty == Type::I64 {
        ctx.coerce_to_f64(v)
    } else if matches!(snap_ty, Type::I64 | Type::F64) && v_ty == Type::Any {
        ctx.coerce_any_to_number(v, snap_ty)
    } else if snap_ty == Type::Str && v_ty == Type::Any {
        ctx.coerce_to_str(v, Type::Any)
    } else if snap_ty == Type::Str && v_ty == Type::Substr {
        // Chunk 561 — a Substr view assigned into an owned-Str slot
        // materializes (mirror of materialize_call_args); the view
        // reference (fresh from string indexing, or the +1 an alias
        // rhs took above) releases after the copy, so both shapes
        // balance.
        let cur_block = ctx.cur_block;
        let owned = ctx.f.append_inst(
            cur_block,
            InstKind::Call(ctx.intrinsics.substr_to_owned, vec![v.clone()]),
            Type::Str,
            None,
        );
        ctx.emit_drop_value(v, Type::Substr);
        Operand::Value(owned)
    } else {
        v
    }
}
