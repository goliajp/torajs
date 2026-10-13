//! Read-only scans of a `for (let …)` loop for the boxing in the parent
//! module: which closures read the binding, and where its plain
//! references are.

use crate::ast::desugar_with::walk::{expr_children, stmt_children_ref, stmt_exprs};
use crate::ast::{Ast, Expr, ExprId, Stmt};

/// The expressions `s` evaluates itself — a `for` head's declaration
/// included — not its child statements'.
pub(super) fn own_arrows(ast: &Ast, s: &Stmt) -> Vec<ExprId> {
    let mut roots = stmt_exprs(s);
    if let Stmt::For { init: Some(i), .. } = s {
        roots.extend(stmt_exprs(i));
    }
    arrows_in(ast, &roots)
}

pub(super) fn stmt_roots(s: &Stmt, out: &mut Vec<ExprId>) {
    out.extend(stmt_exprs(s));
    for c in stmt_children_ref(s) {
        stmt_roots(c, out);
    }
}

/// Whether a function or class declared in the loop body reads `name`.
pub(super) fn decl_reads(ast: &Ast, s: &Stmt, name: &str) -> bool {
    match s {
        Stmt::FnDecl { .. } => {
            crate::ast::free_vars::free_vars_of_body(ast, &[], std::slice::from_ref(s))
                .iter()
                .any(|n| n == name)
        }
        Stmt::ClassDecl { .. } => true,
        _ => stmt_children_ref(s)
            .into_iter()
            .any(|c| decl_reads(ast, c, name)),
    }
}

/// The closures reachable from `roots` without entering one.
pub(super) fn arrows_in(ast: &Ast, roots: &[ExprId]) -> Vec<ExprId> {
    let mut out = Vec::new();
    let mut stack = roots.to_vec();
    while let Some(e) = stack.pop() {
        match ast.get_expr(e) {
            Expr::ArrowFn { .. } => out.push(e),
            _ => stack.extend(expr_children(ast, e)),
        }
    }
    out
}

pub(super) fn arrow_reads(ast: &Ast, a: ExprId, name: &str) -> bool {
    let Expr::ArrowFn { params, body, .. } = ast.get_expr(a) else {
        return false;
    };
    let bound: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
    crate::ast::free_vars::free_vars_of_body(ast, &bound, body)
        .iter()
        .any(|n| n == name)
}

/// `Ident(name)` nodes reachable from `roots` without entering a
/// closure.
pub(super) fn idents_outside_arrows(ast: &Ast, roots: &[ExprId], name: &str) -> Vec<ExprId> {
    let mut out = Vec::new();
    let mut stack = roots.to_vec();
    while let Some(e) = stack.pop() {
        match ast.get_expr(e) {
            Expr::Ident(n) if n == name => out.push(e),
            Expr::ArrowFn { .. } => {}
            _ => stack.extend(expr_children(ast, e)),
        }
    }
    out
}

/// Every `Ident(name)` under `e`, closures and the function
/// declarations inside them included. The rename already stopped
/// where a nested scope rebinds the name.
pub(super) fn collect_deep(ast: &Ast, e: ExprId, name: &str, out: &mut Vec<ExprId>) {
    match ast.get_expr(e) {
        Expr::Ident(n) if n == name => out.push(e),
        Expr::ArrowFn { body, .. } => {
            for s in body {
                collect_deep_stmt(ast, s, name, out);
            }
        }
        _ => {
            for c in expr_children(ast, e) {
                collect_deep(ast, c, name, out);
            }
        }
    }
}

fn collect_deep_stmt(ast: &Ast, s: &Stmt, name: &str, out: &mut Vec<ExprId>) {
    for e in stmt_exprs(s) {
        collect_deep(ast, e, name, out);
    }
    let children: Vec<&Stmt> = match s {
        Stmt::FnDecl { body, .. } => body.iter().collect(),
        _ => stmt_children_ref(s),
    };
    for c in children {
        collect_deep_stmt(ast, c, name, out);
    }
}
