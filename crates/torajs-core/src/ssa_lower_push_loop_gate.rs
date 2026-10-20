//! Whether the pre-reserved fill loop may be installed over a body's
//! push arguments at all: none of them may run user code. The sibling
//! of [`crate::ssa_lower_push_loop_detect`], which recognizes the loop
//! shape; this answers the question that shape leaves open (see
//! [`push_args_cannot_call`]).

use crate::ast::{Expr, ExprId, Stmt};
use crate::ssa::Type;
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_push_loop_detect::{expr_is_inert, is_inert_binop};

/// Whether no push argument (and no branch condition) can run user
/// code — a call, an assignment, an increment, or an operator over an
/// `any` operand (`valueOf`). Code run there could write the loop bound
/// read once above the loop, push to the array behind the inline
/// stores' private running length, or freeze it between appends; an
/// argument that cannot run code has nothing to observe either, so the
/// [[Extensible]] refusal sits once in the preheader
/// (`emit_prereserve_entry_gate`). Strictly wider than
/// [`crate::ssa_lower_push_loop_detect::push_args_all_inert`]: a local
/// of any type, a string literal, `a + b` over non-`any` operands, a
/// typed array element (`dst.push(src[i])`) and a fresh array of such
/// elements all qualify.
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
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_cannot_call(ctx, *cond)
                && push_args_cannot_call(ctx, then_branch)
                && else_branch
                    .as_ref()
                    .is_none_or(|b| push_args_cannot_call(ctx, b))
        }
        _ => false,
    }
}

/// See [`push_args_cannot_call`]. Operand types are read off the
/// checker: `+` over an `any` is a `valueOf` call.
pub(crate) fn expr_cannot_call(ctx: &LowerCtx<'_>, eid: ExprId) -> bool {
    if expr_is_inert(ctx, eid) {
        return true;
    }
    // an instance built in place in an inline array's slot is stores
    if let Some(args) = crate::ssa_lower::inline_push::construction_args(ctx, eid) {
        return args.iter().all(|a| expr_cannot_call(ctx, *a));
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
        // a fresh array of such elements (`rows.push([])`); a spread
        // runs an iterator and is refused with the rest
        Expr::Array(elems) => elems.iter().all(|e| expr_cannot_call(ctx, *e)),
        _ => false,
    }
}

/// Appends per trip for each distinct array a fill loop fills, in
/// first-push order. The detector lists every push site, so a body
/// that pushes one array three times names it three times — and the
/// reserve used to run once per site on the same pre-loop length,
/// leaving room for one append a trip while the unchecked stores made
/// three: `for (…) { xs.push(a); xs.push(b) }` wrote a second bound's
/// worth of slots into whatever cells sat past the buffer.
pub(crate) fn push_counts(names: &[String]) -> Vec<(String, i64)> {
    let mut out: Vec<(String, i64)> = Vec::new();
    for n in names {
        match out.iter_mut().find(|(m, _)| m == n) {
            Some((_, k)) => *k += 1,
            None => out.push((n.clone(), 1)),
        }
    }
    out
}
