//! The type a closure captures a null-narrowed binding at.
//!
//! A closure reads a captured binding whenever it is called, not where
//! it is created, so the narrow in force at creation may already be
//! undone by then: by a write after the closure, by one earlier in an
//! enclosing loop, or by one inside another closure. When any write to
//! the binding can exist, the closure sees the `T | null` the narrow
//! replaced (`LocalInfo::null_declared`); a binding nothing writes
//! again keeps its narrow.
//!
//! Where the writes can be is known exactly only for a local of a
//! top-level function being walked directly: its own body, and the
//! bodies of the closures in it that capture the name. Everywhere else
//! (top-level code, a closure body, a nested declaration) every write
//! to the name in the program counts.

use std::collections::HashSet;

use super::*;
use crate::ast::desugar_async_sm_walk::{visit_expr_children, visit_stmt_exprs};
use crate::ast::stmt_nested_lists::for_each_nested_list;

impl Checker {
    pub(crate) fn capture_ty(&self, ast: &Ast, name: &str, info: &LocalInfo) -> Type {
        match &info.null_declared {
            Some(declared)
                if info.mutable
                    && !matches!(info.ty, Type::Nullable(_))
                    && self.may_be_written(ast, name) =>
            {
                declared.clone()
            }
            _ => info.ty.clone(),
        }
    }

    fn may_be_written(&self, ast: &Ast, name: &str) -> bool {
        let root = self.narrows.root.as_deref().and_then(|r| {
            ast.stmts.iter().find_map(|s| match s {
                Stmt::FnDecl { name, body, .. } if name == r => Some(body),
                _ => None,
            })
        });
        match root {
            Some(body) => body_writes(ast, body, name, &mut HashSet::new()),
            None => any_write(ast, name),
        }
    }
}

/// Does any expression of the program store into a binding spelled
/// `name`?
fn any_write(ast: &Ast, name: &str) -> bool {
    ast.exprs.iter().any(|e| match e {
        Expr::Assign { target, .. } | Expr::PostIncr { target, .. } => is_name(ast, *target, name),
        _ => false,
    })
}

fn is_name(ast: &Ast, e: ExprId, name: &str) -> bool {
    matches!(ast.get_expr(e), Expr::Ident(n) if n == name)
}

/// Does `body`, or a closure in it capturing `name`, store into `name`?
fn body_writes(ast: &Ast, body: &[Stmt], name: &str, seen: &mut HashSet<String>) -> bool {
    if body.iter().any(has_decl) {
        return any_write(ast, name);
    }
    let mut exprs = Vec::new();
    for s in body {
        visit_stmt_exprs(s, &mut |e| exprs.push(e));
    }
    exprs.into_iter().any(|e| expr_writes(ast, e, name, seen))
}

fn expr_writes(ast: &Ast, e: ExprId, name: &str, seen: &mut HashSet<String>) -> bool {
    let own = match ast.get_expr(e) {
        Expr::Assign { target, .. } | Expr::PostIncr { target, .. } => is_name(ast, *target, name),
        Expr::Closure { fn_name, captures } if captures.iter().any(|c| c == name) => {
            seen.insert(fn_name.clone())
                && ast
                    .stmts
                    .iter()
                    .find_map(|s| match s {
                        Stmt::FnDecl { name: n, body, .. } if n == fn_name => Some(body),
                        _ => None,
                    })
                    .is_none_or(|b| body_writes(ast, b, name, seen))
        }
        Expr::ArrowFn { body, .. } => body_writes(ast, body, name, seen),
        _ => false,
    };
    if own {
        return true;
    }
    let mut children = Vec::new();
    visit_expr_children(ast, e, &mut |c| children.push(c));
    children
        .into_iter()
        .any(|c| expr_writes(ast, c, name, seen))
}

/// Does `s` hold a function or class declaration anywhere?
fn has_decl(s: &Stmt) -> bool {
    if matches!(s, Stmt::FnDecl { .. } | Stmt::ClassDecl { .. }) {
        return true;
    }
    let mut hit = false;
    for_each_nested_list(s, &mut |l| hit = hit || l.iter().any(has_decl));
    hit
}
