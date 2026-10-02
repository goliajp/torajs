//! `xs.push()` — the empty-argument push (rotation 651); the empty
//! `xs.unshift()` rides it too (`ssa_lower_arr_variadic`).
//!
//! §23.1.3.23 with no arguments appends nothing and answers the
//! length (test262 S15.4.4.7_A1_T1 and siblings). The typed push arm
//! next door handled exactly one argument and the generic lane had no
//! shape for the empty list, so the program died at lowering with
//! "unsupported member call shape: push". Its own file because the
//! one-argument arm's file sits on the 500-line limit.

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx};

/// `Some(length)` when the receiver is statically an array; `None`
/// leaves the site to the any lane. The arr cell is fixed across grow
/// (B1), so the receiver expression IS the cell to read.
pub(crate) fn try_lower(ctx: &mut LowerCtx<'_>, recv_id: ExprId) -> Option<Operand> {
    // A fn-local answers from the local table; a promoted global
    // (`const ys: number[] = []` at top level) only from the checker's
    // type (`recv_face::static_ty` names heap layouts that need no
    // element type, and `Array<T>` is not one).
    let is_arr = matches!(
        crate::ssa_lower_recv_face::static_ty(ctx, recv_id),
        Some(Type::Arr(_))
    ) || matches!(
        ctx.expr_types.get(&recv_id),
        Some(crate::check::Type::Array(_))
    );
    if !is_arr {
        return None;
    }
    let recv_op = ctx.lower_expr(recv_id);
    crate::ssa_lower_nullable_guard::emit_undefable_heap_guard(ctx, recv_id, &recv_op);
    // §23.1.3.23 step 6 — the one write an empty push performs is
    // `length` (the same value), which a frozen array or a
    // non-writable length refuses with a TypeError (rotation 652;
    // `Object.freeze([]).push()` answered 0). Off the hot path: the
    // appending arm next door never comes here.
    let guard = ctx.intrinsics.arr_len_write_guard;
    let cur_block = ctx.cur_block;
    ctx.f
        .append_void(cur_block, InstKind::Call(guard, vec![recv_op]));
    ctx.emit_throw_check(None);
    let len = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, recv_op, ARR_LEN_OFF),
        Type::I64,
        None,
    );
    ctx.release_owned_temp(recv_id, &recv_op);
    Some(Operand::Value(len))
}
