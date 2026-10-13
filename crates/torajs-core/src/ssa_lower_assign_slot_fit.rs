//! Slot-fit helpers for [`super::ssa_lower_assign_ident`] — "does this
//! value fit the slot, and how do we make it fit". Split out of that
//! file (r381) when the mismatch-message hint pushed it past the
//! 500-line limit; the four functions here answer one question and the
//! rest of that file answers a different one (how an assignment
//! lowers).

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx};
use crate::ssa_lower_checked_cast::Admits;

mod closure_slot;
mod global;

use closure_slot::{any_into_closure_slot, fn_pointer_into_closure_slot};
pub(super) use global::coerce_for_global;

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

/// A typed array (`Arr<Str>`, `Arr<F64>`, a split product's views …)
/// meeting an `Arr<Any>` slot — an `any[]` annotation, or a `string[]`
/// one the width table widened because its alias class flows into an
/// `any` slot. The block keeps its raw-slot layout and is shared as it
/// is; the slot's kind-aware readers rebox each element on read once
/// the header carries the element kind (`emit_arr_mark_kind`), which
/// is how the binding lane has admitted the same crossing since RFC
/// 20260707 chunk 621. Before this the assignment lanes refused it
/// (`let a: any[] = []; a = typed` died at compile time where bun runs
/// it).
pub(crate) fn arr_typed_into_any_slot(ctx: &LowerCtx<'_>, slot_ty: Type, v_ty: Type) -> bool {
    matches!((slot_ty, v_ty), (Type::Arr(s), Type::Arr(v))
        if ctx.arr_layouts[s.0 as usize] == Type::Any
            && ctx.arr_layouts[v.0 as usize] != Type::Any)
}

/// The share for [`arr_typed_into_any_slot`]. A split product's views
/// never leave their block — the readers behind an `any`-element slot
/// would hand the inline views on — so an `Arr<Substr>` value is copied
/// out as owned strings first; then the block carries its element kind.
fn share_typed_into_any_slot(
    ctx: &mut LowerCtx<'_>,
    v_ty: Type,
    v: Operand,
    value: ExprId,
) -> Operand {
    let v = match v_ty {
        Type::Arr(id) if ctx.arr_layouts[id.0 as usize] == Type::Substr => {
            let owned_ty = Type::Arr(crate::ssa_lower::intern_arr_layout(
                ctx.arr_layouts,
                Type::Str,
            ));
            coerce_arr_views_to_owned(ctx, owned_ty, v_ty, v, value)
        }
        _ => v,
    };
    ctx.emit_arr_mark_kind(&v);
    v
}

/// An `any` value meeting a typed `Arr<T>` slot (`a = o` with `o:
/// any`, a split on an `any` receiver rebound into a `string[]`): the
/// box is decode-copied into a fresh typed block at the boundary, the
/// crossing the binding lane makes for an `Array<Any>` init under a
/// typed annotation (chunk 698); a non-array, or an element the kind
/// cannot carry, is a catchable TypeError. Answers the target kind.
/// Not for a view-layout slot: an `Arr<Substr>` block holds inline
/// views, which no copy of owned strings can be.
///
/// A slot whose class meets an `any`-annotated binding is widened to
/// an `any` element by the width table and takes the cell itself
/// ([`arr_any_slot_takes_any`]), so a borrowed rhs reaches this copy
/// only when its `any` type is the checker's alone (an unannotated
/// binding holding a split on an `any` receiver). The copy then
/// detaches the slot from the source — a later write to the source is
/// not seen through the slot, and the two are not `===` — where the
/// runtime semantics share one array; the alternative was the
/// compile-time refusal this replaces. Pinned by
/// `arr-any-slot-from-any-002`.
pub(crate) fn arr_slot_takes_any(ctx: &LowerCtx<'_>, slot_ty: Type, v_ty: Type) -> Option<i64> {
    let Type::Arr(s) = slot_ty else {
        return None;
    };
    if v_ty != Type::Any {
        return None;
    }
    let elem = ctx.arr_layouts[s.0 as usize];
    if elem == Type::Any || elem == Type::Substr {
        return None;
    }
    crate::ssa_lower_stmt_let_decl_convert::arr_elem_kind_const(&elem)
}

/// An `any` value meeting an `Arr<Any>` slot (`a = o` with `a: any[]`,
/// or a `string[]` slot the width table widened): the boxed array cell
/// is shared as it is — an ANY-flavor block or a kind-marked typed one,
/// both of which the slot's readers decode — and a non-array is a
/// catchable TypeError. The box's stake passes into the slot with the
/// cell (a heap box is the cell's pointer).
pub(crate) fn arr_any_slot_takes_any(ctx: &LowerCtx<'_>, slot_ty: Type, v_ty: Type) -> bool {
    matches!(slot_ty, Type::Arr(s) if ctx.arr_layouts[s.0 as usize] == Type::Any)
        && v_ty == Type::Any
}

fn share_any_into_arr_any_slot(ctx: &mut LowerCtx<'_>, slot_ty: Type, v: Operand) -> Operand {
    let fid = *ctx
        .fn_table
        .get("__torajs_arr_cell_from_any")
        .expect("__torajs_arr_cell_from_any intrinsic missing");
    let cur_block = ctx.cur_block;
    let out = ctx
        .f
        .append_inst(cur_block, InstKind::Call(fid, vec![v]), slot_ty, None);
    ctx.emit_throw_check(None);
    Operand::Value(out)
}

/// The copy for [`arr_slot_takes_any`]. The box's stake — the borrow
/// inc the assignment took, or a fresh temp's own — is released once
/// the copy holds its own shares.
fn coerce_any_into_arr_slot(
    ctx: &mut LowerCtx<'_>,
    slot_ty: Type,
    kind: i64,
    v: Operand,
) -> Operand {
    let fid = *ctx
        .fn_table
        .get("__torajs_arr_from_any_typed")
        .expect("__torajs_arr_from_any_typed intrinsic missing");
    let cur_block = ctx.cur_block;
    let out = ctx.f.append_inst(
        cur_block,
        InstKind::Call(fid, vec![v, Operand::ConstI64(kind)]),
        slot_ty,
        None,
    );
    ctx.emit_throw_check(None);
    ctx.emit_drop_value(v, Type::Any);
    Operand::Value(out)
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

/// A call's fresh `Arr<Substr>` product outside a view home
/// (`ast::let_owned_elems`, "Temporaries") is on its way to a receiver
/// typed by its own annotation — an argument, a literal element, a
/// returned or assigned value, a conditional's branch. It leaves the
/// producing expression as owned strings, under `Arr<Str>`. Only a
/// `split` call's own product is converted in place: any other call
/// answering views (`parts.reverse()`) may be handing back an array a
/// view-typed binding still reads, so it is copied out and the call's
/// stake released.
pub(crate) fn settle_escaping_views(ctx: &mut LowerCtx<'_>, eid: ExprId, v: Operand) -> Operand {
    let v_ty = ctx.operand_ty(&v);
    let Type::Arr(id) = v_ty else {
        return v;
    };
    if ctx.arr_layouts[id.0 as usize] != Type::Substr || ctx.ast.view_home_exprs.contains(&eid) {
        return v;
    }
    let owned_ty = Type::Arr(crate::ssa_lower::intern_arr_layout(
        ctx.arr_layouts,
        Type::Str,
    ));
    let is_split = matches!(ctx.ast.get_expr(eid), Expr::Call { callee, .. }
        if matches!(ctx.ast.get_expr(*callee), Expr::Member { name, .. } if name == "split"));
    if is_split {
        return materialize_fresh_views(ctx, owned_ty, v);
    }
    let copy = copy_views_out(ctx, owned_ty, v);
    ctx.emit_drop_value(v, v_ty);
    copy
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
        InstKind::Load(Type::I64, v, ARR_LEN_OFF),
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
    let copy = copy_views_out(ctx, slot_ty, v);
    ctx.emit_drop_value(v, v_ty);
    copy
}

pub(super) fn global_coercion_compatible(
    ctx: &LowerCtx<'_>,
    name: &str,
    slot_ty: Type,
    v_ty: Type,
) -> bool {
    v_ty == slot_ty
        || ctx.casts_assigned_obj(name, true, slot_ty, v_ty)
        || arr_views_into_owned_slot(ctx, slot_ty, v_ty)
        || arr_typed_into_any_slot(ctx, slot_ty, v_ty)
        || arr_slot_takes_any(ctx, slot_ty, v_ty).is_some()
        || arr_any_slot_takes_any(ctx, slot_ty, v_ty)
        // chunk 809 — an Any slot admits everything (boxed at the caller)
        || slot_ty == Type::Any
        || (slot_ty == Type::F64 && v_ty == Type::I64)
        || (slot_ty == Type::I64 && v_ty == Type::I32)
        || (slot_ty == Type::I32 && v_ty == Type::I64)
        || slot_ty == Type::Ptr
        || v_ty == Type::Ptr
        || (matches!(slot_ty, Type::I64 | Type::F64) && v_ty == Type::Any)
        || (slot_ty == Type::Str && v_ty == Type::Any)
        || (slot_ty == Type::Str && v_ty == Type::Substr)
        // rotation 630 — a Bool slot takes an `any` value through
        // ToBoolean-shaped unboxing (`__torajs_anyv_to_bool`), the
        // same typed-lane posture as the Number / Str arms above:
        // `let done = false; ({ value, done } = it.next())` over an
        // any-lane iterator result.
        || (slot_ty == Type::Bool && v_ty == Type::Any)
        // L3b #4 — shorter-arity closures fit wider fn-typed slots
        // (see check_local_coercion's Closure arm).
        || (matches!(slot_ty, Type::Closure(_)) && matches!(v_ty, Type::Closure(_)))
        || fn_pointer_into_closure_slot(slot_ty, v_ty)
        || any_into_closure_slot(slot_ty, v_ty)
        || crate::ssa_lower_iface::slot_takes(ctx, slot_ty, v_ty)
}

pub(super) fn check_local_coercion(ctx: &LowerCtx<'_>, name: &str, snap_ty: Type, v_ty: Type) {
    if snap_ty == Type::Any && v_ty != Type::Any {
        return;
    }
    let direct_compatible = v_ty == snap_ty
        || (snap_ty == Type::F64 && v_ty == Type::I64)
        || (matches!(snap_ty, Type::I64 | Type::F64) && v_ty == Type::Any)
        || (snap_ty == Type::Str && v_ty == Type::Any)
        || (snap_ty == Type::Bool && v_ty == Type::Any)
        || (snap_ty == Type::Str && v_ty == Type::Substr)
        || arr_views_into_owned_slot(ctx, snap_ty, v_ty)
        || arr_typed_into_any_slot(ctx, snap_ty, v_ty)
        || arr_slot_takes_any(ctx, snap_ty, v_ty).is_some()
        || arr_any_slot_takes_any(ctx, snap_ty, v_ty)
        || crate::ssa_lower_iface::slot_takes(ctx, snap_ty, v_ty)
        || ctx.casts_assigned_obj(name, false, snap_ty, v_ty);
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
    if matches!(snap_ty, Type::Closure(_)) && matches!(v_ty, Type::Closure(_))
        || fn_pointer_into_closure_slot(snap_ty, v_ty)
        || any_into_closure_slot(snap_ty, v_ty)
    {
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
    name: &str,
    snap_ty: Type,
    v_ty: Type,
    v: Operand,
    value: ExprId,
) -> Operand {
    if arr_views_into_owned_slot(ctx, snap_ty, v_ty) {
        coerce_arr_views_to_owned(ctx, snap_ty, v_ty, v, value)
    } else if arr_typed_into_any_slot(ctx, snap_ty, v_ty) {
        share_typed_into_any_slot(ctx, v_ty, v, value)
    } else if let Some(kind) = arr_slot_takes_any(ctx, snap_ty, v_ty) {
        coerce_any_into_arr_slot(ctx, snap_ty, kind, v)
    } else if arr_any_slot_takes_any(ctx, snap_ty, v_ty) {
        share_any_into_arr_any_slot(ctx, snap_ty, v)
    } else if snap_ty == Type::Any && v_ty != Type::Any {
        // S2.28 (RFC 20260727-dstr-assignment) — the eid-aware box,
        // same as the global lane and the let-init lane: a typed OOB
        // element read (`b = t[i]` past the end) carries the
        // undefined sentinel, and the eid-blind `box_to_any` was
        // boxing its raw bits as a Number (`let b: any = 0;
        // b = t[1];` answered NaN). The expr gate also keeps
        // `undefined` / `null` rhs tags apart, as the global path
        // has since chunk 809. The caller retained a borrowed heap
        // value; a named function's box takes its own stake.
        ctx.box_owned_from_expr(value, v, false)
    } else if fn_pointer_into_closure_slot(snap_ty, v_ty) {
        crate::ssa_lower_call_fn_cell::fn_value_cell(ctx, snap_ty, value, v)
    } else if snap_ty == Type::F64 && v_ty == Type::I64 {
        ctx.coerce_to_f64(v)
    } else if let Some(cast) = ctx.checked_cast(
        v,
        snap_ty,
        crate::ssa_lower_checked_cast::CastSlot::Assign,
        crate::ssa_lower_checked_cast::CastScope::Inferred,
        Admits::Exact,
    ) {
        cast
    } else if matches!(snap_ty, Type::I64 | Type::F64) && v_ty == Type::Any {
        ctx.coerce_any_to_number(v, snap_ty)
    } else if snap_ty == Type::Bool && v_ty == Type::Any {
        ctx.coerce_to_bool(v)
    } else if snap_ty == Type::Str && v_ty == Type::Any {
        ctx.coerce_to_str(v, Type::Any)
    } else if snap_ty == Type::Str && v_ty == Type::Substr {
        // Chunk 561 — a Substr view assigned into an owned-Str slot
        // materializes (mirror of materialize_call_args). A fresh view
        // releases after the copy; a borrowed one took no share — an
        // inline view's own count is never read by its drop, so a
        // share-then-release returned its parent's reference twice.
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
    } else if let Some(cast) = ctx.cast_assigned_obj(name, false, snap_ty, v) {
        cast
    } else {
        v
    }
}
