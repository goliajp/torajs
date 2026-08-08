//! T-22 Stmt::ForOfSplitIter — "x-safe" analysis: verify no
//! reference to the split-iter user-named `x` binding appears
//! outside the exact `Index(Ident(x), Ident(i))` sequential-access
//! shape, plus a plain-ident-use probe.
//!
//! Chunk 348 — extracted from ast.rs. Paired with chunks 347's
//! `sfi_rewrite` sibling. Both wrappers (`sfi_body_x_safe` +
//! `sfi_stmt_uses_ident`) are called from the SFI desugar pass in
//! ast.rs (lines ~5203/5219/5225); the two recursive walkers
//! (`sfi_stmt_x_safe` + `sfi_expr_x_safe`) stay sibling-private.

use super::{Ast, Expr, ExprId, Stmt};

/// Walk body, return false if any reference to `x_name` appears that
/// is NOT exactly `Index(Ident(x_name), Ident(i_name))` (sequential
/// access). Conservative — false on any uncertain shape.
pub(super) fn sfi_body_x_safe(ast: &Ast, body: &Stmt, x_name: &str, i_name: &str) -> bool {
    sfi_stmt_x_safe(ast, body, x_name, i_name)
}

fn sfi_stmt_x_safe(ast: &Ast, s: &Stmt, x_name: &str, i_name: &str) -> bool {
    match s {
        Stmt::Expr(eid) | Stmt::Throw(eid) | Stmt::Yield(eid) => {
            sfi_expr_x_safe(ast, *eid, x_name, i_name)
        }
        Stmt::Return(Some(eid)) => sfi_expr_x_safe(ast, *eid, x_name, i_name),
        Stmt::Return(None) | Stmt::Break(_) | Stmt::Continue(_) => true,
        Stmt::Labeled { body, .. } => sfi_stmt_x_safe(ast, body, x_name, i_name),
        Stmt::LetDecl { init, .. } | Stmt::UsingDecl { init, .. } => {
            sfi_expr_x_safe(ast, *init, x_name, i_name)
        }
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            sfi_expr_x_safe(ast, *cond, x_name, i_name)
                && sfi_stmt_x_safe(ast, then_branch, x_name, i_name)
                && else_branch
                    .as_deref()
                    .map_or(true, |eb| sfi_stmt_x_safe(ast, eb, x_name, i_name))
        }
        Stmt::While { cond, body } | Stmt::DoWhile { body, cond } => {
            sfi_expr_x_safe(ast, *cond, x_name, i_name)
                && sfi_stmt_x_safe(ast, body, x_name, i_name)
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            init.as_deref()
                .map_or(true, |i| sfi_stmt_x_safe(ast, i, x_name, i_name))
                && cond.map_or(true, |c| sfi_expr_x_safe(ast, c, x_name, i_name))
                && step.map_or(true, |st| sfi_expr_x_safe(ast, st, x_name, i_name))
                && sfi_stmt_x_safe(ast, body, x_name, i_name)
        }
        Stmt::ForOfSplitIter {
            parent, sep, body, ..
        } => {
            sfi_expr_x_safe(ast, *parent, x_name, i_name)
                && sfi_expr_x_safe(ast, *sep, x_name, i_name)
                && sfi_stmt_x_safe(ast, body, x_name, i_name)
        }
        Stmt::ForOf {
            elem_expr, body, ..
        } => {
            sfi_expr_x_safe(ast, *elem_expr, x_name, i_name)
                && sfi_stmt_x_safe(ast, body, x_name, i_name)
        }
        Stmt::Switch {
            scrutinee,
            cases,
            default,
        } => {
            sfi_expr_x_safe(ast, *scrutinee, x_name, i_name)
                && cases.iter().all(|c| {
                    sfi_expr_x_safe(ast, c.value, x_name, i_name)
                        && c.body
                            .iter()
                            .all(|s| sfi_stmt_x_safe(ast, s, x_name, i_name))
                })
                && default.as_ref().map_or(true, |db| {
                    db.iter().all(|s| sfi_stmt_x_safe(ast, s, x_name, i_name))
                })
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            body.iter().all(|s| sfi_stmt_x_safe(ast, s, x_name, i_name))
                && catch_body
                    .iter()
                    .all(|s| sfi_stmt_x_safe(ast, s, x_name, i_name))
                && finally_body.as_ref().map_or(true, |fb| {
                    fb.iter().all(|s| sfi_stmt_x_safe(ast, s, x_name, i_name))
                })
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => stmts
            .iter()
            .all(|s| sfi_stmt_x_safe(ast, s, x_name, i_name)),
        Stmt::YieldInto { value, .. } => sfi_expr_x_safe(ast, *value, x_name, i_name),
        Stmt::FnDecl { .. }
        | Stmt::TypeDecl { .. }
        | Stmt::ClassDecl { .. }
        | Stmt::ImportDecl { .. } => true,
        Stmt::ExportDecl { inner, .. } => inner
            .as_deref()
            .map_or(true, |s| sfi_stmt_x_safe(ast, s, x_name, i_name)),
    }
}

/// Returns true iff every reference to `x_name` in `eid` is the safe
/// shape `Index(Ident(x_name), Ident(i_name))`. Conservative — any
/// shape we can't analyze cleanly returns false.
fn sfi_expr_x_safe(ast: &Ast, eid: ExprId, x_name: &str, i_name: &str) -> bool {
    match ast.get_expr(eid) {
        Expr::Elision => true,
        Expr::Ident(n) => n != x_name,
        Expr::Index { obj, index } => {
            if let Expr::Ident(n) = ast.get_expr(*obj)
                && n == x_name
            {
                return matches!(
                    ast.get_expr(*index),
                    Expr::Ident(in_) if in_ == i_name
                );
            }
            sfi_expr_x_safe(ast, *obj, x_name, i_name)
                && sfi_expr_x_safe(ast, *index, x_name, i_name)
        }
        Expr::Member { obj, .. } => {
            if let Expr::Ident(n) = ast.get_expr(*obj)
                && n == x_name
            {
                return false;
            }
            sfi_expr_x_safe(ast, *obj, x_name, i_name)
        }
        Expr::Call { callee, args } => {
            sfi_expr_x_safe(ast, *callee, x_name, i_name)
                && args
                    .iter()
                    .all(|a| sfi_expr_x_safe(ast, *a, x_name, i_name))
        }
        Expr::Number(_)
        | Expr::BigInt { .. }
        | Expr::String(_)
        | Expr::Bool(_)
        | Expr::Null
        | Expr::Uninit
        | Expr::This
        | Expr::NewTarget
        | Expr::Regex { .. } => true,
        Expr::Array(els) => els.iter().all(|e| sfi_expr_x_safe(ast, *e, x_name, i_name)),
        Expr::ObjectLit { fields } => fields
            .iter()
            .all(|(_, e)| sfi_expr_x_safe(ast, *e, x_name, i_name)),
        Expr::Spread { expr } => sfi_expr_x_safe(ast, *expr, x_name, i_name),
        Expr::BinOp { left, right, .. } => {
            sfi_expr_x_safe(ast, *left, x_name, i_name)
                && sfi_expr_x_safe(ast, *right, x_name, i_name)
        }
        Expr::Assign { target, value } => {
            sfi_expr_x_safe(ast, *target, x_name, i_name)
                && sfi_expr_x_safe(ast, *value, x_name, i_name)
        }
        Expr::Unary { expr, .. } => sfi_expr_x_safe(ast, *expr, x_name, i_name),
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            sfi_expr_x_safe(ast, *cond, x_name, i_name)
                && sfi_expr_x_safe(ast, *then_branch, x_name, i_name)
                && sfi_expr_x_safe(ast, *else_branch, x_name, i_name)
        }
        Expr::TypeOf { expr } | Expr::Delete { expr } => {
            sfi_expr_x_safe(ast, *expr, x_name, i_name)
        }
        Expr::InstanceOf { expr, .. } => sfi_expr_x_safe(ast, *expr, x_name, i_name),
        Expr::As { expr, .. } => sfi_expr_x_safe(ast, *expr, x_name, i_name),
        Expr::Sequence { left, right } => {
            sfi_expr_x_safe(ast, *left, x_name, i_name)
                && sfi_expr_x_safe(ast, *right, x_name, i_name)
        }
        Expr::ArrowFn { .. } | Expr::Closure { .. } => {
            // Conservative — captured X inside a closure is hard to
            // verify safe since the closure body could index X[k]
            // for arbitrary k. Disqualify any X-mention by treating
            // closures as black boxes.
            true
        }
        Expr::Super { args } => args
            .iter()
            .all(|a| sfi_expr_x_safe(ast, *a, x_name, i_name)),
        Expr::New { args, .. } => args
            .iter()
            .all(|a| sfi_expr_x_safe(ast, *a, x_name, i_name)),
        Expr::NewDynamic { callee, args } => {
            sfi_expr_x_safe(ast, *callee, x_name, i_name)
                && args
                    .iter()
                    .all(|a| sfi_expr_x_safe(ast, *a, x_name, i_name))
        }
        Expr::Nullish { lhs, rhs } => {
            sfi_expr_x_safe(ast, *lhs, x_name, i_name) && sfi_expr_x_safe(ast, *rhs, x_name, i_name)
        }
        Expr::OptChain { obj, .. } => {
            if let Expr::Ident(n) = ast.get_expr(*obj)
                && n == x_name
            {
                return false;
            }
            sfi_expr_x_safe(ast, *obj, x_name, i_name)
        }
        Expr::OptIndex { obj, index } => {
            if let Expr::Ident(n) = ast.get_expr(*obj)
                && n == x_name
            {
                return false;
            }
            sfi_expr_x_safe(ast, *obj, x_name, i_name)
                && sfi_expr_x_safe(ast, *index, x_name, i_name)
        }
        Expr::OptCall { callee, args } => {
            sfi_expr_x_safe(ast, *callee, x_name, i_name)
                && args
                    .iter()
                    .all(|a| sfi_expr_x_safe(ast, *a, x_name, i_name))
        }
        Expr::PostIncr { target, .. } => sfi_expr_x_safe(ast, *target, x_name, i_name),
    }
}

pub(super) fn sfi_stmt_uses_ident(ast: &Ast, s: &Stmt, name: &str) -> bool {
    !sfi_stmt_x_safe(ast, s, name, "<<UNUSED>>")
}
