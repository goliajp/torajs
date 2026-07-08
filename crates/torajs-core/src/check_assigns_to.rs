//! `stmt_assigns_to` + `expr_assigns_to` pulled out of
//! [`crate::check`] as chunk-315 of the check.rs god-file decomp.
//!
//! Pure read-only AST walkers — given an `Ast`, a `Stmt` (or
//! `ExprId`), and a target binding name, return `true` iff any
//! reachable sub-expression performs `<name> = ...` (direct
//! assign) or `<name>++` / `<name>--` (post-increment with Ident
//! target). Used by `check_stmt_while::check` to gate the
//! "infinite while-true loop with no breaks and no assigns to
//! the loop variable" diagnostic.
//!
//! The two functions are mutually recursive (Stmt walker recurses
//! into nested Expr / Stmt children; Expr walker recurses back
//! into ArrowFn bodies via the Stmt walker). They are extracted
//! together so the recursion edges stay file-local.
//!
//! Re-exported from `check` as
//! `pub use check_assigns_to::stmt_assigns_to;` so the external
//! `check_stmt_while` caller continues to use the canonical
//! `crate::check::stmt_assigns_to` import path.

use crate::ast::{Ast, Expr, ExprId, Stmt};

pub(crate) fn stmt_assigns_to(ast: &Ast, s: &Stmt, name: &str) -> bool {
    match s {
        Stmt::Expr(eid) | Stmt::Throw(eid) | Stmt::Yield(eid) => expr_assigns_to(ast, *eid, name),
        Stmt::YieldInto { value, .. } => expr_assigns_to(ast, *value, name),
        Stmt::Return(maybe) => maybe.is_some_and(|e| expr_assigns_to(ast, e, name)),
        Stmt::LetDecl { init, .. } => expr_assigns_to(ast, *init, name),
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_assigns_to(ast, *cond, name)
                || stmt_assigns_to(ast, then_branch, name)
                || else_branch
                    .as_ref()
                    .is_some_and(|eb| stmt_assigns_to(ast, eb, name))
        }
        Stmt::While { cond, body } | Stmt::DoWhile { body, cond } => {
            expr_assigns_to(ast, *cond, name) || stmt_assigns_to(ast, body, name)
        }
        Stmt::Switch {
            scrutinee,
            cases,
            default,
        } => {
            if expr_assigns_to(ast, *scrutinee, name) {
                return true;
            }
            for c in cases {
                if expr_assigns_to(ast, c.value, name) {
                    return true;
                }
                for s in &c.body {
                    if stmt_assigns_to(ast, s, name) {
                        return true;
                    }
                }
            }
            if let Some(db) = default {
                for s in db {
                    if stmt_assigns_to(ast, s, name) {
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
            init.as_ref().is_some_and(|i| stmt_assigns_to(ast, i, name))
                || cond.is_some_and(|c| expr_assigns_to(ast, c, name))
                || step.is_some_and(|s| expr_assigns_to(ast, s, name))
                || stmt_assigns_to(ast, body, name)
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            stmts.iter().any(|s| stmt_assigns_to(ast, s, name))
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            body.iter().any(|s| stmt_assigns_to(ast, s, name))
                || catch_body.iter().any(|s| stmt_assigns_to(ast, s, name))
                || finally_body
                    .as_ref()
                    .is_some_and(|fb| fb.iter().any(|s| stmt_assigns_to(ast, s, name)))
        }
        Stmt::Break | Stmt::Continue => false,
        Stmt::ForOfSplitIter {
            parent, sep, body, ..
        } => {
            expr_assigns_to(ast, *parent, name)
                || expr_assigns_to(ast, *sep, name)
                || stmt_assigns_to(ast, body, name)
        }
        Stmt::ForOf {
            elem_expr, body, ..
        } => expr_assigns_to(ast, *elem_expr, name) || stmt_assigns_to(ast, body, name),
        Stmt::FnDecl { .. } | Stmt::TypeDecl { .. } | Stmt::ClassDecl { .. } => false,
        Stmt::ImportDecl { .. } => false,
        Stmt::ExportDecl { inner, .. } => inner
            .as_ref()
            .is_some_and(|inner| stmt_assigns_to(ast, inner, name)),
    }
}

fn expr_assigns_to(ast: &Ast, eid: ExprId, name: &str) -> bool {
    if let Expr::Assign { target, value } = ast.get_expr(eid) {
        if let Expr::Ident(n) = ast.get_expr(*target)
            && n == name
        {
            return true;
        }
        return expr_assigns_to(ast, *target, name) || expr_assigns_to(ast, *value, name);
    }
    match ast.get_expr(eid) {
        Expr::Call { callee, args } => {
            expr_assigns_to(ast, *callee, name)
                || args.iter().any(|a| expr_assigns_to(ast, *a, name))
        }
        Expr::BinOp { left, right, .. } => {
            expr_assigns_to(ast, *left, name) || expr_assigns_to(ast, *right, name)
        }
        Expr::Unary { expr, .. } => expr_assigns_to(ast, *expr, name),
        Expr::Member { obj, .. } => expr_assigns_to(ast, *obj, name),
        Expr::Index { obj, index } => {
            expr_assigns_to(ast, *obj, name) || expr_assigns_to(ast, *index, name)
        }
        Expr::Array(elems) => elems.iter().any(|e| expr_assigns_to(ast, *e, name)),
        Expr::ObjectLit { fields } => fields.iter().any(|(_, e)| expr_assigns_to(ast, *e, name)),
        Expr::ArrowFn { body, .. } => body.iter().any(|s| stmt_assigns_to(ast, s, name)),
        Expr::Closure { .. } => false,
        Expr::New { args, .. } | Expr::Super { args } => {
            args.iter().any(|a| expr_assigns_to(ast, *a, name))
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_assigns_to(ast, *cond, name)
                || expr_assigns_to(ast, *then_branch, name)
                || expr_assigns_to(ast, *else_branch, name)
        }
        Expr::TypeOf { expr }
        | Expr::Spread { expr }
        | Expr::InstanceOf { expr, .. }
        | Expr::As { expr, .. } => expr_assigns_to(ast, *expr, name),
        Expr::Sequence { left, right }
        | Expr::Nullish {
            lhs: left,
            rhs: right,
        } => expr_assigns_to(ast, *left, name) || expr_assigns_to(ast, *right, name),
        Expr::OptChain { obj, .. } => expr_assigns_to(ast, *obj, name),
        Expr::OptIndex { obj, index } => {
            expr_assigns_to(ast, *obj, name) || expr_assigns_to(ast, *index, name)
        }
        Expr::OptCall { callee, args } => {
            expr_assigns_to(ast, *callee, name)
                || args.iter().any(|a| expr_assigns_to(ast, *a, name))
        }
        Expr::PostIncr { target, .. } => {
            if let Expr::Ident(n) = ast.get_expr(*target) {
                if n == name {
                    return true;
                }
            }
            expr_assigns_to(ast, *target, name)
        }
        _ => false,
    }
}
