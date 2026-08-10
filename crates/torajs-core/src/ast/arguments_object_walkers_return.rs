//! The unsafe-return scan of
//! [`super::arguments_object_walkers`] — split to a sibling when the
//! S2 callee scans (RFC 20260810-sloppy-goal-arguments) pushed the
//! walkers past the 500-line file limit. Verbatim move; the parent
//! re-exports it so `arguments_object_collect`'s import path is
//! unchanged.

use super::{Ast, Expr, ExprId, Stmt};

/// RFC 20260708-closure-argv-face — true if any `return` in the
/// body could hand back an `arguments[i]` elem box through a
/// pass-through chain (ternary arm / nullish side / sequence tail /
/// as / assign value) WITHOUT a consuming node in between: the
/// return-root retain can't see through those, so the box would
/// leave borrowing the materialized array's stake (UAF once the
/// array scope-drops). Such bodies stay KeepLoud. A root
/// `arguments[i]` return is fine (the return lowering retains) and
/// any read under a consuming node (BinOp / call arg / literal /
/// member / index position) produces a fresh result.
pub(super) fn body_has_unsafe_return_arguments(ast: &Ast, body: &[Stmt]) -> bool {
    fn stmt_walk(ast: &Ast, s: &Stmt) -> bool {
        match s {
            Stmt::Return(Some(e)) => {
                // root arguments-index — retained by the return
                // lowering, safe.
                if matches!(ast.get_expr(*e), Expr::Index { obj, .. }
                    if matches!(ast.get_expr(*obj), Expr::Ident(n) if n == "arguments"))
                {
                    return false;
                }
                passthrough_aliases(ast, *e)
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                stmt_walk(ast, then_branch)
                    || else_branch.as_ref().is_some_and(|e| stmt_walk(ast, e))
            }
            Stmt::While { body, .. }
            | Stmt::DoWhile { body, .. }
            | Stmt::For { body, .. }
            | Stmt::Labeled { body, .. } => stmt_walk(ast, body),
            Stmt::Block(stmts) | Stmt::Multi(stmts) => stmts.iter().any(|s| stmt_walk(ast, s)),
            Stmt::Try {
                body, catch_body, ..
            } => {
                body.iter().any(|s| stmt_walk(ast, s))
                    || catch_body.iter().any(|s| stmt_walk(ast, s))
            }
            Stmt::Switch { cases, default, .. } => {
                cases
                    .iter()
                    .any(|c| c.body.iter().any(|s| stmt_walk(ast, s)))
                    || default
                        .as_ref()
                        .is_some_and(|d| d.iter().any(|s| stmt_walk(ast, s)))
            }
            _ => false,
        }
    }
    fn passthrough_aliases(ast: &Ast, e: ExprId) -> bool {
        match ast.get_expr(e) {
            Expr::Index { obj, .. } if matches!(ast.get_expr(*obj), Expr::Ident(n) if n == "arguments") => {
                true
            }
            Expr::Ternary {
                then_branch,
                else_branch,
                ..
            } => passthrough_aliases(ast, *then_branch) || passthrough_aliases(ast, *else_branch),
            Expr::Nullish { lhs, rhs } => {
                passthrough_aliases(ast, *lhs) || passthrough_aliases(ast, *rhs)
            }
            Expr::Sequence { right, .. } => passthrough_aliases(ast, *right),
            Expr::As { expr, .. } => passthrough_aliases(ast, *expr),
            Expr::Assign { value, .. } => passthrough_aliases(ast, *value),
            _ => false,
        }
    }
    body.iter().any(|s| stmt_walk(ast, s))
}
