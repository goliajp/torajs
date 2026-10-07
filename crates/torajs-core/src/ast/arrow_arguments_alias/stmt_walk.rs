//! The statement half of the alias pass's scope walk; the expression
//! half and the scope bookkeeping stay in the parent.

use super::{Ast, Hits, Mode, Stmt, params_shadow, process_scope, scan_expr};

/// Statement walk within one fn scope (`mode` — see [`Mode`]). A
/// nested `FnDecl` is its own scope — recurse through
/// [`process_scope`] and do not extend the current one.
pub(super) fn scan_stmt(ast: &mut Ast, s: &mut Stmt, mode: Mode, hits: &mut Hits) {
    match s {
        Stmt::FnDecl { params, body, .. } => {
            let shadowed = params_shadow(params);
            let mut b = std::mem::take(body);
            process_scope(ast, &mut b, shadowed);
            *body = b;
        }
        Stmt::Expr(e) | Stmt::Throw(e) | Stmt::Yield(e) => scan_expr(ast, *e, mode, hits),
        Stmt::Return(Some(e)) => {
            scan_expr(ast, *e, mode, hits);
        }
        Stmt::LetDecl { init, .. } | Stmt::UsingDecl { init, .. } => {
            scan_expr(ast, *init, mode, hits);
        }
        Stmt::YieldInto { value, .. } => scan_expr(ast, *value, mode, hits),
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            scan_expr(ast, *cond, mode, hits);
            scan_stmt(ast, then_branch, mode, hits);
            if let Some(e) = else_branch {
                scan_stmt(ast, e, mode, hits);
            }
        }
        Stmt::While { cond, body } | Stmt::DoWhile { cond, body } => {
            scan_expr(ast, *cond, mode, hits);
            scan_stmt(ast, body, mode, hits);
        }
        Stmt::Switch {
            scrutinee,
            cases,
            default,
            ..
        } => {
            scan_expr(ast, *scrutinee, mode, hits);
            for c in cases.iter_mut() {
                scan_expr(ast, c.value, mode, hits);
                for cs in c.body.iter_mut() {
                    scan_stmt(ast, cs, mode, hits);
                }
            }
            if let Some(d) = default {
                for ds in d.iter_mut() {
                    scan_stmt(ast, ds, mode, hits);
                }
            }
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            if let Some(s0) = init {
                scan_stmt(ast, s0, mode, hits);
            }
            if let Some(c) = cond {
                scan_expr(ast, *c, mode, hits);
            }
            if let Some(st) = step {
                scan_expr(ast, *st, mode, hits);
            }
            scan_stmt(ast, body, mode, hits);
        }
        Stmt::ForOfSplitIter {
            parent, sep, body, ..
        } => {
            scan_expr(ast, *parent, mode, hits);
            scan_expr(ast, *sep, mode, hits);
            scan_stmt(ast, body, mode, hits);
        }
        Stmt::ForOf {
            elem_expr, body, ..
        } => {
            scan_expr(ast, *elem_expr, mode, hits);
            scan_stmt(ast, body, mode, hits);
        }
        Stmt::Labeled { body, .. } => scan_stmt(ast, body, mode, hits),
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            for bs in body.iter_mut() {
                scan_stmt(ast, bs, mode, hits);
            }
            for cs in catch_body.iter_mut() {
                scan_stmt(ast, cs, mode, hits);
            }
            if let Some(fb) = finally_body {
                for fs in fb.iter_mut() {
                    scan_stmt(ast, fs, mode, hits);
                }
            }
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            for bs in stmts.iter_mut() {
                scan_stmt(ast, bs, mode, hits);
            }
        }
        _ => {}
    }
}
