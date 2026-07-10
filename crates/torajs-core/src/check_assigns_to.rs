//! `stmt_assigns_to` + the member variant, pulled out of
//! [`crate::check`] as chunk-315 of the check.rs god-file decomp.
//!
//! Pure read-only AST walkers — given an `Ast`, a `Stmt` (or
//! `ExprId`), and an assign-target predicate, return `true` iff any
//! reachable sub-expression performs `<target> = ...` (direct
//! assign) or `<target>++` / `<target>--` (post-increment). The
//! walker is generic over the target shape (chunk 782):
//!
//! - [`stmt_assigns_to`] — Ident target `name = ...`; gates the
//!   `check_stmt_while` binding narrow and the "infinite while-true
//!   loop with no assigns to the loop variable" diagnostic.
//! - [`stmt_assigns_to_member`] — `recv = ...` OR `recv.field =
//!   ...`; gates the while-body member-path narrow (a re-bound
//!   receiver or a re-assigned field can turn the guarded member
//!   back to undefined on the next iteration, which the
//!   check-time invalidation hook in check_type_of_assign cannot
//!   see — the body is only walked once).
//!
//! The two walkers are mutually recursive (Stmt walker recurses
//! into nested Expr / Stmt children; Expr walker recurses back
//! into ArrowFn bodies via the Stmt walker). They are extracted
//! together so the recursion edges stay file-local.
//!
//! Re-exported from `check` as
//! `pub use check_assigns_to::stmt_assigns_to;` so the external
//! `check_stmt_while` caller continues to use the canonical
//! `crate::check::stmt_assigns_to` import path.

use crate::ast::{Ast, Expr, ExprId, Stmt};

/// Assign-target predicate: answers whether the `Expr` at the given
/// id is the target shape the caller is scanning for.
type HitFn<'a> = &'a dyn Fn(ExprId) -> bool;

pub(crate) fn stmt_assigns_to(ast: &Ast, s: &Stmt, name: &str) -> bool {
    stmt_assigns_matching(
        ast,
        s,
        &|target| matches!(ast.get_expr(target), Expr::Ident(n) if n == name),
    )
}

/// `recv = ...` or `recv.field = ...` anywhere in `s` — the
/// member-narrow while gate (see module doc).
pub(crate) fn stmt_assigns_to_member(ast: &Ast, s: &Stmt, recv: &str, field: &str) -> bool {
    stmt_assigns_matching(ast, s, &|target| match ast.get_expr(target) {
        Expr::Ident(n) => n == recv,
        Expr::Member { obj, name } => {
            name == field && matches!(ast.get_expr(*obj), Expr::Ident(n) if n == recv)
        }
        _ => false,
    })
}

fn stmt_assigns_matching(ast: &Ast, s: &Stmt, hit: HitFn) -> bool {
    match s {
        Stmt::Expr(eid) | Stmt::Throw(eid) | Stmt::Yield(eid) => {
            expr_assigns_matching(ast, *eid, hit)
        }
        Stmt::YieldInto { value, .. } => expr_assigns_matching(ast, *value, hit),
        Stmt::Return(maybe) => maybe.is_some_and(|e| expr_assigns_matching(ast, e, hit)),
        Stmt::LetDecl { init, .. } => expr_assigns_matching(ast, *init, hit),
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_assigns_matching(ast, *cond, hit)
                || stmt_assigns_matching(ast, then_branch, hit)
                || else_branch
                    .as_ref()
                    .is_some_and(|eb| stmt_assigns_matching(ast, eb, hit))
        }
        Stmt::While { cond, body } | Stmt::DoWhile { body, cond } => {
            expr_assigns_matching(ast, *cond, hit) || stmt_assigns_matching(ast, body, hit)
        }
        Stmt::Switch {
            scrutinee,
            cases,
            default,
        } => {
            if expr_assigns_matching(ast, *scrutinee, hit) {
                return true;
            }
            for c in cases {
                if expr_assigns_matching(ast, c.value, hit) {
                    return true;
                }
                for s in &c.body {
                    if stmt_assigns_matching(ast, s, hit) {
                        return true;
                    }
                }
            }
            if let Some(db) = default {
                for s in db {
                    if stmt_assigns_matching(ast, s, hit) {
                        return true;
                    }
                }
            }
            false
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            init.as_ref()
                .is_some_and(|i| stmt_assigns_matching(ast, i, hit))
                || cond.is_some_and(|c| expr_assigns_matching(ast, c, hit))
                || step.is_some_and(|s| expr_assigns_matching(ast, s, hit))
                || stmt_assigns_matching(ast, body, hit)
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            stmts.iter().any(|s| stmt_assigns_matching(ast, s, hit))
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            body.iter().any(|s| stmt_assigns_matching(ast, s, hit))
                || catch_body
                    .iter()
                    .any(|s| stmt_assigns_matching(ast, s, hit))
                || finally_body
                    .as_ref()
                    .is_some_and(|fb| fb.iter().any(|s| stmt_assigns_matching(ast, s, hit)))
        }
        Stmt::Break | Stmt::Continue => false,
        Stmt::ForOfSplitIter {
            parent, sep, body, ..
        } => {
            expr_assigns_matching(ast, *parent, hit)
                || expr_assigns_matching(ast, *sep, hit)
                || stmt_assigns_matching(ast, body, hit)
        }
        Stmt::ForOf {
            elem_expr, body, ..
        } => expr_assigns_matching(ast, *elem_expr, hit) || stmt_assigns_matching(ast, body, hit),
        Stmt::FnDecl { .. } | Stmt::TypeDecl { .. } | Stmt::ClassDecl { .. } => false,
        Stmt::ImportDecl { .. } => false,
        Stmt::ExportDecl { inner, .. } => inner
            .as_ref()
            .is_some_and(|inner| stmt_assigns_matching(ast, inner, hit)),
    }
}

fn expr_assigns_matching(ast: &Ast, eid: ExprId, hit: HitFn) -> bool {
    if let Expr::Assign { target, value } = ast.get_expr(eid) {
        if hit(*target) {
            return true;
        }
        return expr_assigns_matching(ast, *target, hit) || expr_assigns_matching(ast, *value, hit);
    }
    match ast.get_expr(eid) {
        Expr::Call { callee, args } => {
            expr_assigns_matching(ast, *callee, hit)
                || args.iter().any(|a| expr_assigns_matching(ast, *a, hit))
        }
        Expr::BinOp { left, right, .. } => {
            expr_assigns_matching(ast, *left, hit) || expr_assigns_matching(ast, *right, hit)
        }
        Expr::Unary { expr, .. } => expr_assigns_matching(ast, *expr, hit),
        Expr::Member { obj, .. } => expr_assigns_matching(ast, *obj, hit),
        Expr::Index { obj, index } => {
            expr_assigns_matching(ast, *obj, hit) || expr_assigns_matching(ast, *index, hit)
        }
        Expr::Array(elems) => elems.iter().any(|e| expr_assigns_matching(ast, *e, hit)),
        Expr::ObjectLit { fields } => fields
            .iter()
            .any(|(_, e)| expr_assigns_matching(ast, *e, hit)),
        Expr::ArrowFn { body, .. } => body.iter().any(|s| stmt_assigns_matching(ast, s, hit)),
        Expr::Closure { .. } => false,
        Expr::New { args, .. } | Expr::Super { args } => {
            args.iter().any(|a| expr_assigns_matching(ast, *a, hit))
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_assigns_matching(ast, *cond, hit)
                || expr_assigns_matching(ast, *then_branch, hit)
                || expr_assigns_matching(ast, *else_branch, hit)
        }
        Expr::TypeOf { expr }
        | Expr::Spread { expr }
        | Expr::InstanceOf { expr, .. }
        | Expr::As { expr, .. } => expr_assigns_matching(ast, *expr, hit),
        Expr::Sequence { left, right }
        | Expr::Nullish {
            lhs: left,
            rhs: right,
        } => expr_assigns_matching(ast, *left, hit) || expr_assigns_matching(ast, *right, hit),
        Expr::OptChain { obj, .. } => expr_assigns_matching(ast, *obj, hit),
        Expr::OptIndex { obj, index } => {
            expr_assigns_matching(ast, *obj, hit) || expr_assigns_matching(ast, *index, hit)
        }
        Expr::OptCall { callee, args } => {
            expr_assigns_matching(ast, *callee, hit)
                || args.iter().any(|a| expr_assigns_matching(ast, *a, hit))
        }
        Expr::PostIncr { target, .. } => {
            if hit(*target) {
                return true;
            }
            expr_assigns_matching(ast, *target, hit)
        }
        _ => false,
    }
}
