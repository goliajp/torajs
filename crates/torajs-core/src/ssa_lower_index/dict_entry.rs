//! The update `d[k] = (d[k] ?? 0) + 1` on a number dictionary with one
//! probe: the read on the right-hand side calls
//! `__torajs_dynobj_dict_get_entry`, which notes the own entry its
//! value came from, and the write calls
//! `__torajs_dynobj_dict_set_entry`, which stores into that entry
//! instead of probing again. Both call sites keep the shape and the
//! operands of the plain read and write: one call and its throw check.
//!
//! The noted entry is good only while no program code runs: a getter,
//! a `valueOf` or a call may add keys (moving the entry table) or
//! redefine the entry. So the write asks for the entry only when the
//! read sits where it is always evaluated and everything the
//! right-hand side evaluates besides it is a number literal, a
//! number-typed binding or a non-short-circuit operator over those and
//! the read's answer, which is a number whenever an entry is noted.
//! The write then runs right after its read, with no other entry read
//! in between. Code that runs before the read's probe (the read's own
//! key) cannot stale the entry. A request the read did not take (the
//! read was not this dictionary's keyed read) leaves the write on the
//! plain call.
//!
//! The store is §10.1.9.2 OrdinarySetWithOwnDescriptor on an own
//! writable data property of the receiver itself: the new value
//! replaces [[Value]] and the attributes stay. The read notes an entry
//! only when it is own, writable, holds a number and is not on a
//! module namespace (whose [[Set]] refuses every write), and the write
//! uses it only when the read's receiver box and key cell are the
//! write's: the target's key expression runs between the two
//! evaluations of the receiver and may rebind it, and the two key
//! expressions may answer different strings. A number in the entry is
//! an immediate, so the store releases nothing, and the value's stake
//! moves into the entry.

use crate::ast::{BinOp, Expr, ExprId, UnaryOp};
use crate::ssa_lower::LowerCtx;

/// A write's request to the read keyed by `index`, and whether the
/// read took it.
#[derive(Clone, Copy)]
pub(crate) struct Request {
    index: ExprId,
    taken: bool,
}

/// Ask the read of `recv[…]` in `value`, the right-hand side of the
/// write `recv[key] = value`, to note its entry, when the shape keeps
/// the entry good up to the store (the module doc).
pub(crate) fn ask(ctx: &mut LowerCtx<'_>, recv: ExprId, value: ExprId) {
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
            taken: false,
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

/// The read keyed by `index` takes the request: true when the write
/// around it asked, so the read is the one that notes its entry.
pub(crate) fn answer(ctx: &mut LowerCtx<'_>, index: ExprId) -> bool {
    let Some(req) = ctx.binop.dict_entry else {
        return false;
    };
    if req.index != index || req.taken {
        return false;
    }
    ctx.binop.dict_entry = Some(Request { taken: true, ..req });
    true
}

/// True when the read on the right-hand side, now lowered, took the
/// request; clears the request either way.
pub(crate) fn take(ctx: &mut LowerCtx<'_>) -> bool {
    ctx.binop.dict_entry.take().is_some_and(|r| r.taken)
}
