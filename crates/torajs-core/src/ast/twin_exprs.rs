//! The expressions the receiver-polymorphic twins hold — the census
//! leaves them out.
//!
//! A `__cmany_` / `__smany_` / `__ctorany_` twin is the compiler's own
//! clone of a method body with every parameter retyped `any`, minted
//! so a rebound or under-supplied bare call has somewhere correct to
//! run. Its expressions are the mono's expressions counted a second
//! time, and the second count always lands in the `any` bucket, so a
//! method that gains a twin reads as a typed-coverage loss although
//! the user's program and its typed body are unchanged. The census
//! therefore skips them: the mono already scored the same source.
//! A generator's all-`any` twin (`desugar_generators_any_twin`) is the
//! same kind of clone, its factory and its iterator class's methods
//! alike.

use std::collections::HashSet;

use super::desugar_async_sm_walk::{visit_expr_children, visit_stmt_exprs};
use super::{Ast, Expr, ExprId, Stmt};

pub(crate) fn twin_body_exprs(ast: &Ast) -> HashSet<ExprId> {
    let mut out: HashSet<ExprId> = HashSet::new();
    for stmt in &ast.stmts {
        let Stmt::FnDecl {
            name, params, body, ..
        } = stmt
        else {
            continue;
        };
        if !(name.starts_with("__cmany_")
            || name.starts_with("__smany_")
            || name.starts_with("__ctorany_")
            || crate::ast::is_generator_any_twin_decl(name))
        {
            continue;
        }
        let mut roots: Vec<ExprId> = params.iter().filter_map(|p| p.default).collect();
        for s in body {
            visit_stmt_exprs(s, &mut |e| roots.push(e));
        }
        while let Some(e) = roots.pop() {
            if out.insert(e) {
                visit_expr_children(ast, e, &mut |c| roots.push(c));
            }
        }
    }
    out
}

/// How a body may read a binding ([`body_reads`]).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum NameRead {
    No,
    /// Only through identifiers of the body's own scope.
    Direct,
    /// Through a closure capture, a direct `eval`, or a nested function
    /// or class body this walk does not enter.
    Nested,
}

/// How `body` may read the binding `name`.
pub(crate) fn body_reads(ast: &Ast, body: &[Stmt], name: &str) -> NameRead {
    let mut read = NameRead::No;
    crate::ast::walk_stmts(body, &mut |s| {
        if matches!(s, Stmt::FnDecl { .. } | Stmt::ClassDecl { .. }) {
            read = NameRead::Nested;
        }
    });
    let mut roots: Vec<ExprId> = Vec::new();
    for s in body {
        visit_stmt_exprs(s, &mut |e| roots.push(e));
    }
    let mut seen: HashSet<ExprId> = HashSet::new();
    while let Some(e) = roots.pop() {
        if !seen.insert(e) {
            continue;
        }
        let here = match ast.get_expr(e) {
            Expr::Ident(n) if n == "eval" => NameRead::Nested,
            Expr::Ident(n) if n == name => NameRead::Direct,
            Expr::Closure { captures, .. } if captures.iter().any(|c| c == name) => {
                NameRead::Nested
            }
            Expr::ArrowFn { .. } => NameRead::Nested,
            _ => NameRead::No,
        };
        read = read.max(here);
        visit_expr_children(ast, e, &mut |c| roots.push(c));
    }
    read
}
