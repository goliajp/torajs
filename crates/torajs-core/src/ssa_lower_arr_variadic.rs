//! `recv.push(a, b, …)` / `recv.unshift(a, b, …)` on a typed array,
//! and the empty `recv.unshift()`.
//!
//! §13.3.8.1 ArgumentListEvaluation runs every argument before
//! §23.1.3.23 / §23.1.3.35 append the first element. The multi-argument
//! forms used to be split by an AST pass into one single-argument call
//! per value, which appended between arguments (`xs.push(1, xs.length)`
//! read the grown length), ran unshift's arguments in reverse, and
//! only knew statement positions — `console.log(xs.push(2, 3))` and
//! every temporary receiver (`[1].push(2, 3)`) died at lowering with
//! "unsupported member call shape".
//!
//! Here the receiver is evaluated once, then each argument in order;
//! each value is pinned (`SharedPins::operands`, the assignment forms'
//! single-evaluation contract), so the one-value grow lanes read it
//! back as a borrow and take the array's stake with their own inc.
//! What an argument owned is released once every value is stored, and
//! parked on the throw edges until then.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx};

/// One evaluated argument: its eid, operand, and the throw-edge token
/// of the reference it owned before the pin made it read as a borrow.
struct Held {
    eid: ExprId,
    op: Operand,
    owned: Option<usize>,
}

/// `Some(new length)` when `callee` is `push` / `unshift` with two or
/// more arguments (or `unshift` with none) on a statically typed array;
/// `None` leaves the site to the other lanes.
pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let Expr::Member { obj, name } = ctx.ast.get_expr(callee) else {
        return None;
    };
    let (recv_id, unshift) = match name.as_str() {
        "push" => (*obj, false),
        "unshift" => (*obj, true),
        _ => return None,
    };
    if unshift && args.is_empty() {
        // §23.1.3.35 with no arguments still performs the length Set,
        // exactly the empty push's single write.
        return crate::ssa_lower_call_arr_push_empty::try_lower(ctx, recv_id);
    }
    if args.len() < 2 || !statically_array(ctx, recv_id) {
        return None;
    }
    let recv = ctx.lower_expr(recv_id);
    let Type::Arr(arr_id) = ctx.operand_ty(&recv) else {
        ctx.redispatch_lowered = Some((recv_id, recv));
        return None;
    };
    let arr_ty = Type::Arr(arr_id);
    crate::ssa_lower_nullable_guard::emit_undefable_heap_guard(ctx, recv_id, &recv);
    let recv_tok = ctx.park_owned_temp(recv_id, &recv);
    let held: Vec<Held> = args.iter().map(|&a| hold_arg(ctx, a)).collect();
    let elem_ty = ctx.arr_layouts[arr_id.0 as usize];
    let order: Vec<&Held> = if unshift {
        held.iter().rev().collect()
    } else {
        held.iter().collect()
    };
    let mut len = Operand::ConstI64(0);
    for h in order {
        len = store_one(ctx, &recv, h.eid, elem_ty, arr_ty, unshift);
    }
    for h in held {
        release_arg(ctx, h);
    }
    ctx.unpark_owned_temp(recv_tok);
    ctx.release_owned_temp(recv_id, &recv);
    Some(len)
}

/// The empty-push module's receiver test: a fn-local answers from the
/// local table, a promoted global only from the checker's type.
fn statically_array(ctx: &LowerCtx<'_>, recv_id: ExprId) -> bool {
    matches!(
        crate::ssa_lower_recv_face::static_ty(ctx, recv_id),
        Some(Type::Arr(_))
    ) || matches!(
        ctx.expr_types.get(&recv_id),
        Some(crate::check::Type::Array(_))
    )
}

/// Evaluate one argument, record (and park) what it owned, then pin it.
fn hold_arg(ctx: &mut LowerCtx<'_>, eid: ExprId) -> Held {
    let op = ctx.lower_expr(eid);
    let ty = ctx.operand_ty(&op);
    let owns = ctx.expr_transfers_ownership(eid) || ctx.expr_minted_closure(eid, &op);
    let owned = (owns && !ty.is_copy()).then(|| ctx.push_throw_temp(op, ty));
    ctx.pins.operands.insert(eid, op);
    Held { eid, op, owned }
}

/// Unpin one argument and drop the reference it owned: the array took
/// its own with the grow lane's inc.
fn release_arg(ctx: &mut LowerCtx<'_>, h: Held) {
    ctx.pins.operands.remove(&h.eid);
    if let Some(tok) = h.owned {
        ctx.pop_throw_temp(tok);
        let ty = ctx.operand_ty(&h.op);
        ctx.emit_drop_value(h.op, ty);
    }
}

/// Append one pinned value through the single-value lane and answer
/// the length after it.
fn store_one(
    ctx: &mut LowerCtx<'_>,
    recv: &Operand,
    arg: ExprId,
    elem_ty: Type,
    arr_ty: Type,
    unshift: bool,
) -> Operand {
    if matches!(elem_ty, Type::Any) {
        return if unshift {
            ctx.emit_arr_any_unshift_at_value(*recv, arg, arr_ty)
        } else {
            ctx.emit_arr_any_push_at_value(*recv, arg, arr_ty)
        };
    }
    let (val, owned_from_substr) =
        crate::ssa_lower_call_arr_push::coerce_push_value(ctx, arg, elem_ty);
    let slot = ctx.raw_slot_arg(val);
    let fid = if unshift {
        ctx.intrinsics.arr_unshift
    } else {
        ctx.intrinsics.arr_push
    };
    let new_arr = crate::ssa_lower_arr_grow_kernel::emit_grow_kernel_checked(
        ctx,
        fid,
        *recv,
        slot,
        arg,
        val,
        elem_ty,
        owned_from_substr,
        arr_ty,
    );
    let len = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(new_arr), ARR_LEN_OFF),
        Type::I64,
        None,
    );
    Operand::Value(len)
}
