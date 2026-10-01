//! Does a node write an owned value INTO the binding `x`, or hand
//! `x` on as a bare value — the predicate [`super::let_owned_elems`]
//! asks of every statement in a binding's window. The rule and its
//! bias (a shape the walk cannot prove safe counts as a write) are
//! documented there.

use super::view_homes::stores_owned;
use super::{Ast, BinOp, Expr, ExprId, Stmt};

/// `x` itself, through the `as` wrappers a use site can carry and
/// the forms that answer one of their operands unchanged.
fn is_bare(ast: &Ast, eid: ExprId, x: &str) -> bool {
    match ast.get_expr(eid) {
        Expr::Ident(n) => n == x,
        Expr::As { expr, .. } => is_bare(ast, *expr, x),
        Expr::Ternary {
            then_branch,
            else_branch,
            ..
        } => is_bare(ast, *then_branch, x) || is_bare(ast, *else_branch, x),
        Expr::Nullish { lhs, rhs } => is_bare(ast, *lhs, x) || is_bare(ast, *rhs, x),
        Expr::BinOp { op, left, right } if matches!(op, BinOp::LAnd | BinOp::LOr) => {
            is_bare(ast, *left, x) || is_bare(ast, *right, x)
        }
        Expr::Sequence { right, .. } => is_bare(ast, *right, x),
        _ => false,
    }
}

// The walk below is the owned-elements twin of
// `ssa_lower_toplevel_globals::write_through::binding_written_through`
// (same variant catalogue, a different question): true when `x` is
// written INTO with an owned value, or escapes as a bare value,
// somewhere under the node.
fn expr_writes(ast: &Ast, eid: ExprId, x: &str) -> bool {
    let is_x = |e: ExprId| matches!(ast.get_expr(e), Expr::Ident(n) if n == x);
    match ast.get_expr(eid) {
        Expr::Call { callee, args } => {
            if let Expr::Member { obj, name } = ast.get_expr(*callee)
                && is_x(*obj)
                && stores_owned(name)
            {
                return true;
            }
            // the whole array handed to a callee: an escape
            if args.iter().any(|a| is_bare(ast, *a, x)) {
                return true;
            }
            expr_writes(ast, *callee, x) || args.iter().any(|a| expr_writes(ast, *a, x))
        }
        Expr::Assign { target, value } => {
            let target_is_through_x =
                matches!(ast.get_expr(*target), Expr::Index { obj, .. } if is_x(*obj));
            // the binding itself reassigned (`x = [...]` / `x = other`):
            // its slot has to take whatever the program stores later,
            // and an `Arr<Substr>` slot takes only a fresh split
            // product — so it is typed `Arr<Str>` from the start and
            // every value entering it is owned (plan-state 468-02)
            let rebound = is_x(*target);
            // the whole array stored anywhere: an escape
            target_is_through_x
                || rebound
                || is_bare(ast, *value, x)
                || expr_writes(ast, *target, x)
                || expr_writes(ast, *value, x)
        }
        Expr::PostIncr { target, .. } => {
            matches!(ast.get_expr(*target), Expr::Index { obj, .. } if is_x(*obj))
                || expr_writes(ast, *target, x)
        }
        // Anything that can hide a write we have not modelled
        // counts: a lifted closure that captured the binding, or
        // an arrow whose body is not walked here.
        Expr::Closure { captures, .. } => captures.iter().any(|c| c == x),
        Expr::ArrowFn { .. } => true,
        // Pure recursion over every other variant — the same
        // catalogue `ast::escape_analyze::eal_expr_safe` walks.
        Expr::Member { obj, .. } => expr_writes(ast, *obj, x),
        Expr::Index { obj, index } => expr_writes(ast, *obj, x) || expr_writes(ast, *index, x),
        Expr::BinOp { left, right, .. } | Expr::Sequence { left, right } => {
            expr_writes(ast, *left, x) || expr_writes(ast, *right, x)
        }
        Expr::Nullish { lhs, rhs } => expr_writes(ast, *lhs, x) || expr_writes(ast, *rhs, x),
        Expr::Unary { expr, .. }
        | Expr::TypeOf { expr }
        | Expr::Delete { expr }
        | Expr::As { expr, .. }
        | Expr::Spread { expr } => expr_writes(ast, *expr, x),
        Expr::InstanceOf { expr, rhs } => expr_writes(ast, *expr, x) || expr_writes(ast, *rhs, x),
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_writes(ast, *cond, x)
                || expr_writes(ast, *then_branch, x)
                || expr_writes(ast, *else_branch, x)
        }
        Expr::Array(els) => els
            .iter()
            .any(|e| is_bare(ast, *e, x) || expr_writes(ast, *e, x)),
        Expr::ObjectLit { fields } => fields
            .iter()
            .any(|(_, e)| is_bare(ast, *e, x) || expr_writes(ast, *e, x)),
        Expr::OptChain { obj, .. } => expr_writes(ast, *obj, x),
        Expr::OptIndex { obj, index } => expr_writes(ast, *obj, x) || expr_writes(ast, *index, x),
        Expr::OptCall { callee, args } => {
            expr_writes(ast, *callee, x) || args.iter().any(|a| expr_writes(ast, *a, x))
        }
        Expr::New { args, .. } | Expr::Super { args } => args
            .iter()
            .any(|a| is_bare(ast, *a, x) || expr_writes(ast, *a, x)),
        Expr::NewDynamic { callee, args } => {
            expr_writes(ast, *callee, x)
                || args
                    .iter()
                    .any(|a| is_bare(ast, *a, x) || expr_writes(ast, *a, x))
        }
        Expr::Ident(_)
        | Expr::Elision
        | Expr::This
        | Expr::NewTarget
        | Expr::Number(_)
        | Expr::BigInt { .. }
        | Expr::String(_)
        | Expr::Bool(_)
        | Expr::Null
        | Expr::Uninit
        | Expr::Regex { .. } => false,
    }
}
pub(super) fn stmt_writes(ast: &Ast, s: &Stmt, x: &str) -> bool {
    match s {
        Stmt::Expr(e) | Stmt::Throw(e) => expr_writes(ast, *e, x),
        // the whole array yielded, returned, or bound again: an escape
        // — the receiver is typed by its own annotation
        Stmt::Yield(e) | Stmt::YieldInto { value: e, .. } => {
            is_bare(ast, *e, x) || expr_writes(ast, *e, x)
        }
        Stmt::Return(e) => e.is_some_and(|e| is_bare(ast, e, x) || expr_writes(ast, e, x)),
        Stmt::Break(_) | Stmt::Continue(_) => false,
        Stmt::LetDecl { init, .. } | Stmt::UsingDecl { init, .. } => {
            is_bare(ast, *init, x) || expr_writes(ast, *init, x)
        }
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_writes(ast, *cond, x)
                || stmt_writes(ast, then_branch, x)
                || else_branch
                    .as_deref()
                    .is_some_and(|e| stmt_writes(ast, e, x))
        }
        Stmt::While { cond, body } | Stmt::DoWhile { body, cond } => {
            expr_writes(ast, *cond, x) || stmt_writes(ast, body, x)
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            init.as_deref().is_some_and(|i| stmt_writes(ast, i, x))
                || cond.is_some_and(|c| expr_writes(ast, c, x))
                || step.is_some_and(|st| expr_writes(ast, st, x))
                || stmt_writes(ast, body, x)
        }
        Stmt::ForOfSplitIter {
            parent, sep, body, ..
        } => expr_writes(ast, *parent, x) || expr_writes(ast, *sep, x) || stmt_writes(ast, body, x),
        Stmt::ForOf {
            elem_expr, body, ..
        } => expr_writes(ast, *elem_expr, x) || stmt_writes(ast, body, x),
        Stmt::Switch {
            scrutinee,
            cases,
            default,
            ..
        } => {
            expr_writes(ast, *scrutinee, x)
                || cases.iter().any(|c| {
                    expr_writes(ast, c.value, x) || c.body.iter().any(|s| stmt_writes(ast, s, x))
                })
                || default
                    .as_ref()
                    .is_some_and(|db| db.iter().any(|s| stmt_writes(ast, s, x)))
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            body.iter().any(|s| stmt_writes(ast, s, x))
                || catch_body.iter().any(|s| stmt_writes(ast, s, x))
                || finally_body
                    .as_ref()
                    .is_some_and(|fb| fb.iter().any(|s| stmt_writes(ast, s, x)))
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => stmts.iter().any(|s| stmt_writes(ast, s, x)),
        Stmt::Labeled { body, .. } => stmt_writes(ast, body, x),
        // A named fn body CAN reach a promoted top-level binding
        // (that is why it was promoted); walk it. Class methods
        // likewise.
        Stmt::FnDecl { body, .. } => body.iter().any(|s| stmt_writes(ast, s, x)),
        Stmt::ClassDecl { methods, .. } => methods
            .iter()
            .any(|m| m.body.iter().any(|s| stmt_writes(ast, s, x))),
        Stmt::ExportDecl { inner, .. } => inner.as_deref().is_some_and(|s| stmt_writes(ast, s, x)),
        Stmt::TypeDecl { .. } | Stmt::ImportDecl { .. } => false,
    }
}
