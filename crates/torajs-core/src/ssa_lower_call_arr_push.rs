//! `<Arr>.push(v)` in-place arm pulled out of
//! [`crate::ssa_lower::lower_expr_inner`] `Expr::Call` dispatch as chunk-4
//! of the `Expr::Call` god-arm decomp (chunks 1-3 = Arr higher-order +
//! Map dispatch + Set dispatch).
//!
//! Four legal receiver shapes (other shapes like `getArr().push(v)` are
//! rejected upstream; B1 fixed the cell across grow so the historical
//! "no place to store a realloc'd pointer" concern is gone — remaining
//! rejections are just unwired shapes):
//! - (a) Ident bound to a mutable `Type::Arr` local — load cur ptr from
//!   the slot, call `arr_push` (which may realloc), store result back
//!   into the slot. Includes the v0.6+1 push-loop pre-reserve fast-path:
//!   if the enclosing for-loop's lowerer detected the canonical fill
//!   pattern and emitted `arr_reserve(xs, len + N)`, skip the runtime
//!   call entirely and write the slot directly at
//!   `slot0 + len*8`. The Phase 2 captured-array writeback
//!   then mirrors the new ptr into the env block so re-invoked closures
//!   see the live buffer.
//! - K.8 — Ident-receiver where the binding is a top-level refcount
//!   global (registered by the K.6 globals pass). Load cur ptr from the
//!   global slot via `GlobalRef`, push, store back.
//! - (b) `obj.field.push(v)` where `field` is `Type::Arr` inside a struct
//!   — load cur ptr from `struct + offset`, push, store back at the
//!   same offset.
//!
//! All three paths handle `Type::Any` element arrays via
//! [`crate::ssa_lower_arr_any_push::SsaLower::emit_arr_any_push_at_slot`]
//! to dodge the 8-byte / 16-byte tagged-slot ABI mismatch that pre-P0.10
//! tora's typed `arr_push` introduced (raw int into Any slot → SIGSEGV on
//! next decode).
//!
//! Returns `Some(new_len)` when callee shape matches; `None` lets the
//! caller fall through to the Phase I.1 sibling-class static dispatch
//! and the generic method dispatch arms below.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx, OBJ_HEADER_SIZE};
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
    let elem_ty = ctx.arr_layouts[arr_id.0 as usize];
    if matches!(elem_ty, Type::Any) {
        let n = ctx.emit_arr_any_push_at_value(recv_val, args[0], recv_ty);
        ctx.unpark_owned_temp(tok);
        ctx.release_owned_temp(recv_id, &recv_val);
        return Some(n);
    }
    let (val, val_owned_from_substr) = coerce_push_value(ctx, args[0], elem_ty);
    let push_arg = ctx.raw_slot_arg(val);
    // Chunk 733's stake dance (owned-shape arg hands its +1 to the
    // array; probes P1 closure-push 30.5MB / P6 nested-arr 44.9MB /
    // P7 owned-str 16MB vs 6.3MB flat) lives in the helper now, on
    // both edges.
    let new_arr = emit_grow_kernel_checked(
        ctx,
        ctx.intrinsics.arr_push,
        recv_val,
        push_arg,
        args[0],
        val,
        elem_ty,
        val_owned_from_substr,
        recv_ty,
    );
    let new_len = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(new_arr), ARR_LEN_OFF),
        Type::I64,
        None,
    );
    ctx.unpark_owned_temp(tok);
    ctx.release_owned_temp(recv_id, &recv_val);
    Some(Operand::Value(new_len))
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
    // P0.10 — Array<Any>.push(<concrete>) routes through arr_push_any
    // with a (tag, value) pair. Pre-fix tora called the regular arr_push
    // intrinsic which uses 8-byte slots and corrupted the Array<Any>'s
    // 16-byte tagged-slot layout. Same boxing scheme as the Index-assign
    // Any path shipped earlier.
    if matches!(elem_ty, Type::Any) {
        return Some(ctx.emit_arr_any_push_at_slot(Operand::Value(info.slot), 0, args[0], arr_ty));
    }
    let cur_arr = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(arr_ty, Operand::Value(info.slot), 0),
        arr_ty,
        None,
    );
    let (val, val_owned_from_substr) = coerce_push_value(ctx, args[0], elem_ty);
    // v0.6+1 perf checkpoint — push-loop pre-reserve. If the enclosing
    // for-loop's lowerer detected the canonical fill pattern and emitted
    // `arr_reserve(xs, len + N)`, this push is guaranteed to fit without
    // a buffer grow. Inline the fast-path: read hoisted len, write the
    // slot at slot0 + len*8, bump len_slot (B1: slot0 is hoisted off
    // the data pointer; the reserve guarantees it stays valid).
    if let Some(state) = ctx.prereserve.unchecked_for.get(recv_name).copied() {
        let len_next = ctx.emit_prereserved_push(state, val);
        crate::ssa_lower_arr_grow_kernel::take_pushed_stake(
            ctx,
            args[0],
            val,
            elem_ty,
            val_owned_from_substr,
        );
        // chunk 9c — fast-push spec parity: ret new length (already in SSA
        // as `len_next`, mirrors arr[#8]).
        return Some(Operand::Value(len_next));
    }
    let push_arg = ctx.raw_slot_arg(val);
    let new_arr = emit_grow_kernel_checked(
        ctx,
        ctx.intrinsics.arr_push,
        Operand::Value(cur_arr),
        push_arg,
        args[0],
        val,
        elem_ty,
        val_owned_from_substr,
        arr_ty,
    );
    // B1 (RFC 20260706-arr-grow-alias-stability): the cell never
    // moves, so the historical slot write-back + captured-env mirror
    // are retired — every alias already sees the grown buffer.
    // chunk 9c — push spec parity: ret new length per JS spec §22.1.3.20.
    let new_len = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(new_arr), ARR_LEN_OFF),
        Type::I64,
        None,
    );
    Some(Operand::Value(new_len))
}

/// K.8 — Ident-receiver where the binding is a top-level refcount global
/// (registered by the K.6 globals pass). Load cur ptr from the global slot
/// via `GlobalRef`, push, store back. Mirror semantic of the local path
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
    if matches!(elem_ty, Type::Any) {
        return Some(ctx.emit_arr_any_push_at_slot(Operand::Value(slot_ptr), 0, args[0], arr_ty));
    }
    let cur_arr = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(arr_ty, Operand::Value(slot_ptr), 0),
        arr_ty,
        None,
    );
    let (val, val_owned_from_substr) = coerce_push_value(ctx, args[0], elem_ty);
    let push_arg = ctx.raw_slot_arg(val);
    let new_arr = emit_grow_kernel_checked(
        ctx,
        ctx.intrinsics.arr_push,
        Operand::Value(cur_arr),
        push_arg,
        args[0],
        val,
        elem_ty,
        val_owned_from_substr,
        arr_ty,
    );
    // B1 — cell fixed across grow; global-slot write-back retired.
    let new_len = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(new_arr), ARR_LEN_OFF),
        Type::I64,
        None,
    );
    Some(Operand::Value(new_len))
}

/// (b) `obj.field.push(v)` — field-receiver path. Load the struct pointer
/// once (borrow), find the field's offset, then load → push → store-back
/// at that offset. P0.10 mirror: `class W { items: any[] = [] }` allocates
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
    if matches!(elem_ty, Type::Any) {
        return Some(ctx.emit_arr_any_push_at_slot(obj_val, offset, args[0], field_ty));
    }
    let cur_arr = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(field_ty, obj_val, offset),
        field_ty,
        None,
    );
    // The shared push coercion (chain-mark, bool → i64, W4 width
    // align, Substr / Any materialize) — the field path's historical
    // inline copy lacked the Substr and Any arms, so an admitted Any
    // arg would have raw-written box bits into the typed slot.
    let (val, val_owned_from_substr) = coerce_push_value(ctx, args[0], elem_ty);
    let push_arg = ctx.raw_slot_arg(val);
    let new_arr = emit_grow_kernel_checked(
        ctx,
        ctx.intrinsics.arr_push,
        Operand::Value(cur_arr),
        push_arg,
        args[0],
        val,
        elem_ty,
        val_owned_from_substr,
        field_ty,
    );
    // B1 — cell fixed across grow; field write-back retired.
    let new_len = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(new_arr), ARR_LEN_OFF),
        Type::I64,
        None,
    );
    Some(Operand::Value(new_len))
}
