//! The names a statement list declares — the half of the Annex B
//! eligibility question that is about the LIST, not about one
//! declaration.
//!
//! [`super::annexb_fn_var`] answers "what does §B.3.3 say about this
//! declaration"; everything here answers "which names does this list
//! already bind, and where". Split out when the parent reached the
//! 500-line file limit; the seam is the one its own doc draws.

use std::collections::HashSet;

use super::{Param, Stmt};

/// The names a statement list declares with a `function` of its own.
/// `Multi` is transparent — its children share the surrounding scope,
/// so a declaration inside one is this list's.
pub(super) fn own_fn_names(body: &[Stmt]) -> HashSet<String> {
    let mut out = HashSet::new();
    own_fn_names_into(body, &mut out);
    out
}

fn own_fn_names_into(body: &[Stmt], out: &mut HashSet<String>) {
    for st in body {
        match st {
            Stmt::FnDecl { name, .. } => {
                out.insert(name.clone());
            }
            Stmt::Multi(inner) => own_fn_names_into(inner, out),
            _ => {}
        }
    }
}

/// The names a statement list's BLOCK-SHAPED children declare with a
/// `function`. Intersected with the list's own declarations it names
/// the collision §B.3.3 cannot otherwise answer: the body-level
/// declaration is the one the var binding should hold before the block
/// runs, and this pass lifts and renames it away.
pub(super) fn block_nested_fn_names(body: &[Stmt]) -> HashSet<String> {
    let mut out = HashSet::new();
    nested_fn_names_of_list(body, &mut out);
    out
}

/// A statement list that shares its enclosing scope: its OWN
/// declarations are not nested, only what its block-shaped members
/// hold.
fn nested_fn_names_of_list(body: &[Stmt], out: &mut HashSet<String>) {
    for st in body {
        if matches!(st, Stmt::FnDecl { .. }) {
            continue;
        }
        nested_fn_names_of_stmt(st, out);
    }
}

fn nested_fn_names_of_stmt(stmt: &Stmt, out: &mut HashSet<String>) {
    match stmt {
        // A declaration reached AS a child is nested by construction —
        // the bare-branch form `if (c) function f() {}`.
        Stmt::FnDecl { name, .. } => {
            out.insert(name.clone());
        }
        Stmt::Block(b) => {
            for st in b {
                nested_fn_names_of_stmt(st, out);
            }
        }
        // Transparent — see `nested_fn_names_of_list`.
        Stmt::Multi(b) => nested_fn_names_of_list(b, out),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            nested_fn_names_of_stmt(then_branch, out);
            if let Some(eb) = else_branch {
                nested_fn_names_of_stmt(eb, out);
            }
        }
        Stmt::While { body, .. }
        | Stmt::DoWhile { body, .. }
        | Stmt::For { body, .. }
        | Stmt::ForOf { body, .. }
        | Stmt::ForOfSplitIter { body, .. }
        | Stmt::Labeled { body, .. } => nested_fn_names_of_stmt(body, out),
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            for st in body
                .iter()
                .chain(catch_body)
                .chain(finally_body.iter().flatten())
            {
                nested_fn_names_of_stmt(st, out);
            }
        }
        Stmt::Switch { cases, default, .. } => {
            for st in cases
                .iter()
                .flat_map(|c| c.body.iter())
                .chain(default.iter().flatten())
            {
                nested_fn_names_of_stmt(st, out);
            }
        }
        _ => {}
    }
}

/// A function's parameter names. §B.3.3.1 step 1.a.iii skips Annex B
/// when the name is one of them: the parameter already binds it, and
/// the tests say the binding keeps its argument across the
/// declaration.
pub(super) fn param_names(params: &[Param]) -> HashSet<String> {
    params.iter().map(|p| p.name.clone()).collect()
}

/// The names a statement list declares lexically. A `class` is already
/// a `TypeDecl` plus declarations by the time this pass runs, and a
/// `function` is the thing §B.3.3 is about rather than a bar to it, so
/// `let` / `const` are all that is left. `Multi` shares the surrounding
/// scope, so its children count as this list's.
pub(super) fn lex_names_into(body: &[Stmt], out: &mut Vec<String>) {
    for st in body {
        match st {
            Stmt::LetDecl {
                name,
                is_var: false,
                ..
            } => out.push(name.clone()),
            Stmt::Multi(inner) => lex_names_into(inner, out),
            _ => {}
        }
    }
}

/// The span of a `function <name>` declaration the list gives the
/// §B.3.3 shape. Unlike [`block_nested_fn_names`] the list's OWN
/// declarations count: the only caller passes a catch block, which is
/// a block, so a declaration written directly in it is nested already.
pub(super) fn annexb_fn_span(body: &[Stmt], name: &str) -> Option<crate::lexer::Span> {
    body.iter().find_map(|st| annexb_fn_span_of_stmt(st, name))
}

fn annexb_fn_span_of_stmt(stmt: &Stmt, name: &str) -> Option<crate::lexer::Span> {
    match stmt {
        Stmt::FnDecl { name: n, span, .. } if n == name => Some(*span),
        Stmt::Block(b) | Stmt::Multi(b) => annexb_fn_span(b, name),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => annexb_fn_span_of_stmt(then_branch, name).or_else(|| {
            else_branch
                .as_deref()
                .and_then(|eb| annexb_fn_span_of_stmt(eb, name))
        }),
        Stmt::While { body, .. }
        | Stmt::DoWhile { body, .. }
        | Stmt::For { body, .. }
        | Stmt::ForOf { body, .. }
        | Stmt::ForOfSplitIter { body, .. }
        | Stmt::Labeled { body, .. } => annexb_fn_span_of_stmt(body, name),
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => annexb_fn_span(body, name)
            .or_else(|| annexb_fn_span(catch_body, name))
            .or_else(|| {
                finally_body
                    .as_deref()
                    .and_then(|fb| annexb_fn_span(fb, name))
            }),
        Stmt::Switch { cases, default, .. } => cases
            .iter()
            .find_map(|c| annexb_fn_span(&c.body, name))
            .or_else(|| default.as_deref().and_then(|d| annexb_fn_span(d, name))),
        _ => None,
    }
}
