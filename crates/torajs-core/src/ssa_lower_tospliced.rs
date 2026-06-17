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
//! Subset matches `splice`'s fixed 2-arg shape; the variadic
//! `...items` insert form is a follow-up. Receiver dispatch mirrors
//! `splice`: (a) Ident bound to a `Type::Arr` local, (b) Ident bound
//! to a K.8 top-level refcount global.
//!
//! Returning `Some` here short-circuits the generic member-call
//! dispatch in `ssa_lower.rs`; `None` falls through to the
//! "unsupported member call shape" panic site.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx};

/// Single entry point — try both receiver shapes in turn.
pub(crate) fn try_lower(
    ctx: &mut LowerCtx,
    callee_eid: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    if let Some(v) = try_lower_local(ctx, callee_eid, args) {
        return Some(v);
    }
    try_lower_global(ctx, callee_eid, args)
}

/// (a) `xs.toSpliced(start, deleteCount)` where `xs` is an Ident bound
/// to a `Type::Arr` local — load cur ptr from the slot, emit the
/// shared pattern via [`emit_clone_splice_return`].
fn try_lower_local(ctx: &mut LowerCtx, callee_eid: ExprId, args: &[ExprId]) -> Option<Operand> {
    let Expr::Member { obj: recv_id, name } = ctx.ast.get_expr(callee_eid) else {
        return None;
    };
    if name != "toSpliced" || args.len() != 2 {
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
    Some(emit_clone_splice_return(ctx, arr_ty, arr_id, cur_arr, args))
}

/// (b) `xs.toSpliced(start, deleteCount)` where `xs` is an Ident bound
/// to a K.8 top-level refcount global — load cur ptr via `GlobalRef`,
/// emit the shared pattern via [`emit_clone_splice_return`].
fn try_lower_global(ctx: &mut LowerCtx, callee_eid: ExprId, args: &[ExprId]) -> Option<Operand> {
    let Expr::Member { obj: recv_id, name } = ctx.ast.get_expr(callee_eid) else {
        return None;
    };
    if name != "toSpliced" || args.len() != 2 {
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
    Some(emit_clone_splice_return(ctx, arr_ty, arr_id, cur_arr, args))
}

/// Shared body: clone via `arr_slice(arr, 0, len)`, rc_inc the cloned
/// range when elems are refcounted, splice on the clone, drop the
/// removed sub-array, return the clone.
fn emit_clone_splice_return(
    ctx: &mut LowerCtx,
    arr_ty: Type,
    arr_id: crate::ssa::ArrId,
    cur_arr: crate::ssa::ValueId,
    args: &[ExprId],
) -> Operand {
    let elem_ty = ctx.arr_layouts[arr_id.0 as usize];
    let len = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(cur_arr), ARR_LEN_OFF),
        Type::I64,
        None,
    );
    let clone = ctx.f.append_inst(
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
            InstKind::Load(Type::I64, Operand::Value(clone), ARR_LEN_OFF),
            Type::I64,
            None,
        );
        ctx.emit_arr_rc_inc_range(
            Operand::Value(clone),
            Operand::ConstI64(0),
            Operand::Value(clone_len),
        );
    }
    let start = ctx.lower_expr(args[0]);
    let delete_count = ctx.lower_expr(args[1]);
    let removed = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.arr_splice,
            vec![Operand::Value(clone), start, delete_count],
        ),
        arr_ty,
        None,
    );
    ctx.emit_drop_value(Operand::Value(removed), arr_ty);
    Operand::Value(clone)
}
