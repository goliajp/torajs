//! Where the pre-reserved fill loop's [[Extensible]] refusal may sit —
//! once in the preheader, or on every append — decided by whether a
//! push argument can run user code. The sibling of
//! [`crate::ssa_lower_push_loop_detect`], which decides whether the
//! loop takes the inline-store lane at all; the two ask different
//! questions of the same arguments (see [`push_args_cannot_call`]).

use crate::ast::{Expr, ExprId, Stmt};
use crate::ssa::Type;
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_push_loop_detect::{expr_is_inert, is_inert_binop};

/// The wider question the [[Extensible]] gate asks of the same
/// arguments: not "can it look at the array" but "can it run user
/// code" — a call, or an operator over an `any` operand (`valueOf`).
/// An argument that cannot has nothing to observe about the order
/// between its own evaluation and the refusal, so the gate may sit
/// once in the preheader; one that can takes the gate on every append
/// (`emit_prereserve_entry_gate` / `emit_prereserve_append_gate`).
/// Strictly wider than [`crate::ssa_lower_push_loop_detect::push_args_all_inert`]: a local of any type,
/// a string literal, `a + b` over non-`any` operands and a typed
/// array element (`dst.push(src[i])`) all qualify. `defer_len` was
/// the first key tried and cost array-copy-1m +15% for a per-append
/// check `src[i]` never needed.
pub(crate) fn push_args_cannot_call(ctx: &LowerCtx<'_>, s: &Stmt) -> bool {
    match s {
        Stmt::Expr(eid) => match ctx.ast.get_expr(*eid) {
            Expr::Call { args, .. } => args.iter().all(|a| expr_cannot_call(ctx, *a)),
            Expr::Assign { target, value } => {
                matches!(ctx.ast.get_expr(*target), Expr::Ident(_)) && expr_cannot_call(ctx, *value)
            }
            Expr::PostIncr { target, .. } => {
                matches!(ctx.ast.get_expr(*target), Expr::Ident(_))
            }
            _ => false,
        },
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            stmts.iter().all(|s| push_args_cannot_call(ctx, s))
        }
        _ => false,
    }
}

/// See [`push_args_cannot_call`]. Operand types are read off the
/// checker: `+` over an `any` is a `valueOf` call.
fn expr_cannot_call(ctx: &LowerCtx<'_>, eid: ExprId) -> bool {
    if expr_is_inert(ctx, eid) {
        return true;
    }
    let primitive = |e: ExprId| {
        matches!(
            ctx.expr_types.get(&e),
            Some(crate::check::Type::Number)
                | Some(crate::check::Type::Boolean)
                | Some(crate::check::Type::String)
        )
    };
    match ctx.ast.get_expr(eid) {
        Expr::Number(_) | Expr::Bool(_) | Expr::String(_) | Expr::Ident(_) => true,
        Expr::BinOp { op, left, right } => {
            is_inert_binop(*op)
                && primitive(*left)
                && primitive(*right)
                && expr_cannot_call(ctx, *left)
                && expr_cannot_call(ctx, *right)
        }
        Expr::Index { obj, index } => {
            let Expr::Ident(name) = ctx.ast.get_expr(*obj) else {
                return false;
            };
            let Some(info) = ctx.locals.get(name) else {
                return false;
            };
            let Type::Arr(id) = info.ty else {
                return false;
            };
            ctx.arr_layouts[id.0 as usize] != Type::Any && expr_cannot_call(ctx, *index)
        }
        _ => false,
    }
}
