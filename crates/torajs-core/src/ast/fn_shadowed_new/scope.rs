//! The names each kind of scope binds, as the census reads them.

use super::super::Stmt;
use super::super::annexb_names::own_fn_names;
use super::super::desugar_with::walk::stmt_children_ref;
use super::Scope;
use std::collections::HashSet;

pub(super) fn shadowing(names: HashSet<String>) -> Scope {
    names.into_iter().map(|n| (n, true)).collect()
}

/// The names a statement list declares with a `class` of its own
/// (`Multi` shares the list's scope). They resolve to the class.
pub(super) fn add_class_names(stmts: &[Stmt], scope: &mut Scope) {
    for s in stmts {
        match s {
            Stmt::ClassDecl { name, .. } => {
                scope.entry(name.clone()).or_insert(false);
            }
            Stmt::Multi(inner) => add_class_names(inner, scope),
            _ => {}
        }
    }
}

/// The `let` / `const` names a statement list binds DIRECTLY. `Multi`
/// shares the surrounding scope, so its entries belong to this list;
/// a nested `Block` gets its own scope and is not descended into here.
pub(super) fn let_const_names(stmts: &[Stmt]) -> HashSet<String> {
    let mut out = HashSet::new();
    for s in stmts {
        match s {
            Stmt::LetDecl {
                name,
                is_var: false,
                ..
            }
            | Stmt::UsingDecl { name, .. } => {
                out.insert(name.clone());
            }
            Stmt::Multi(inner) => out.extend(let_const_names(inner)),
            _ => {}
        }
    }
    out
}

/// Every `var` the body binds, at any depth — `var` is function-scoped,
/// so a `var W` inside a nested block shadows the whole body and not
/// just that block. The walk stops at a nested function: its `var`s
/// are its own.
pub(super) fn var_names(stmts: &[Stmt], out: &mut HashSet<String>) {
    for s in stmts {
        if let Stmt::LetDecl {
            name, is_var: true, ..
        } = s
        {
            out.insert(name.clone());
        }
        if matches!(s, Stmt::FnDecl { .. }) {
            continue;
        }
        for c in stmt_children_ref(s) {
            var_names(std::slice::from_ref(c), out);
        }
    }
}

/// The lexical names a block-shaped list binds: its `let` / `const`,
/// its own `function`s (a block's own in strict code, and its block
/// binding under §B.3.3 too) and its classes.
pub(super) fn block_scope(list: &[Stmt]) -> Scope {
    let mut names = let_const_names(list);
    names.extend(own_fn_names(list));
    let mut scope = shadowing(names);
    add_class_names(list, &mut scope);
    scope
}
