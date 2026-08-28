//! Which top-level fn names an arrow's enclosing scopes rebind.
//!
//! `free_vars_of_arrow` pre-binds every top-level fn name so a
//! reference to one does not land in the capture set. That is right
//! until a local binding SHADOWS the name: the reference then belongs
//! to the local, the arrow has to capture it, and pre-binding hands it
//! to the declaration instead — `typeof g` reading `object` outside an
//! arrow and `function` inside it, in the same call (RFC 20260828).
//! `lift_arrow_fns` scans the expression arena flat and cannot see
//! which body an arrow sits in, so the answer is computed here, once,
//! off the statement tree.
//!
//! Over-reporting is the dangerous direction. Dropping a name from the
//! pre-bind set where it is NOT shadowed turns a genuine top-level call
//! inside an arrow into `closure capture 'top' not in scope` — measured,
//! by emptying the set outright. Under-reporting only leaves a shape
//! unfixed. So a scope closes as soon as the language closes it, and a
//! construct whose scoping is unclear is left out rather than guessed.

use super::desugar_with::walk::{arrow_nodes, stmt_children_ref};
use super::{Ast, Expr, ExprId, Stmt};
use std::collections::HashMap;

/// Arrow `ExprId` → the subset of `globals` its enclosing scopes bind.
/// Absent key = nothing shadowed, which is the common case.
pub(super) fn shadowed_globals_by_arrow(
    ast: &Ast,
    globals: &[String],
) -> HashMap<ExprId, Vec<String>> {
    let mut out: HashMap<ExprId, Vec<String>> = HashMap::new();
    if globals.is_empty() {
        return out;
    }
    let mut scope: Vec<String> = Vec::new();
    walk_list(ast, &ast.stmts, globals, &mut scope, &mut out);
    out
}

/// A statement list that owns its bindings — everything pushed while
/// walking it goes out of scope at the end.
fn walk_list(
    ast: &Ast,
    list: &[Stmt],
    globals: &[String],
    scope: &mut Vec<String>,
    out: &mut HashMap<ExprId, Vec<String>>,
) {
    let depth = scope.len();
    for s in list {
        walk_stmt(ast, s, globals, scope, out);
    }
    scope.truncate(depth);
}

fn walk_stmt(
    ast: &Ast,
    s: &Stmt,
    globals: &[String],
    scope: &mut Vec<String>,
    out: &mut HashMap<ExprId, Vec<String>>,
) {
    // Arrows rooted in this statement's expressions see the scope as it
    // stands here. `arrow_nodes` walks expression children only, so a
    // nested arrow living inside another arrow's BODY (a statement
    // list, not an expression child) is reached by the recursion below
    // rather than by this call.
    for eid in arrow_nodes(ast, s) {
        record(globals, scope, eid, out);
        if let Expr::ArrowFn { params, body, .. } = ast.get_expr(eid) {
            let depth = scope.len();
            scope.extend(params.iter().map(|p| p.name.clone()));
            walk_list(ast, body, globals, scope, out);
            scope.truncate(depth);
        }
    }
    match s {
        // Visible to the statements that follow it in this list.
        Stmt::LetDecl { name, .. } | Stmt::UsingDecl { name, .. } => {
            scope.push(name.clone());
            return;
        }
        Stmt::FnDecl { params, body, .. } => {
            let depth = scope.len();
            scope.extend(params.iter().map(|p| p.name.clone()));
            walk_list(ast, body, globals, scope, out);
            scope.truncate(depth);
            return;
        }
        Stmt::ForOf { var_name, body, .. } => {
            let depth = scope.len();
            scope.push(var_name.clone());
            walk_stmt(ast, body, globals, scope, out);
            scope.truncate(depth);
            return;
        }
        Stmt::Try {
            body,
            catch_param,
            catch_body,
            finally_body,
            ..
        } => {
            walk_list(ast, body, globals, scope, out);
            let depth = scope.len();
            if let Some(cp) = catch_param {
                scope.push(cp.clone());
            }
            walk_list(ast, catch_body, globals, scope, out);
            scope.truncate(depth);
            if let Some(fb) = finally_body {
                walk_list(ast, fb, globals, scope, out);
            }
            return;
        }
        // A compiler-minted sequence SHARES the surrounding scope (the
        // destructuring expansion's lets must stay visible to the
        // statements after it), so its children are not fenced off.
        Stmt::Multi(list) => {
            for child in list {
                walk_stmt(ast, child, globals, scope, out);
            }
            return;
        }
        _ => {}
    }
    // Every other child opens and closes its own frame. `Block` is the
    // real one; the rest hold a single statement that a binding cannot
    // legally escape anyway.
    for child in stmt_children_ref(s) {
        let depth = scope.len();
        walk_stmt(ast, child, globals, scope, out);
        scope.truncate(depth);
    }
}

fn record(
    globals: &[String],
    scope: &[String],
    eid: ExprId,
    out: &mut HashMap<ExprId, Vec<String>>,
) {
    let hit: Vec<String> = globals
        .iter()
        .filter(|g| scope.iter().any(|b| b == *g))
        .cloned()
        .collect();
    if !hit.is_empty() {
        out.insert(eid, hit);
    }
}
