//! The names a statement list binds BEFORE any of it runs — the two
//! kinds the free-variable walk has to pre-bind on entry.
//!
//! A function declaration hoists: a reference anywhere in the holding
//! list, including before the declaration, resolves to it.
//!
//! A `var` hoists further — it is function-scoped, however deep in
//! blocks it is written.
//!
//! `var` is function-scoped: `{ var q = 1; } q` is one binding, visible
//! before and after the block. The free-variable walk treats a block as
//! a binding region and drops what it declared on the way out, which is
//! right for `let` and wrong for `var` — a reference after the block
//! then looks free, and a closure around it captures a name its own
//! body declares. Binding them up front, the way `var` hoisting itself
//! does, is the fix.
//!
//! The walk stops at a nested function: its `var`s belong to it.

use super::Stmt;

/// Function declarations hoist: a reference anywhere in the holding
/// list — including before the declaration — resolves to the local
/// decl, never to an outer binding. Bind every direct `FnDecl` name
/// before walking the list's statements.
pub(super) fn hoist_fn_decl_names(list: &[Stmt], bound: &mut Vec<String>) {
    for s in list {
        if let Stmt::FnDecl { name, .. } = s {
            if !bound.contains(name) {
                bound.push(name.clone());
            }
        }
    }
}

pub(super) fn hoist_var_names(list: &[Stmt], bound: &mut Vec<String>) {
    for s in list {
        walk(s, bound);
    }
}

fn walk(s: &Stmt, bound: &mut Vec<String>) {
    match s {
        Stmt::LetDecl {
            name, is_var: true, ..
        } => {
            if !bound.contains(name) {
                bound.push(name.clone());
            }
        }
        Stmt::Block(b) | Stmt::Multi(b) => hoist_var_names(b, bound),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            walk(then_branch, bound);
            if let Some(eb) = else_branch.as_deref() {
                walk(eb, bound);
            }
        }
        Stmt::While { body, .. }
        | Stmt::DoWhile { body, .. }
        | Stmt::For { body, .. }
        | Stmt::ForOf { body, .. }
        | Stmt::ForOfSplitIter { body, .. }
        | Stmt::Labeled { body, .. } => walk(body, bound),
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            hoist_var_names(body, bound);
            hoist_var_names(catch_body, bound);
            if let Some(fb) = finally_body {
                hoist_var_names(fb, bound);
            }
        }
        Stmt::Switch { cases, default, .. } => {
            for c in cases {
                hoist_var_names(&c.body, bound);
            }
            if let Some(d) = default {
                hoist_var_names(d, bound);
            }
        }
        _ => {}
    }
}
