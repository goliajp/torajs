//! What becomes of each of a body's direct declarations.
//!
//! Routes follow the peer-reference edges (a declaration naming another
//! of the same list), each constraint along its own direction:
//!
//! - a value is a local `let`, which a declaration kept or lifted to
//!   top level cannot name: a declaration naming a value must be one
//!   too (needing a value spreads from callee to caller)
//! - a declaration that cannot be a value (`is_rewritable`) keeps every
//!   peer it names a declaration as well (spreads from caller to
//!   callee); a value naming it is unaffected, as the name it reads is
//!   a top-level one once the declaration is lifted
//! - a declaration left in the body is lifted later under a name of its
//!   own lane: a peer naming it is not lifted here either
//!
//! In an async body every declaration needs a value, so the moved body
//! declares nothing (blade 2 refuses one that does). In a generator
//! body only one that reads the frames around it (or names one that
//! does) needs it; the rest are lifted to top level here, before the
//! machine splits the body into states — left in the body a declaration
//! is declared in the first state only, out of reach of a call after a
//! `yield` — and stay direct calls (`nested_fns_capture` draws the same
//! line in an ordinary body).

use crate::ast::free_vars::free_vars_of_fn;
use crate::ast::scope_chain::not_rebound;
use crate::ast::{Ast, Stmt};
use std::collections::HashSet;

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
/// function names. A declaration in `var_shared` shares its binding
/// with a written `var` (`nested_fns_top::var_shared_fn_names`): the
/// binding has to be a value the body writes, so it needs one too, and
/// a declaration naming it reads a local of the frame.
pub(super) fn select(
    ast: &Ast,
    body: &[Stmt],
    globals: &[String],
    capture_frames: Option<&[String]>,
    var_shared: &HashSet<String>,
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
    let names_peer = |a: usize, b: usize| free[a].iter().any(|n| n == names[b]);
    let mut needs: Vec<bool> = free
        .iter()
        .zip(&names)
        .map(|(f, name)| {
            capture_frames.is_none()
                || var_shared.contains(*name)
                || f.iter().any(|n| {
                    n != "arguments" && (!names.contains(&n.as_str()) || var_shared.contains(n))
                })
        })
        .collect();
    spread(&mut needs, names_peer);
    let mut refuses: Vec<bool> = decls
        .iter()
        .map(|s| !super::is_rewritable(ast, s))
        .collect();
    spread(&mut refuses, |a, b| names_peer(b, a));
    let mut stays: Vec<bool> = (0..decls.len())
        .map(|k| needs[k] || is_special(ast, decls[k]))
        .collect();
    spread(&mut stays, names_peer);
    (0..decls.len())
        .map(|k| {
            if needs[k] && !refuses[k] {
                Route::Value
            } else if capture_frames.is_some() && !stays[k] {
                Route::Lift
            } else {
                Route::Keep
            }
        })
        .collect()
}

/// Close `mark` over `reaches`: `a` becomes marked when `reaches(a, b)`
/// holds for a marked `b`.
fn spread(mark: &mut [bool], reaches: impl Fn(usize, usize) -> bool) {
    loop {
        let mut changed = false;
        for a in 0..mark.len() {
            if !mark[a] && (0..mark.len()).any(|b| mark[b] && reaches(a, b)) {
                mark[a] = true;
                changed = true;
            }
        }
        if !changed {
            return;
        }
    }
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
