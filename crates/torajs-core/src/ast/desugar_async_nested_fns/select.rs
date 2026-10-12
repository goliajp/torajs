//! What becomes of each of a body's direct declarations.
//!
//! The answer is per connected component of the peer-reference
//! relation (a declaration naming another of the same list): a value
//! is a local `let`, which a peer lifted to top level could not name.
//! A component with a member that cannot be a value (`is_rewritable`)
//! keeps every member a declaration.
//!
//! In an async body every other component becomes values, so the moved
//! body declares nothing (blade 2 refuses one that does). In a
//! generator body only a component that reads the frames around it
//! does; one that reads nothing of them is lifted to top level here,
//! before the machine splits the body into states — left in the body it
//! is declared in the first state only, out of reach of a call after a
//! `yield` — and stays a direct call (`nested_fns_capture` draws the
//! same line in an ordinary body).

use crate::ast::free_vars::free_vars_of_fn;
use crate::ast::scope_chain::not_rebound;
use crate::ast::{Ast, Stmt};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Route {
    /// `let f = function …`, capturing the machine frame.
    Value,
    /// A top-level declaration under a mangled name.
    Lift,
    /// Left as it is.
    Keep,
}

/// For each direct `function` declaration of `body` (in order), its
/// route. `capture_frames` is `None` for an async body; for a generator
/// body it holds every name the frames around a direct declaration bind
/// (the generator's own included), and `globals` the top-level
/// function names.
pub(super) fn select(
    ast: &Ast,
    body: &[Stmt],
    globals: &[String],
    capture_frames: Option<&[String]>,
) -> Vec<Route> {
    let decls: Vec<&Stmt> = body
        .iter()
        .filter(|s| matches!(s, Stmt::FnDecl { .. }))
        .collect();
    let names: Vec<&str> = decls
        .iter()
        .map(|s| match s {
            Stmt::FnDecl { name, .. } => name.as_str(),
            _ => unreachable!("filtered to declarations"),
        })
        .collect();
    let rebind = capture_frames.unwrap_or(&[]);
    let free: Vec<Vec<String>> = decls
        .iter()
        .map(|s| match s {
            Stmt::FnDecl { params, body, .. } => {
                let mut pre: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
                pre.extend(not_rebound(globals, rebind));
                free_vars_of_fn(ast, &pre, &[], params, body)
            }
            _ => unreachable!("filtered to declarations"),
        })
        .collect();
    let component = components(&names, &free);
    let mut captures = vec![capture_frames.is_none(); decls.len()];
    let mut stuck = vec![false; decls.len()];
    let mut special = vec![false; decls.len()];
    for (k, s) in decls.iter().enumerate() {
        if free[k]
            .iter()
            .any(|n| n != "arguments" && !names.contains(&n.as_str()))
        {
            captures[component[k]] = true;
        }
        if !super::is_rewritable(ast, s) {
            stuck[component[k]] = true;
        }
        if is_special(ast, s) {
            special[component[k]] = true;
        }
    }
    (0..decls.len())
        .map(|k| {
            let c = component[k];
            if captures[c] && !stuck[c] {
                Route::Value
            } else if capture_frames.is_some() && !captures[c] && !special[c] {
                Route::Lift
            } else {
                Route::Keep
            }
        })
        .collect()
}

/// A generator or async declaration has lanes of its own, keyed by its
/// name; only a plain function is lifted here.
fn is_special(ast: &Ast, s: &Stmt) -> bool {
    let Stmt::FnDecl {
        name, is_generator, ..
    } = s
    else {
        unreachable!("filtered to declarations");
    };
    *is_generator || ast.async_fns.contains(name) || ast.async_generator_fns.contains(name)
}

/// Component label per declaration: the smallest index joined to it by
/// a chain of reference edges, in either direction.
fn components(names: &[&str], free: &[Vec<String>]) -> Vec<usize> {
    let mut label: Vec<usize> = (0..names.len()).collect();
    loop {
        let mut changed = false;
        for a in 0..names.len() {
            for b in 0..names.len() {
                let edge = free[a].iter().any(|n| n == names[b]);
                if edge && label[a] != label[b] {
                    let m = label[a].min(label[b]);
                    label[a] = m;
                    label[b] = m;
                    changed = true;
                }
            }
        }
        if !changed {
            return label;
        }
    }
}
