//! The `let` names a function body declares — a flat syntactic scan
//! with no analysis state behind it, which is why it sits beside the
//! constraint walk rather than inside it. `Scope` needs the set to
//! tell a block-local binding from an outer one, and `fallthrough`
//! needs it for the same reason at return sites.
//!
//! Block-insensitive on purpose: a name declared in any nested block
//! belongs to the function, and the width lattice joins over all of
//! its writes regardless of which block each one sits in. A nested
//! `FnDecl` is its own scope and stops the walk.

use crate::ast::Stmt;
use std::collections::HashSet;

pub(super) fn collect_let_names_fn(stmt: &Stmt) -> HashSet<String> {
    let mut s = HashSet::new();
    if let Stmt::FnDecl { body, .. } = stmt {
        for b in body {
            collect_let_names(b, &mut s);
        }
    }
    s
}

/// Flat (block-insensitive) collection of every `let` name in a fn
/// body. Nested FnDecls are their own scope and excluded.
pub(super) fn collect_let_names(s: &Stmt, out: &mut HashSet<String>) {
    match s {
        Stmt::LetDecl { name, .. } => {
            out.insert(name.clone());
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            collect_let_names(then_branch, out);
            if let Some(eb) = else_branch {
                collect_let_names(eb, out);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => collect_let_names(body, out),
        Stmt::Labeled { body, .. } => collect_let_names(body, out),
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                collect_let_names(i, out);
            }
            collect_let_names(body, out);
        }
        // W4 (D2) — the for-of element binding is a per-iteration let;
        // registering it makes its key resolvable so the elem-width
        // dep from the pre-built `src[i]` read lands on it, in the
        // same commit as the lowering sites that consume the table.
        Stmt::ForOf { var_name, body, .. } => {
            out.insert(var_name.clone());
            collect_let_names(body, out);
        }
        Stmt::ForOfSplitIter { body, .. } => collect_let_names(body, out),
        Stmt::Switch { cases, default, .. } => {
            for c in cases {
                for cs in &c.body {
                    collect_let_names(cs, out);
                }
            }
            if let Some(d) = default {
                for ds in d {
                    collect_let_names(ds, out);
                }
            }
        }
        Stmt::Try {
            body,
            catch_param,
            catch_body,
            finally_body,
            ..
        } => {
            // The catch binding is a slot like any other — it has to be
            // resolvable for the pending-throw join below to reach it.
            if let Some(p) = catch_param {
                out.insert(p.clone());
            }
            for bs in body {
                collect_let_names(bs, out);
            }
            for cs in catch_body {
                collect_let_names(cs, out);
            }
            if let Some(fb) = finally_body {
                for fs in fb {
                    collect_let_names(fs, out);
                }
            }
        }
        Stmt::Block(v) | Stmt::Multi(v) => {
            for bs in v {
                collect_let_names(bs, out);
            }
        }
        _ => {}
    }
}
