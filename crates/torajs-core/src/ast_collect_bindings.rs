//! Annotation-predicate binding walks (chunk 765 extraction from
//! [`crate::ast_collect_fn_closure`], which had drifted past the
//! 500-line file limit as store-site axes stacked up).
//!
//! Collect `let`/`const`/`var` binding NAMES whose annotation
//! matches a predicate, recursing through fn bodies and every
//! statement container. Three consumers, differing only in the
//! annotation test:
//!
//! - [`collect_any_bindings`] — `any`-annotated bindings (the
//!   assign-into-any / call-arg-into-any wrap axes).
//! - [`collect_fn_arr_bindings`] — fn-typed ARRAY annotations
//!   (chunk 733; element slots are Closure-repr).
//! - [`collect_fn_ann_bindings`] — plain fn-type annotations at any
//!   scope depth (chunk 736; variadic anns keep their boxed-dual
//!   route).

use std::collections::HashSet;

use crate::ast::{Stmt, is_fn_like_ann};

/// Collect every `let`/`const`/`var` binding name carrying an `any`
/// annotation, recursing through fn bodies and statement containers.
pub(crate) fn collect_any_bindings(stmts: &[Stmt], out: &mut HashSet<String>) {
    collect_bindings_matching(stmts, &|a| a.trim() == "any", out);
}

/// Chunk 733 — binding names declared with a fn-typed array
/// annotation (`((n)=>n)[]` / `Array<(n)=>n>`); their element slots
/// are Closure-repr, so named-fn store-sites into them wrap.
pub(crate) fn collect_fn_arr_bindings(stmts: &[Stmt], out: &mut HashSet<String>) {
    collect_bindings_matching(stmts, &crate::ast::is_fn_arr_ann, out);
}

/// Chunk 736 — binding names declared with a plain fn-type
/// annotation at ANY scope depth (the closure_bindings top-level
/// walk misses fn-body `let cb: (n)=>n` bindings, so a body-local
/// `cb = top_fn` assign escaped the wrap). Variadic anns keep their
/// boxed-dual route.
pub(crate) fn collect_fn_ann_bindings(stmts: &[Stmt], out: &mut HashSet<String>) {
    collect_bindings_matching(stmts, &|a| is_fn_like_ann(a) && !a.contains("__rest("), out);
}

/// Shared annotation-predicate binding walk (chunk 733 — the any
/// and fn-arr collections differ only in the ann test).
fn collect_bindings_matching(
    stmts: &[Stmt],
    pred: &dyn Fn(&str) -> bool,
    out: &mut HashSet<String>,
) {
    for s in stmts {
        collect_bindings_matching_stmt(s, pred, out);
    }
}

fn collect_bindings_matching_stmt(
    s: &Stmt,
    pred: &dyn Fn(&str) -> bool,
    out: &mut HashSet<String>,
) {
    match s {
        Stmt::LetDecl { name, type_ann, .. } => {
            if type_ann.as_deref().is_some_and(pred) {
                out.insert(name.clone());
            }
        }
        Stmt::FnDecl { body, .. } => collect_bindings_matching(body, pred, out),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            collect_bindings_matching_stmt(then_branch, pred, out);
            if let Some(eb) = else_branch {
                collect_bindings_matching_stmt(eb, pred, out);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            collect_bindings_matching_stmt(body, pred, out)
        }
        Stmt::For { init, body, .. } => {
            if let Some(init) = init {
                collect_bindings_matching_stmt(init, pred, out);
            }
            collect_bindings_matching_stmt(body, pred, out);
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => collect_bindings_matching(stmts, pred, out),
        Stmt::Switch { cases, default, .. } => {
            for c in cases {
                collect_bindings_matching(&c.body, pred, out);
            }
            if let Some(d) = default {
                collect_bindings_matching(d, pred, out);
            }
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            collect_bindings_matching(body, pred, out);
            collect_bindings_matching(catch_body, pred, out);
            if let Some(fb) = finally_body {
                collect_bindings_matching(fb, pred, out);
            }
        }
        _ => {}
    }
}
