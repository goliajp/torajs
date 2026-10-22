//! The update `d[k] = (d[k] ?? 0) + 1` on a number dictionary with one
//! probe: the read on the right-hand side answers the own entry its
//! value came from, and the write hands that entry to
//! `__torajs_dynobj_dict_set_entry`, which stores into it instead of
//! probing again. The call site keeps the shape of the plain write:
//! one call and its throw check.
//!
//! The entry pointer is good only while no program code runs: a
//! getter, a `valueOf` or a call may add keys (moving the entry table)
//! or redefine the entry. So the write asks for the entry only when
//! the read sits where it is always evaluated and everything the
//! right-hand side evaluates besides it is a number literal, a
//! number-typed binding or a non-short-circuit operator over those and
//! the read's answer, which is a number whenever an entry is answered.
//! Code that runs before the read's probe (the read's own key) cannot
//! stale the pointer. A request the read did not answer (the read was
//! not this dictionary's keyed read) leaves the write on the call.
//!
//! The store is §10.1.9.2 OrdinarySetWithOwnDescriptor on an own
//! writable data property of the receiver itself: the new value
//! replaces [[Value]] and the attributes stay. The read answers an
//! entry only when it is own, writable, holds a number and is not on a
//! module namespace (whose [[Set]] refuses every write), and only when
//! the read's receiver box and key cell are the write's: the target's
//! key expression runs between the two evaluations of the receiver
//! and may rebind it, and the two key expressions may answer different
//! strings. A number in the entry is an immediate, so the store
//! releases nothing, and the value's stake moves into the entry.

use crate::ast::{BinOp, Expr, ExprId, UnaryOp};
use crate::ssa::{Operand, Type, ValueId};
use crate::ssa_lower::LowerCtx;

/// A write's request to the read keyed by `index`: the write's
/// receiver and key, and, once the read took the request, the slot it
/// answers the entry in (null when there is none to overwrite).
#[derive(Clone, Copy)]
pub(crate) struct Request {
    index: ExprId,
    write_recv: Operand,
    write_key: ValueId,
    slot: Option<ValueId>,
}

/// Ask the read of `recv[…]` in `value`, the right-hand side of the
/// write `recv[key] = value` whose receiver and key lowered to
/// `write_recv` and `write_key`, to answer its entry, when the shape
/// keeps the pointer good up to the store (the module doc).
pub(crate) fn ask(
    ctx: &mut LowerCtx<'_>,
    recv: ExprId,
    value: ExprId,
    write_recv: Operand,
    write_key: ValueId,
) {
    let ast = ctx.ast;
    let Expr::Ident(name) = ast.get_expr(recv) else {
        return;
    };
    let mut read = None;
    if pure_around_read(ctx, name, value, true, &mut read)
        && let Some(index) = read
    {
        ctx.binop.dict_entry = Some(Request {
            index,
            write_recv,
            write_key,
            slot: None,
        });
    }
}

/// True when `e` evaluates nothing but number literals, number-typed
/// bindings, non-short-circuit operators and at most one keyed read
/// of `recv` (recorded in `read`), placed where `always` says every
/// evaluation of the whole right-hand side reaches it.
fn pure_around_read(
    ctx: &LowerCtx<'_>,
    recv: &str,
    e: ExprId,
    always: bool,
    read: &mut Option<ExprId>,
) -> bool {
    match ctx.ast.get_expr(e) {
        Expr::Number(_) => true,
        Expr::Ident(_) => matches!(ctx.expr_types.get(&e), Some(crate::check::Type::Number)),
        Expr::Index { obj, index } => {
            let on_recv = matches!(ctx.ast.get_expr(*obj), Expr::Ident(n) if n == recv);
            if !(always && on_recv && read.is_none()) {
                return false;
            }
            *read = Some(*index);
            true
        }
        Expr::BinOp { op, left, right } => {
            !matches!(op, BinOp::LAnd | BinOp::LOr)
                && pure_around_read(ctx, recv, *left, always, read)
                && pure_around_read(ctx, recv, *right, always, read)
        }
        Expr::Unary { op, expr } => {
            matches!(op, UnaryOp::Neg | UnaryOp::Plus | UnaryOp::BitNot)
                && pure_around_read(ctx, recv, *expr, always, read)
        }
        Expr::Nullish { lhs, rhs } => {
            pure_around_read(ctx, recv, *lhs, always, read)
                && pure_around_read(ctx, recv, *rhs, false, read)
        }
        _ => false,
    }
}

/// The read keyed by `index` takes the request: the operands it adds
/// to the runtime call (the entry slot, the write's receiver and key),
/// `None` when nothing asked.
pub(crate) fn answer(ctx: &mut LowerCtx<'_>, index: ExprId) -> Option<[Operand; 3]> {
    let req = ctx.binop.dict_entry?;
    if req.index != index || req.slot.is_some() {
        return None;
    }
    let slot = ctx.alloca_in_entry(Type::Ptr, Some("__dict_entry"));
    ctx.binop.dict_entry = Some(Request {
        slot: Some(slot),
        ..req
    });
    Some([
        Operand::Value(slot),
        req.write_recv,
        Operand::Value(req.write_key),
    ])
}

/// The entry slot once the right-hand side is lowered, when the read
/// took the request; clears the request either way.
pub(crate) fn take(ctx: &mut LowerCtx<'_>) -> Option<ValueId> {
    ctx.binop.dict_entry.take().and_then(|r| r.slot)
}
