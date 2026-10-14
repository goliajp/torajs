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

use super::{Ast, Stmt};

/// The names a list binds for its whole extent, before any of its
/// statements run — bound before walking the list. A function
/// declaration hoists: a reference anywhere in the holding list,
/// including before the declaration, resolves to the local decl. A
/// lexical declaration (`let` / `const` / `using` / `class`) is in
/// scope from the block's start too (§14.3.1): a reference before it
/// is the temporal dead zone of THIS binding, never an outer name —
/// `(async function () { console.log(w); await null; let w = 1 })`
/// read as free captured a `w` the enclosing scope never bound
/// (`unknown identifier`) where bun raises the ReferenceError. `var`
/// hoists further and is [`hoist_var_names`]'s.
pub(super) fn hoist_list_names(list: &[Stmt], bound: &mut Vec<String>) {
    for s in list {
        let name = match s {
            Stmt::FnDecl { name, .. } | Stmt::ClassDecl { name, .. } => name,
            Stmt::LetDecl {
                name,
                is_var: false,
                ..
            }
            | Stmt::UsingDecl { name, .. } => name,
            // `desugar_using`'s wrapper holds the rest of THIS list
            // (`try { … } catch (__usc_N)`), in the scope it was
            // written in — its declarations belong to this list.
            Stmt::Try {
                body,
                catch_param: Some(p),
                ..
            } if p.starts_with("__usc_") => {
                hoist_list_names(body, bound);
                continue;
            }
            // a compiler-minted group shares the list's scope (a dead-zone
            // flag declared beside the statement it guards)
            Stmt::Multi(inner) => {
                hoist_list_names(inner, bound);
                continue;
            }
            _ => continue,
        };
        if !bound.contains(name) {
            bound.push(name.clone());
        }
    }
}

/// §B.3.3 gives a block-nested `function f` a var-scoped binding on
/// top of its block-scoped one, in sloppy code. That var binding is a
/// local of THIS body, so every reference to the name in the body —
/// before the block, after it, or from a closure the body mints —
/// means the local, never an outer binding of the same name. The
/// names hoist for the same reason `var` does, so they hoist here.
///
/// Missing it, the arrow lift captured the outer binding, and the
/// Annex B pass then declared the var binding into the very scope the
/// captured env parameter already bound: `var f; (function () { f =
/// 123; { function f() {} } })()` answered "redeclaration of `f` in
/// current scope" where node prints the function. The read-only shape
/// hid it — with nothing writing `f` there was no outer binding to
/// capture in the first place.
pub(super) fn hoist_annexb_fn_names(ast: &Ast, list: &[Stmt], bound: &mut Vec<String>) {
    if !ast.program_is_sloppy() {
        return;
    }
    let mut names: Vec<String> = super::annexb_names::block_nested_fn_names(list)
        .into_iter()
        .collect();
    names.sort();
    for name in names {
        if bound.contains(&name) {
            continue;
        }
        if super::annexb_names::annexb_fn_span(list, &name)
            .is_some_and(|sp| super::annexb_fn_var::annexb_applies_at(ast, &name, sp))
        {
            bound.push(name);
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
