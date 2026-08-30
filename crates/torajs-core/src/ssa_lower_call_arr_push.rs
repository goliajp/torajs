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
//!   `arr_ptr + head_off + len*8`. The Phase 2 captured-array writeback
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
    if name != "push" || args.len() != 1 {
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
fn try_lower_index(ctx: &mut LowerCtx<'_>, recv_id: ExprId, args: &[ExprId]) -> Option<Operand> {
    if !matches!(ctx.ast.get_expr(recv_id), Expr::Index { .. }) {
        return None;
    }
    let recv_val = ctx.lower_expr(recv_id);
    let recv_ty = ctx.operand_ty(&recv_val);
    let arr_id = match recv_ty {
        Type::Arr(id) => id,
        _ => return None,
    };
    let elem_ty = ctx.arr_layouts[arr_id.0 as usize];
    if matches!(elem_ty, Type::Any) {
        return Some(ctx.emit_arr_any_push_at_value(recv_val, args[0], recv_ty));
    }
    let (val, val_owned_from_substr) = coerce_push_value(ctx, args[0], elem_ty);
    let push_arg = ctx.raw_slot_arg(val);
    let new_arr = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.arr_push, vec![recv_val, push_arg]),
        recv_ty,
        None,
    );
    if elem_ty.is_refcounted() && !val_owned_from_substr {
        ctx.emit_rc_inc(val);
        // Chunk 733 — an owned-shape arg (closure literal / call
        // result / array literal) hands its +1 to the array: the inc
        // above is the array's stake, so the temp's own must release
        // (probe P1 closure-push churn 30.5MB / P6 nested-arr 44.9MB
        // / P7 owned-str 16MB vs 6.3MB flat). Borrow shapes are a
        // no-op through the predicate. The substr-coerce lane skips
        // (its fresh rc=1 already IS the hand-off).
        ctx.release_owned_temp(args[0], &val);
    }
    let new_len = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(new_arr), ARR_LEN_OFF),
        Type::I64,
        None,
    );
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
    // slot at data_ptr + head_off + len*8, bump len_slot (B1: base is
    // the hoisted data pointer; the reserve guarantees it stays valid).
    if let Some(state) = ctx.prereserve.unchecked_for.get(recv_name).copied() {
        let len_next = ctx.emit_prereserved_push(state, val.clone());
        if elem_ty.is_refcounted() && !val_owned_from_substr {
            ctx.emit_rc_inc(val);
            // Chunk 733 — owned-shape arg hand-off (see try_lower_index).
            ctx.release_owned_temp(args[0], &val);
        }
        // chunk 9c — fast-push spec parity: ret new length (already in SSA
        // as `len_next`, mirrors arr[#8]).
        return Some(Operand::Value(len_next));
    }
    let push_arg = ctx.raw_slot_arg(val);
    let new_arr = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.arr_push,
            vec![Operand::Value(cur_arr), push_arg],
        ),
        arr_ty,
        None,
    );
    if elem_ty.is_refcounted() && !val_owned_from_substr {
        ctx.emit_rc_inc(val);
        // Chunk 733 — owned-shape arg hand-off (see try_lower_index).
        ctx.release_owned_temp(args[0], &val);
    }
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
    if ctx.locals.get(recv_name).is_some() {
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
    let new_arr = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.arr_push,
            vec![Operand::Value(cur_arr), push_arg],
        ),
        arr_ty,
        None,
    );
    if elem_ty.is_refcounted() && !val_owned_from_substr {
        ctx.emit_rc_inc(val);
        // Chunk 733 — owned-shape arg hand-off (see try_lower_index).
        ctx.release_owned_temp(args[0], &val);
    }
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
    let push_arg = ctx.raw_slot_arg(val.clone());
    let new_arr = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.arr_push,
            vec![Operand::Value(cur_arr), push_arg],
        ),
        field_ty,
        None,
    );
    if elem_ty.is_refcounted() && !val_owned_from_substr {
        ctx.emit_rc_inc(val.clone());
        // Chunk 733 — owned-shape arg hand-off (see try_lower_index).
        ctx.release_owned_temp(args[0], &val);
    }
    // B1 — cell fixed across grow; field write-back retired.
    let new_len = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(new_arr), ARR_LEN_OFF),
        Type::I64,
        None,
    );
    Some(Operand::Value(new_len))
}

/// Lower args[0] and apply elem-width / type coercions shared between the
/// (a) ident-local and K.8 ident-global paths:
/// - bool → i64 coerce.
/// - i64 → f64 widen for F64-elem arrays (W4).
/// - F64 elem receiving F64 value: panic — container-width analysis missed.
/// - Substr → owned Str via `substr_to_owned` for `for (const c of "abc")
///   chars.push(c)` (for-of-str loop binds `c` as Substr 16-byte view, but
///   Arr<Str> slots are 8-byte heap-Str pointers). Returned `Operand` is
///   fresh rc=1 with no caller binding, so the share-inc is skipped.
///
/// Returns `(val_operand, val_owned_from_substr)` — the second flag tells
/// the caller to skip `emit_rc_inc` on the value (the substr_to_owned
/// already allocated a fresh ref).
pub(crate) fn coerce_push_value(
    ctx: &mut LowerCtx<'_>,
    arg_eid: ExprId,
    elem_ty: Type,
) -> (Operand, bool) {
    let mut val = ctx.lower_expr(arg_eid);
    // Chunk 575 — an array value entering a container slot must be
    // self-describing for the cycle walker (chain mark; no-op for
    // non-Arr elem types). See index-assign twin.
    ctx.emit_arr_mark_kind(&val);
    val = ctx.coerce_bool_to_i64(val);
    let mut val_owned_from_substr = false;
    val = match (elem_ty, ctx.operand_ty(&val)) {
        (Type::F64, Type::I64) => ctx.coerce_to_f64(val),
        // Any arg into a scalar/Str elem slot — unbox at the store
        // boundary (checker push/unshift 1-arg Any admit pairing).
        // The coerced Str is fresh rc=1, so it rides the same
        // skip-inc hand-off flag as the substr lane; the original
        // box's stake settles when the arg was an owned temp
        // (any-arith result), while borrow shapes stay with their
        // binding.
        (t @ (Type::F64 | Type::I64), Type::Any) => {
            let n = ctx.coerce_any_to_number(val.clone(), t);
            if ctx.expr_transfers_ownership(arg_eid) {
                ctx.emit_drop_value(val, Type::Any);
            }
            n
        }
        // A read of a bool array comes back as a tagged value (it has
        // an out-of-range exit to describe), so pushing one straight
        // into another bool array arrives here. Unboxing is exact for
        // every value a bool slot can hold; a promoted `undefined`
        // lands on `false`, which is the only answer the slot has.
        (Type::Bool, Type::Any) => {
            let b = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.any_to_bool, vec![val.clone()]),
                Type::Bool,
                None,
            );
            if ctx.expr_transfers_ownership(arg_eid) {
                ctx.emit_drop_value(val, Type::Any);
            }
            Operand::Value(b)
        }
        (Type::Str, Type::Any) => {
            let s = ctx.coerce_to_str(val.clone(), Type::Any);
            if ctx.expr_transfers_ownership(arg_eid) {
                ctx.emit_drop_value(val, Type::Any);
            }
            val_owned_from_substr = true;
            s
        }
        (Type::I64, Type::F64) => panic!(
            "ssa-lower: f64 value into i64 array elem via push — \
             container width analysis missed this write"
        ),
        (Type::Str, Type::Substr) => {
            let owned = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.substr_to_owned, vec![val.clone()]),
                Type::Str,
                None,
            );
            // Chunk 743 — substr_to_owned READS the view without
            // consuming it: a fresh view mint (string index / slice
            // call) hands this lane its only ref, which nothing else
            // drops (churn: 300k `ss.push(s[i%8])` 15.96MB vs 6.3MB
            // flat — unshift twin identical). Borrow views (ident
            // reads of a live binding) stay with their owner.
            if ctx.expr_transfers_ownership(arg_eid) {
                ctx.emit_drop_value(val, Type::Substr);
            }
            val_owned_from_substr = true;
            Operand::Value(owned)
        }
        _ => val,
    };
    (val, val_owned_from_substr)
}
