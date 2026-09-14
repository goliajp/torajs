//! RFC 20260915 blade 5 — the one `await using` shape the machine
//! cannot honour yet: a binding read BEFORE its declaration.
//!
//! `function f() { return x + 1; }` declared ahead of `await using x
//! = null` and called before it must throw a ReferenceError (§8.1.1.1
//! temporal dead zone; test262 `await-using/*-before-initialization`,
//! `for-of/head-await-using-bound-names-fordecl-tdz` — the loop's
//! source `[x]` reads the head's own `x`). Inside a machine every
//! `let` is a field of the iterator, initialised to `undefined` the
//! moment the object exists, so the read answers `undefined` and the
//! throw is gone (`Ast::tdz_reads` is computed last in the pipeline
//! and only for a top-level statement list — the lifted field has
//! neither a `let` nor a statement order left). Making that right
//! needs a TDZ sentinel on lifted fields — registered — so a body
//! whose `await using` name is referenced anywhere ahead of the
//! declaration keeps the synchronous lowering, the posture blade 5
//! replaced for every other shape.
//!
//! Textual precedence is deliberately coarse (a same-named binding in
//! an unrelated earlier scope refuses too): the direction is the
//! gate's — refusing keeps today's lowering, never the wrong one.

use super::desugar_with::walk;
use super::{Ast, Expr, ExprId, Stmt};
use std::collections::HashSet;

/// Does some `await using` binding of `body` (a `const x =
/// __torajs_using_add_async(env, …)` by this pass) have its name read
/// or written anywhere textually ahead of that declaration, nested
/// functions included?
pub(super) fn using_bound_read_before_decl(ast: &Ast, body: &[Stmt]) -> bool {
    let mut seen: HashSet<String> = HashSet::new();
    stmts_read_before(ast, body, &mut seen)
}

fn stmts_read_before(ast: &Ast, stmts: &[Stmt], seen: &mut HashSet<String>) -> bool {
    stmts.iter().any(|s| stmt_read_before(ast, s, seen))
}

/// Pre-order: a declaration's own initializer is evaluated before the
/// binding exists (`await using x = x` is a TDZ read too), so the
/// statement's expressions are collected first and the declaration
/// checked after.
fn stmt_read_before(ast: &Ast, s: &Stmt, seen: &mut HashSet<String>) -> bool {
    for e in walk::stmt_exprs(s) {
        collect_idents(ast, e, seen);
    }
    if let Stmt::LetDecl { name, init, .. } = s
        && is_async_using_add(ast, *init)
        && seen.contains(name)
    {
        return true;
    }
    walk::stmt_children_ref(s)
        .into_iter()
        .any(|c| stmt_read_before(ast, c, seen))
}

fn is_async_using_add(ast: &Ast, init: ExprId) -> bool {
    matches!(ast.get_expr(init), Expr::Call { callee, .. }
        if matches!(ast.get_expr(*callee), Expr::Ident(n) if n == "__torajs_using_add_async"))
}

fn collect_idents(ast: &Ast, eid: ExprId, seen: &mut HashSet<String>) {
    match ast.get_expr(eid) {
        Expr::Ident(n) => {
            seen.insert(n.clone());
        }
        Expr::ArrowFn { body, .. } => {
            for s in body {
                for e in walk::stmt_exprs(s) {
                    collect_idents(ast, e, seen);
                }
                for c in walk::stmt_children_ref(s) {
                    collect_stmt_idents(ast, c, seen);
                }
            }
        }
        _ => {
            for c in walk::expr_children(ast, eid) {
                collect_idents(ast, c, seen);
            }
        }
    }
}

fn collect_stmt_idents(ast: &Ast, s: &Stmt, seen: &mut HashSet<String>) {
    for e in walk::stmt_exprs(s) {
        collect_idents(ast, e, seen);
    }
    for c in walk::stmt_children_ref(s) {
        collect_stmt_idents(ast, c, seen);
    }
}
