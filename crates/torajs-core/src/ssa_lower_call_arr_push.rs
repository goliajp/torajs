//! `<Arr>.push(v)` in-place arm pulled out of
//! [`crate::ssa_lower::lower_expr_inner`] `Expr::Call` dispatch as chunk-4
//! of the `Expr::Call` god-arm decomp (chunks 1-3 = Arr higher-order +
//! Map dispatch + Set dispatch).
//!
//! Four receiver shapes: a local binding, a top-level binding, an
//! element or other borrowed place (`xs[i]`, `(0, xs)`, an owned
//! temporary), and a field (`obj.items`). Each reads the array out and
//! hands it to one push core, [`push_onto`]; B1 fixed the cell across
//! grow, so nothing is stored back. A local filled by a pre-reserved
//! loop stores the slot directly at the running length instead.
//!
//! `Array<Any>` takes the 16-byte tagged-slot push
//! ([`crate::ssa_lower_arr_any_push`]); routing it through the 8-byte
//! typed `arr_push` wrote raw words into tagged slots.
//!
//! Returns `Some(new_len)` when callee shape matches; `None` lets the
//! caller fall through to the Phase I.1 sibling-class static dispatch
//! and the generic method dispatch arms below.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx, OBJ_HEADER_SIZE, inline_push, push_recv_hold};
use crate::ssa_lower_arr_grow_kernel::emit_grow_kernel_checked;
pub(crate) use crate::ssa_lower_call_arr_push_coerce::coerce_push_value;

/// Try to lower `<recv>.push(v)`. Returns `Some(new_len)` when one of the
/// four legal receiver shapes (local Ident, K.8 global Ident, `xs[i]`
/// index read, or `obj.field`) matches; `None` falls through to the next
/// dispatch arm.
pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let (recv_id, name) = match ctx.ast.get_expr(callee) {
        Expr::Member { obj, name } => (*obj, name.clone()),
        _ => return None,
    };
    if name != "push" {
        return None;
    }
    if args.is_empty() {
        return crate::ssa_lower_call_arr_push_empty::try_lower(ctx, recv_id);
    }
    if args.len() != 1 {
        return None;
    }
    if let Some(op) = try_lower_ident_local(ctx, recv_id, args) {
        return Some(op);
    }
    if let Some(op) = try_lower_ident_global(ctx, recv_id, args) {
        return Some(op);
    }
    if let Some(op) = try_lower_index(ctx, recv_id, args) {
        return Some(op);
    }
    try_lower_field(ctx, recv_id, args)
}

/// (c) `xs[i].push(v)` — index-read receiver (chunk 697). B1 fixed
/// the arr cell across grow (the data pointer indirection absorbs
/// realloc), so the borrowed elem read IS the receiver and no
/// outer-slot write-back exists to miss — the historical "no place
/// to store a possibly-realloc'd pointer" rejection is moot.
///
/// A comma or cast receiver whose value is a borrowed place (`(0, xs).push(v)`,
/// `(xs as any[]).push(v)`, the dead-zone guard `flag_closure_tdz` wraps a
/// closure's read in) rides the same arm for the same reason: its value is the cell.
fn try_lower_index(ctx: &mut LowerCtx<'_>, recv_id: ExprId, args: &[ExprId]) -> Option<Operand> {
    let mut tail = recv_id;
    while let Expr::Sequence { right: t, .. } | Expr::As { expr: t, .. } = ctx.ast.get_expr(tail) {
        tail = *t;
    }
    let borrowed_comma = tail != recv_id
        && matches!(
            ctx.ast.get_expr(tail),
            Expr::Ident(_) | Expr::Member { .. } | Expr::Index { .. }
        );
    // An owned temporary array (`[1, 2].push(3)`, `mk().push(v)`) is
    // the same cell-is-the-receiver story, released after the push.
    let owned_temp = ctx.expr_owned_shape(recv_id)
        && matches!(
            ctx.expr_types.get(&recv_id),
            Some(crate::check::Type::Array(_))
        );
    if !borrowed_comma && !owned_temp && !matches!(ctx.ast.get_expr(recv_id), Expr::Index { .. }) {
        return None;
    }
    let recv_val = ctx.lower_expr(recv_id);
    let recv_ty = ctx.operand_ty(&recv_val);
    let arr_id = match recv_ty {
        Type::Arr(id) => id,
        _ => return None,
    };
    let tok = ctx.park_owned_temp(recv_id, &recv_val);
    // a borrowed element or place may be overwritten by the argument
    let hold = match owned_temp {
        true => None,
        false => push_recv_hold::hold_place(ctx, args[0], recv_val),
    };
    let elem_ty = ctx.arr_layouts[arr_id.0 as usize];
    let n = push_onto(ctx, recv_val, args[0], elem_ty, recv_ty);
    push_recv_hold::release(ctx, hold);
    ctx.unpark_owned_temp(tok);
    ctx.release_owned_temp(recv_id, &recv_val);
    Some(n)
}

/// (a) Ident-receiver path: local binding to mutable `Type::Arr`. Includes
/// the v0.6+1 push-loop pre-reserve fast-path + the M2 captured-array env
/// writeback.
fn try_lower_ident_local(
    ctx: &mut LowerCtx<'_>,
    recv_id: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let recv_name = match ctx.ast.get_expr(recv_id) {
        Expr::Ident(n) => n,
        _ => return None,
    };
    let info = ctx.locals.get(recv_name).copied()?;
    let arr_id = match info.ty {
        Type::Arr(id) => id,
        _ => return None,
    };
    let arr_ty = info.ty;
    let elem_ty = ctx.arr_layouts[arr_id.0 as usize];
    let cur_arr = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(arr_ty, Operand::Value(info.slot), 0),
        arr_ty,
        None,
    );
    // P0.10 — Array<Any>.push(<concrete>) routes through arr_push_any
    // with a (tag, value) pair, never through the 8-byte-slot fast push.
    let fast = match elem_ty {
        Type::Any => None,
        _ => ctx.prereserve.unchecked_for.get(recv_name).copied(),
    };
    // v0.6+1 perf checkpoint — push-loop pre-reserve. If the enclosing
    // loop's lowerer detected the canonical fill pattern and reserved
    // room for it, this push is guaranteed to fit without a buffer
    // grow: store the slot at the running length and bump it. A fill
    // loop's arguments cannot call, so they cannot rebind the receiver.
    if let Some(state) = fast {
        if let Type::Inline(sid) = elem_ty {
            let n = inline_push::push_prereserved(ctx, state, args[0], sid);
            return Some(Operand::Value(n));
        }
        let (val, val_owned_from_substr) = coerce_push_value(ctx, args[0], elem_ty);
        let len_next = ctx.emit_prereserved_push(state, val);
        crate::ssa_lower_arr_grow_kernel::take_pushed_stake(
            ctx,
            args[0],
            val,
            elem_ty,
            val_owned_from_substr,
        );
        // push answers the new length (§23.1.3.23)
        return Some(Operand::Value(len_next));
    }
    let hold = push_recv_hold::hold(ctx, recv_name, false, args[0], Operand::Value(cur_arr));
    let n = push_onto(ctx, Operand::Value(cur_arr), args[0], elem_ty, arr_ty);
    push_recv_hold::release(ctx, hold);
    Some(n)
}

/// K.8 — Ident-receiver where the binding is a top-level refcount global
/// (registered by the K.6 globals pass). Load cur ptr from the global slot
/// via `GlobalRef` and push onto it. Mirror semantic of the local path
/// above — including refcount inc on the pushed value for refcounted
/// element types. P0.10 mirror: `const a: any[] = []` routes through
/// `arr_alloc_any(0)` so push must use the 16-byte tagged-slot intrinsic.
fn try_lower_ident_global(
    ctx: &mut LowerCtx<'_>,
    recv_id: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let recv_name = match ctx.ast.get_expr(recv_id) {
        Expr::Ident(n) => n,
        _ => return None,
    };
    if ctx.locals.contains_key(recv_name) {
        return None;
    }
    let slot_ty = ctx.globals.get(recv_name).copied()?;
    let arr_id = match slot_ty {
        Type::Arr(id) => id,
        _ => return None,
    };
    let arr_ty = slot_ty;
    let elem_ty = ctx.arr_layouts[arr_id.0 as usize];
    let slot_ptr = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::GlobalRef(recv_name.clone()),
        Type::Ptr,
        None,
    );
    let cur_arr = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(arr_ty, Operand::Value(slot_ptr), 0),
        arr_ty,
        None,
    );
    let hold = push_recv_hold::hold(ctx, recv_name, true, args[0], Operand::Value(cur_arr));
    let n = push_onto(ctx, Operand::Value(cur_arr), args[0], elem_ty, arr_ty);
    push_recv_hold::release(ctx, hold);
    Some(n)
}

/// (b) `obj.field.push(v)` — field-receiver path. Load the struct pointer
/// once (borrow), find the field's offset, read the array there and push
/// onto it. P0.10 mirror: `class W { items: any[] = [] }` allocates
/// via arr_alloc_any (16B tagged slots) so push must use arr_push_any too.
fn try_lower_field(ctx: &mut LowerCtx<'_>, recv_id: ExprId, args: &[ExprId]) -> Option<Operand> {
    let (struct_id, field_name) = match ctx.ast.get_expr(recv_id) {
        Expr::Member { obj, name } => (*obj, name.clone()),
        _ => return None,
    };
    let obj_val = ctx.lower_expr(struct_id);
    let obj_ty = ctx.operand_ty(&obj_val);
    let sid = match obj_ty {
        Type::Obj(s) => s,
        _ => return None,
    };
    let layout = ctx.struct_layouts[sid.0 as usize].clone();
    let (idx, field_ty) = layout.iter().enumerate().find_map(|(i, (fname, fty))| {
        if *fname == field_name {
            Some((i, *fty))
        } else {
            None
        }
    })?;
    let arr_id = match field_ty {
        Type::Arr(id) => id,
        _ => return None,
    };
    let elem_ty = ctx.arr_layouts[arr_id.0 as usize];
    let offset = OBJ_HEADER_SIZE + (idx as u64) * 8;
    let cur_arr = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(field_ty, obj_val, offset),
        field_ty,
        None,
    );
    // the argument may overwrite the field, or release the object
    let hold = push_recv_hold::hold_place(ctx, args[0], Operand::Value(cur_arr));
    let n = push_onto(ctx, Operand::Value(cur_arr), args[0], elem_ty, field_ty);
    push_recv_hold::release(ctx, hold);
    Some(n)
}

/// `arr.push(arg)` onto an array already read out, of any element type.
fn push_onto(
    ctx: &mut LowerCtx<'_>,
    arr: Operand,
    arg: ExprId,
    elem_ty: Type,
    arr_ty: Type,
) -> Operand {
    if matches!(elem_ty, Type::Any) {
        return ctx.emit_arr_any_push_at_value(arr, arg, arr_ty);
    }
    if let Type::Inline(sid) = elem_ty {
        return inline_push::push(ctx, arr, arg, sid);
    }
    // The shared push coercion (chain-mark, bool → i64, W4 width
    // align, Substr / Any materialize) — the field path's historical
    // inline copy lacked the Substr and Any arms, so an admitted Any
    // arg would have raw-written box bits into the typed slot.
    let (val, val_owned_from_substr) = coerce_push_value(ctx, arg, elem_ty);
    let push_arg = ctx.raw_slot_arg(val);
    let new_arr = emit_grow_kernel_checked(
        ctx,
        ctx.intrinsics.arr_push,
        arr,
        push_arg,
        arg,
        val,
        elem_ty,
        val_owned_from_substr,
        arr_ty,
    );
    // B1 — the cell is fixed across grow; nothing to write back.
    let new_len = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(new_arr), ARR_LEN_OFF),
        Type::I64,
        None,
    );
    Operand::Value(new_len)
}
