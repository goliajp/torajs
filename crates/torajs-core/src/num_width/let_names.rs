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

/// One binding the walk found: its name, plus the `LetDecl` it came
/// from when it is one (a for-of element binding and a catch param
/// are bindings too, but carry no initializer to read).
pub(super) type BindingVisit<'v> = &'v mut dyn FnMut(&str, Option<&Stmt>);

/// Flat (block-insensitive) collection of every `let` name in a fn
/// body. Nested FnDecls are their own scope and excluded.
pub(super) fn collect_let_names(s: &Stmt, out: &mut HashSet<String>) {
    walk_bindings(s, &mut |name, _| {
        out.insert(name.to_string());
    });
}

/// The walk itself — every binding site in a fn body, block-
/// insensitive, nested `FnDecl`s excluded. `collect_let_names` wants
/// the names; the const-int table (`analyze_tables`) wants the
/// initializers, so the walk hands both to the visitor.
pub(super) fn walk_bindings(s: &Stmt, f: BindingVisit) {
    match s {
        Stmt::LetDecl { name, .. } => {
            f(name, Some(s));
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            walk_bindings(then_branch, f);
            if let Some(eb) = else_branch {
                walk_bindings(eb, f);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => walk_bindings(body, f),
        Stmt::Labeled { body, .. } => walk_bindings(body, f),
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                walk_bindings(i, f);
            }
            walk_bindings(body, f);
        }
        // W4 (D2) — the for-of element binding is a per-iteration let;
        // registering it makes its key resolvable so the elem-width
        // dep from the pre-built `src[i]` read lands on it, in the
        // same commit as the lowering sites that consume the table.
        Stmt::ForOf { var_name, body, .. } => {
            f(var_name, None);
            walk_bindings(body, f);
        }
        Stmt::ForOfSplitIter { body, .. } => walk_bindings(body, f),
        Stmt::Switch { cases, default, .. } => {
            for c in cases {
                for cs in &c.body {
                    walk_bindings(cs, f);
                }
            }
            if let Some(d) = default {
                for ds in d {
                    walk_bindings(ds, f);
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
                f(p, None);
            }
            for bs in body {
                walk_bindings(bs, f);
            }
            for cs in catch_body {
                walk_bindings(cs, f);
            }
            if let Some(fb) = finally_body {
                for fs in fb {
                    walk_bindings(fs, f);
                }
            }
        }
        Stmt::Block(v) | Stmt::Multi(v) => {
            for bs in v {
                walk_bindings(bs, f);
            }
        }
        _ => {}
    }
}
