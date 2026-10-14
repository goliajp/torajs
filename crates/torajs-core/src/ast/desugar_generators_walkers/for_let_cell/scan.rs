//! Read-only scans of a `for (let …)` loop for the boxing in the parent
//! module: which closures read the binding, and where its plain
//! references are.

use crate::ast::desugar_with::walk::{expr_children, stmt_children_ref, stmt_exprs};
use crate::ast::{Ast, Expr, ExprId, Stmt};

/// The closures made by the expressions `s` evaluates itself — a `for`
/// head's declarations included, labels seen through — not by its child
/// statements'.
pub(super) fn own_arrows(ast: &Ast, s: &Stmt) -> Vec<ExprId> {
    if let Stmt::Labeled { body, .. } = s {
        return own_arrows(ast, body);
    }
    let mut roots = stmt_exprs(s);
    if let Stmt::For { init: Some(i), .. } = s {
        for d in head_decls(i) {
            roots.extend(stmt_exprs(d));
        }
    }
    arrows_in(ast, &roots)
}

/// The statements of a `for` head in order: the head itself, or the
/// members of its `Multi` — several declarators, or the flag the
/// parser adds to a loop it rotated.
pub(super) fn head_decls(init: &Stmt) -> Vec<&Stmt> {
    match init {
        Stmt::Multi(v) => v.iter().flat_map(head_decls).collect(),
        s => vec![s],
    }
}

/// [`head_decls`] for a rewrite.
pub(super) fn head_decls_mut(init: &mut Stmt) -> Vec<&mut Stmt> {
    match init {
        Stmt::Multi(v) => v.iter_mut().flat_map(head_decls_mut).collect(),
        s => vec![s],
    }
}

/// Whether `name` occurs anywhere in what `s` evaluates itself,
/// closures included.
pub(super) fn mentions(ast: &Ast, s: &Stmt, name: &str) -> bool {
    let mut out = Vec::new();
    for e in stmt_exprs(s) {
        collect_deep(ast, e, name, &mut out);
    }
    !out.is_empty()
}

pub(super) fn stmt_roots(s: &Stmt, out: &mut Vec<ExprId>) {
    out.extend(stmt_exprs(s));
    for c in stmt_children_ref(s) {
        stmt_roots(c, out);
    }
}

/// Whether a function declared in the loop body reads `name`. A class
/// is no such declaration: it is made where its statement runs
/// (`class_member`).
pub(super) fn decl_reads(ast: &Ast, s: &Stmt, name: &str) -> bool {
    match s {
        Stmt::FnDecl { .. } => {
            crate::ast::free_vars::free_vars_of_body(ast, &[], std::slice::from_ref(s))
                .iter()
                .any(|n| n == name)
        }
        Stmt::ClassDecl { .. } => false,
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

pub(super) fn collect_deep_stmt(ast: &Ast, s: &Stmt, name: &str, out: &mut Vec<ExprId>) {
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

/// Whether something under `roots`, closures and the function
/// declarations inside them included, assigns `name`.
pub(super) fn assigns_deep(ast: &Ast, roots: &[ExprId], name: &str) -> bool {
    let mut idents = Vec::new();
    for &e in roots {
        collect_deep(ast, e, name, &mut idents);
    }
    !idents.is_empty()
        && ast.exprs.iter().any(|x| {
            matches!(x, Expr::Assign { target, .. } | Expr::PostIncr { target, .. }
                if idents.contains(target))
        })
}
