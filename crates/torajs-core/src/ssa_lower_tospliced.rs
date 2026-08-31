//! `Array.prototype.toSpliced` (ES2023 §23.1.3.42) lowering — carved
//! out of `ssa_lower.rs::lower_expr_inner` to keep the immutable
//! splice pattern out of the 27k-line god-file.
//!
//! Emit shape: `arr_slice(arr, 0, len)` produces a fresh deep-copy
//! receiver (refcounted elems get `emit_arr_rc_inc_range` per the
//! existing `Array.from(<typed Array<T>>)` shallow-copy idiom), then
//! `arr_splice(clone, start, deleteCount)` mutates the clone in-place
//! (no realloc per the splice doc above — only shrinks the live
//! range), and the returned removed sub-array is dropped via
//! `emit_drop_value`. Source array stays untouched (unlike `splice`).
//!
//! 0-2 args splice the clone shrink-only; 3+ args (RFC
//! 20260720-splice-insert knife 3) route the `...items` tail through
//! the mutating sibling's [`crate::ssa_lower_splice::emit_splice_items`]
//! on the clone. Receiver dispatch mirrors `splice`: (a) Ident bound
//! to a `Type::Arr` local, (b) Ident bound to a K.8 top-level
//! refcount global.
//!
//! Returning `Some` here short-circuits the generic member-call
//! dispatch in `ssa_lower.rs`; `None` falls through to the
//! "unsupported member call shape" panic site.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx};

/// Single entry point — try all receiver shapes in turn.
pub(crate) fn try_lower(
    ctx: &mut LowerCtx,
    callee_eid: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    if let Some(v) = try_lower_local(ctx, callee_eid, args) {
        return Some(v);
    }
    if let Some(v) = try_lower_global(ctx, callee_eid, args) {
        return Some(v);
    }
    try_lower_generic(ctx, callee_eid, args)
}

/// (c) Fallback — any expression that lowers to `Type::Arr` (literal
/// arrays, method chains returning Array, struct-field reads, etc.).
/// Drops the receiver after the clone (it was a fresh-owned value
/// when the receiver was a non-Ident expression — the source array
/// stays referenced through the clone's own ARC bumps).
fn try_lower_generic(ctx: &mut LowerCtx, callee_eid: ExprId, args: &[ExprId]) -> Option<Operand> {
    let Expr::Member { obj: recv_id, name } = ctx.ast.get_expr(callee_eid) else {
        return None;
    };
    if name != "toSpliced" {
        return None;
    }
    let recv_eid = *recv_id;
    let recv_op = ctx.lower_expr(recv_eid);
    let recv_ty = ctx.operand_ty(&recv_op);
    let Type::Arr(arr_id) = recv_ty else {
        return None;
    };
    let cur_arr = match recv_op {
        Operand::Value(v) => v,
        _ => return None,
    };
    let clone = emit_clone_splice_return(ctx, arr_id, cur_arr, args);
    // The receiver expression produced a fresh-owned array (literal
    // or call result); the clone's elements already hold their own
    // ARC bumps via `emit_arr_rc_inc_range`, so drop the source to
    // balance ownership.
    if ctx.expr_is_fresh_owned(recv_eid) {
        ctx.emit_drop_value(Operand::Value(cur_arr), recv_ty);
    }
    Some(clone)
}

/// (a) `xs.toSpliced(start, deleteCount)` where `xs` is an Ident bound
/// to a `Type::Arr` local — load cur ptr from the slot, emit the
/// shared pattern via [`emit_clone_splice_return`].
fn try_lower_local(ctx: &mut LowerCtx, callee_eid: ExprId, args: &[ExprId]) -> Option<Operand> {
    let Expr::Member { obj: recv_id, name } = ctx.ast.get_expr(callee_eid) else {
        return None;
    };
    if name != "toSpliced" {
        return None;
    }
    let Expr::Ident(recv_name) = ctx.ast.get_expr(*recv_id) else {
        return None;
    };
    let info = ctx.locals.get(recv_name).copied()?;
    let Type::Arr(arr_id) = info.ty else {
        return None;
    };
    let arr_ty = info.ty;
    let cur_arr = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(arr_ty, Operand::Value(info.slot), 0),
        arr_ty,
        None,
    );
    Some(emit_clone_splice_return(ctx, arr_id, cur_arr, args))
}

/// (b) `xs.toSpliced(start, deleteCount)` where `xs` is an Ident bound
/// to a K.8 top-level refcount global — load cur ptr via `GlobalRef`,
/// emit the shared pattern via [`emit_clone_splice_return`].
fn try_lower_global(ctx: &mut LowerCtx, callee_eid: ExprId, args: &[ExprId]) -> Option<Operand> {
    let Expr::Member { obj: recv_id, name } = ctx.ast.get_expr(callee_eid) else {
        return None;
    };
    if name != "toSpliced" {
        return None;
    }
    let Expr::Ident(recv_name) = ctx.ast.get_expr(*recv_id) else {
        return None;
    };
    if ctx.locals.get(recv_name).is_some() {
        return None;
    }
    let slot_ty = ctx.globals.get(recv_name).copied()?;
    let Type::Arr(arr_id) = slot_ty else {
        return None;
    };
    let arr_ty = slot_ty;
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
    Some(emit_clone_splice_return(ctx, arr_id, cur_arr, args))
}

/// Shared body: clone via `arr_slice(arr, 0, len)`, rc_inc the cloned
/// range when elems are refcounted, splice on the clone, drop the
/// removed sub-array, return the clone.
fn emit_clone_splice_return(
    ctx: &mut LowerCtx,
    arr_id: crate::ssa::ArrId,
    cur_arr: crate::ssa::ValueId,
    args: &[ExprId],
) -> Operand {
    let elem_ty = ctx.arr_layouts[arr_id.0 as usize];
    // The clone of an `Arr<Substr>` is `Arr<Str>` (views do not leave
    // their split block — rotation 468); the splice below and the
    // removed-array drop read the clone's layout, not the receiver's.
    let arr_ty = Type::Arr(ctx.copied_arr_layout(arr_id));
    let len = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(cur_arr), ARR_LEN_OFF),
        Type::I64,
        None,
    );
    // 刀 13c (RFC 20260721-array-proto-cluster) — an Any-elem
    // receiver gathers through the exotic-aware `arr_any_slice` per
    // §23.1.3.42 steps 16/18 [[Get]] order (accessor indexes run
    // their getter, holes continue to the prototype digit keys, a
    // mid-gather length mutation reads through per live index); the
    // splice below then operates on the clean dense clone — slot ops
    // there are equivalent to the spec's per-index
    // CreateDataProperty. The kernel incs each slot itself (owned
    // contract) — no rc_inc range. Typed receivers keep the raw
    // `arr_slice` memcpy + inc range.
    let clone = if elem_ty == Type::Any {
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.arr_any_slice,
                vec![
                    Operand::Value(cur_arr),
                    Operand::ConstI64(0),
                    Operand::Value(len),
                ],
            ),
            arr_ty,
            None,
        );
        ctx.emit_throw_check(None);
        v
    } else {
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.arr_slice,
                vec![
                    Operand::Value(cur_arr),
                    Operand::ConstI64(0),
                    Operand::Value(len),
                ],
            ),
            arr_ty,
            None,
        );
        if elem_ty.is_refcounted() {
            let clone_len = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(Type::I64, Operand::Value(v), ARR_LEN_OFF),
                Type::I64,
                None,
            );
            ctx.emit_adopt_copied_range(
                Operand::Value(v),
                elem_ty,
                Operand::ConstI64(0),
                Operand::Value(clone_len),
            );
        }
        v
    };
    // Arity defaults mirror `ssa_lower_splice::emit_splice_return`:
    // 0-arg → `start = 0` / `deleteCount = 0` (clone is returned
    // unchanged); 1-arg → `deleteCount = i64::MAX` sentinel that the
    // `__torajs_arr_splice` helper clamps to `len - actualStart`
    // (the spec §23.1.3.42 step 4-6 defaults).
    let start = if args.is_empty() {
        Operand::ConstI64(0)
    } else {
        // §23.1.3.42 step 3 ToIntegerOrInfinity(start) — same i64
        // helper ABI, same hazard, as the mutating sibling.
        ctx.lower_to_index_operand(args[0])
    };
    // S237 — `arr.toSpliced(start, undefined)` mirrors splice undef
    // handling per ES §23.1.3.42 step 7: ToIntegerOrInfinity(undef)=0,
    // so the explicit-undefined deleteCount yields 0 removal (distinct
    // from omitted-arg 1-arg `len - actualStart` default).
    let delete_count = match args.len() {
        0 => Operand::ConstI64(0),
        1 => Operand::ConstI64(i64::MAX),
        _ => {
            if matches!(
                ctx.expr_types.get(&args[1]),
                Some(crate::check::Type::Undefined)
            ) {
                Operand::ConstI64(0)
            } else {
                ctx.lower_to_index_operand(args[1])
            }
        }
    };
    // RFC 20260720-splice-insert knife 3 — 3+ args splice the
    // `...items` tail into the clone (same emit as the mutating
    // sibling); the removed product is dropped either way.
    let removed = if args.len() > 2 {
        let r = crate::ssa_lower_splice::emit_splice_items(
            ctx,
            arr_ty,
            clone,
            start,
            delete_count,
            &args[2..],
        );
        match r {
            Operand::Value(v) => v,
            _ => unreachable!("emit_splice_items answers a Value"),
        }
    } else {
        ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.arr_splice,
                vec![Operand::Value(clone), start, delete_count],
            ),
            arr_ty,
            None,
        )
    };
    ctx.emit_drop_value(Operand::Value(removed), arr_ty);
    Operand::Value(clone)
}
