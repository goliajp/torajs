//! `block_can_throw` / `stmt_can_throw` / `expr_can_throw` — the
//! conservative "could this subtree raise?" walk behind the
//! unreachable-catch lint. Conservative in one direction only: a
//! `false` must mean no throw is possible, so every shape that cannot
//! be proved safe answers `true`.
//!
//! Extracted from `linter.rs` at the 500-line limit, alongside the
//! `refs` sibling.

use crate::ast::{Ast, Expr, ExprId, Stmt};

/// Conservative "can this block throw?" — returns true if it contains
/// any `throw`, any `Call` (since user-defined fns may throw), any
/// nested try, or any other shape we can't statically prove safe.
/// False means definitively no throw possible — the catch block is
/// then unreachable.
pub(super) fn block_can_throw(ast: &Ast, stmts: &[Stmt]) -> bool {
    for s in stmts {
        if stmt_can_throw(ast, s) {
            return true;
        }
    }
    false
}

pub(super) fn stmt_can_throw(ast: &Ast, s: &Stmt) -> bool {
    match s {
        Stmt::Throw(_) => true,
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            had_catch,
            ..
        } => {
            // A nested try with its own catch absorbs its body's
            // throws — but the catch / finally bodies themselves may
            // still throw upward.
            if !had_catch && block_can_throw(ast, body) {
                return true;
            }
            block_can_throw(ast, catch_body)
                || finally_body
                    .as_ref()
                    .map(|fb| block_can_throw(ast, fb))
                    .unwrap_or(false)
        }
        Stmt::Expr(eid) => expr_can_throw(ast, *eid),
        Stmt::Return(Some(eid)) => expr_can_throw(ast, *eid),
        Stmt::LetDecl { init, .. } | Stmt::UsingDecl { init, .. } => expr_can_throw(ast, *init),
        Stmt::Yield(eid) => expr_can_throw(ast, *eid),
        Stmt::YieldInto { value, .. } => expr_can_throw(ast, *value),
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_can_throw(ast, *cond)
                || stmt_can_throw(ast, then_branch)
                || else_branch
                    .as_ref()
                    .map(|e| stmt_can_throw(ast, e))
                    .unwrap_or(false)
        }
        Stmt::While { cond, body } | Stmt::DoWhile { cond, body } => {
            expr_can_throw(ast, *cond) || stmt_can_throw(ast, body)
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            init.as_ref()
                .map(|s| stmt_can_throw(ast, s))
                .unwrap_or(false)
                || cond.map(|c| expr_can_throw(ast, c)).unwrap_or(false)
                || step.map(|c| expr_can_throw(ast, c)).unwrap_or(false)
                || stmt_can_throw(ast, body)
        }
        Stmt::ForOfSplitIter {
            parent, sep, body, ..
        } => expr_can_throw(ast, *parent) || expr_can_throw(ast, *sep) || stmt_can_throw(ast, body),
        Stmt::ForOf {
            elem_expr, body, ..
        } => expr_can_throw(ast, *elem_expr) || stmt_can_throw(ast, body),
        Stmt::Switch {
            scrutinee,
            cases,
            default,
        } => {
            if expr_can_throw(ast, *scrutinee) {
                return true;
            }
            for c in cases {
                if expr_can_throw(ast, c.value) || block_can_throw(ast, &c.body) {
                    return true;
                }
            }
            default
                .as_ref()
                .map(|d| block_can_throw(ast, d))
                .unwrap_or(false)
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => block_can_throw(ast, stmts),
        Stmt::Labeled { body, .. } => stmt_can_throw(ast, body),
        Stmt::ExportDecl { inner, .. } => inner
            .as_ref()
            .map(|s| stmt_can_throw(ast, s))
            .unwrap_or(false),
        Stmt::Return(None)
        | Stmt::Break(_)
        | Stmt::Continue(_)
        | Stmt::FnDecl { .. }
        | Stmt::TypeDecl { .. }
        | Stmt::ClassDecl { .. }
        | Stmt::ImportDecl { .. } => false,
    }
}

pub(super) fn expr_can_throw(ast: &Ast, eid: ExprId) -> bool {
    match ast.get_expr(eid) {
        Expr::Elision => false,
        Expr::Call { .. }
        | Expr::OptCall { .. }
        | Expr::New { .. }
        | Expr::NewDynamic { .. }
        | Expr::Super { .. } => true,
        Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => expr_can_throw(ast, *obj),
        Expr::OptIndex { obj, index } => expr_can_throw(ast, *obj) || expr_can_throw(ast, *index),
        Expr::Index { obj, index } => expr_can_throw(ast, *obj) || expr_can_throw(ast, *index),
        Expr::BinOp { left, right, .. } => {
            expr_can_throw(ast, *left) || expr_can_throw(ast, *right)
        }
        Expr::Unary { expr, .. }
        | Expr::TypeOf { expr }
        | Expr::Delete { expr }
        | Expr::PostIncr { target: expr, .. }
        | Expr::As { expr, .. } => expr_can_throw(ast, *expr),
        Expr::Sequence { left, right } => expr_can_throw(ast, *left) || expr_can_throw(ast, *right),
        Expr::Assign { target, value } => {
            expr_can_throw(ast, *target) || expr_can_throw(ast, *value)
        }
        Expr::Array(items) => items.iter().any(|e| expr_can_throw(ast, *e)),
        Expr::ObjectLit { fields } => fields.iter().any(|(_, e)| expr_can_throw(ast, *e)),
        Expr::Spread { expr } => expr_can_throw(ast, *expr),
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_can_throw(ast, *cond)
                || expr_can_throw(ast, *then_branch)
                || expr_can_throw(ast, *else_branch)
        }
        Expr::Nullish { lhs, rhs } => expr_can_throw(ast, *lhs) || expr_can_throw(ast, *rhs),
        Expr::ArrowFn { .. } | Expr::Closure { .. } => false, // declaration only
        Expr::Ident(_)
        | Expr::String(_)
        | Expr::Number(_)
        | Expr::BigInt { .. }
        | Expr::Bool(_)
        | Expr::Null
        | Expr::Uninit
        | Expr::Regex { .. }
        | Expr::This
        | Expr::NewTarget => false,
        // ES §13.10.2 throws in its own right — step 2 on a target
        // that is not an object, step 4 on one that is not callable —
        // and a `@@hasInstance` handler is an arbitrary call. Listing
        // it with the leaves was right only while the operator was a
        // compile-time fold over a name.
        Expr::InstanceOf { .. } => true,
    }
}
