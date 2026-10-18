//! The inertness allowlist the fill-loop lanes ask of push arguments
//! and of the loop bound: expressions that cannot call, read a heap
//! cell, or throw.

use super::{array_is_unfilled, names_all_owned};
use crate::ast::{Expr, ExprId, Stmt, UnaryOp};
use crate::ssa_lower::LowerCtx;

/// True when every `xs.push(arg)` in `s` has an argument that cannot
/// look at the array being filled, and cannot leave the loop by any
/// edge but its normal exit.
///
/// This is what buys the deferred length word (see
/// [`crate::ssa_lower::PreReserveState::defer_len`]). The body shape
/// the detectors above accept says the loop contains nothing but
/// pushes; it says nothing about what is being pushed, and an
/// argument is an arbitrary expression. `xs.push(xs.length)` reads
/// the very word the loop is holding back, and `xs.push(f(i))` hands
/// control to a function that may read the array or throw out of the
/// loop entirely.
///
/// An allowlist, because the safe direction is to answer no: a shape
/// this does not recognise gets the length word written on every
/// append instead, which costs a store rather than an answer.
/// Literals, numeric locals, and arithmetic over them are the whole
/// list — none of those can call, read a heap cell, or throw. A
/// member read is out: `xs.length` is the case that started this. An
/// element read `a[k]` answering `number` / `boolean` is in when `a`
/// cannot be one of `names` (the arrays being filled) under the same
/// test the bound's `a.length` passes ([`array_is_unfilled`]): it
/// reads a cell whose length word is live, a typed element read
/// cannot call, and its out-of-range answer is `undefined`, not a
/// throw. That is the copy loop `dst.push(src[i])`. The operand types
/// are checked, not just the operator: `a + b` over `any` runs
/// `valueOf`, which is user code with the array in scope.
pub(crate) fn push_args_all_inert(ctx: &LowerCtx<'_>, s: &Stmt, names: &[String]) -> bool {
    match s {
        Stmt::Expr(eid) => match ctx.ast.get_expr(*eid) {
            Expr::Call { args, .. } => args.iter().all(|a| push_arg_is_inert(ctx, *a, names)),
            // The `while` lane's body carries its counter step as the
            // last statement. Bumping a local cannot look at the array
            // either, in either spelling.
            Expr::Assign { target, value } => {
                matches!(ctx.ast.get_expr(*target), Expr::Ident(_)) && expr_is_inert(ctx, *value)
            }
            Expr::PostIncr { target, .. } => {
                matches!(ctx.ast.get_expr(*target), Expr::Ident(_))
            }
            _ => false,
        },
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            stmts.iter().all(|s| push_args_all_inert(ctx, s, names))
        }
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_is_inert(ctx, *cond)
                && push_args_all_inert(ctx, then_branch, names)
                && else_branch
                    .as_ref()
                    .is_none_or(|b| push_args_all_inert(ctx, b, names))
        }
        _ => false,
    }
}

/// Every `if` condition in a fill-loop body is inert. A condition is
/// code no push statement put there, and the reserve's facts — the
/// cell neither moves, grows nor shrinks under the loop, and the bound
/// read above it stays the bound — hold only while nothing in the body
/// can call.
pub(crate) fn branch_conds_inert(ctx: &LowerCtx<'_>, s: &Stmt) -> bool {
    match s {
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_is_inert(ctx, *cond)
                && branch_conds_inert(ctx, then_branch)
                && else_branch
                    .as_ref()
                    .is_none_or(|b| branch_conds_inert(ctx, b))
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => stmts.iter().all(|s| branch_conds_inert(ctx, s)),
        _ => true,
    }
}

/// One push argument. See [`push_args_all_inert`].
fn push_arg_is_inert(ctx: &LowerCtx<'_>, eid: ExprId, names: &[String]) -> bool {
    // an instance built in place reads nothing but its arguments
    if let Some(args) = crate::ssa_lower::inline_push::construction_args(ctx, eid) {
        return args.iter().all(|a| push_arg_is_inert(ctx, *a, names));
    }
    let typed = matches!(
        ctx.expr_types.get(&eid),
        Some(crate::check::Type::Number) | Some(crate::check::Type::Boolean)
    );
    match ctx.ast.get_expr(eid) {
        Expr::Index { obj, index } if typed => {
            matches!(ctx.ast.get_expr(*obj), Expr::Ident(a)
                if array_is_unfilled(ctx, *obj, a, names, names_all_owned(ctx, names)))
                && expr_is_inert(ctx, *index)
        }
        Expr::BinOp { op, left, right } if typed && is_inert_binop(*op) => {
            push_arg_is_inert(ctx, *left, names) && push_arg_is_inert(ctx, *right, names)
        }
        _ => expr_is_inert(ctx, eid),
    }
}

/// The allowlist itself. See [`push_args_all_inert`].
pub(crate) fn expr_is_inert(ctx: &LowerCtx<'_>, eid: ExprId) -> bool {
    if !matches!(
        ctx.expr_types.get(&eid),
        Some(crate::check::Type::Number) | Some(crate::check::Type::Boolean)
    ) {
        return false;
    }
    match ctx.ast.get_expr(eid) {
        Expr::Number(_) | Expr::Bool(_) | Expr::Ident(_) => true,
        Expr::BinOp { op, left, right } => {
            is_inert_binop(*op) && expr_is_inert(ctx, *left) && expr_is_inert(ctx, *right)
        }
        Expr::Unary { op, expr } => !matches!(op, UnaryOp::Plus) && expr_is_inert(ctx, *expr),
        _ => false,
    }
}

/// Arithmetic, comparison and logic that cannot call, read a heap
/// cell, or throw once every operand is a primitive (each caller checks
/// that). Shared by the push-argument allowlist and the bound's.
pub(crate) fn is_inert_binop(op: crate::ast::BinOp) -> bool {
    use crate::ast::BinOp as B;
    matches!(
        op,
        B::Add
            | B::Sub
            | B::Mul
            | B::Div
            | B::Mod
            | B::Lt
            | B::Gt
            | B::Le
            | B::Ge
            | B::Eq
            | B::Neq
            | B::LooseEq
            | B::LooseNeq
            | B::LAnd
            | B::LOr
            | B::BitAnd
            | B::BitOr
            | B::BitXor
            | B::Shl
            | B::Shr
            | B::UShr
    )
}
