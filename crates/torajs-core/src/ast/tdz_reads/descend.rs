//! The statement lists nested in a statement, each run as a list of
//! its own.

use std::collections::HashSet;

use super::super::desugar_with::walk::{expr_children, stmt_children_ref, stmt_exprs};
use super::super::{Ast, Expr, ExprId, Stmt};
use super::{flatten, mark_list, run_nested};

/// Every statement list reachable from `s`, each run as its own scope.
pub(super) fn descend_lists(ast: &Ast, s: &Stmt, out: &mut HashSet<ExprId>) {
    match s {
        Stmt::Block(b) => {
            run_nested(ast, b, out);
            return;
        }
        Stmt::Switch {
            cases,
            default,
            default_at,
            ..
        } => {
            let (before, after) = cases.split_at(*default_at);
            let mut flat: Vec<&Stmt> = Vec::new();
            for body in before
                .iter()
                .map(|c| c.body.as_slice())
                .chain(default.as_deref())
                .chain(after.iter().map(|c| c.body.as_slice()))
            {
                flatten(body, &mut flat);
            }
            mark_list(ast, &flat, out);
        }
        // §14.7.4.2 creates the head's lexical bindings before the
        // first initializer runs: `for (let x = k, k = 1; ;)`
        Stmt::For { init: Some(i), .. } => {
            let mut flat: Vec<&Stmt> = Vec::new();
            flatten(std::slice::from_ref(&**i), &mut flat);
            mark_list(ast, &flat, out);
        }
        Stmt::FnDecl { body, .. } => {
            run_nested(ast, body, out);
            return;
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            run_nested(ast, body, out);
            run_nested(ast, catch_body, out);
            if let Some(f) = finally_body {
                run_nested(ast, f, out);
            }
            return;
        }
        _ => {}
    }
    for root in stmt_exprs(s) {
        descend_expr_lists(ast, root, out);
    }
    for c in stmt_children_ref(s) {
        descend_lists(ast, c, out);
    }
}

/// A function expression's body is a list too, and the pipeline runs
/// this pass before the arrows are lifted out of the arena.
fn descend_expr_lists(ast: &Ast, eid: ExprId, out: &mut HashSet<ExprId>) {
    if let Expr::ArrowFn { body, .. } = ast.get_expr(eid) {
        run_nested(ast, body, out);
        return;
    }
    for c in expr_children(ast, eid) {
        descend_expr_lists(ast, c, out);
    }
}
