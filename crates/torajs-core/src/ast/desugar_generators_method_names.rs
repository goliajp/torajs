//! A generator body's binding that is spelled like one of the machine's
//! own methods.
//!
//! Params, `var`s and lifted `let`s become fields of the `__Gen_*`
//! machine, and a field is an own property: a param or `var` named
//! `next` shadows the machine's `next` method for every reader that
//! looks it up by name. The async-function driver does, so
//! `async function run(next) { await 0 }` threw "value is not a
//! function". The binding is renamed before the lift; the name it had
//! is the program's, the field it gets is the machine's.
//!
//! A lexical `let` / `const` of that name takes the J.2.b rename instead
//! (the name is reserved in the lift, `desugar_generators_prep`).

use std::collections::HashMap;

use super::{Ast, Param, Stmt};

/// The machine's public methods a binding may be spelled like (`return`
/// and `throw` are reserved words, never a binding).
pub(super) const MACHINE_METHODS: &[&str] = &["next"];

/// Rename every param and `var` of the body named like a machine method,
/// together with its references. A body that also declares the name
/// lexically somewhere keeps it: those references are the inner
/// binding's, which the J.2.b rename handles per range.
pub(super) fn free_method_names(ast: &mut Ast, params: &mut [Param], body: &mut [Stmt]) {
    for &m in MACHINE_METHODS {
        let as_param = params.iter().any(|p| p.name == m);
        let vars = super::desugar_generators_walkers::var_slots(ast, body);
        if !(as_param || vars.contains_key(m)) || body.iter().any(|s| lexical_in(s, m)) {
            continue;
        }
        let fresh = format!("{m}__gm");
        let renames = HashMap::from([(m.to_string(), fresh.clone())]);
        super::nested_fns_idents::rewrite_idents_in_body(ast, body, &renames, true);
        for p in params.iter_mut().filter(|p| p.name == m) {
            p.name = fresh.clone();
        }
        for s in body.iter_mut() {
            rename_var(s, m, &fresh);
        }
    }
}

/// A declaration of `name` anywhere in `s` outside nested functions
/// that is not the function's `var` (a block `let` / `const`, a function
/// or class, a loop variable, a `catch` parameter).
fn lexical_in(s: &Stmt, name: &str) -> bool {
    let here = match s {
        Stmt::LetDecl {
            name: n, is_var, ..
        } => !is_var && n == name,
        Stmt::FnDecl { name: n, .. } | Stmt::ClassDecl { name: n, .. } => n == name,
        Stmt::ForOf { var_name, .. } => var_name == name,
        Stmt::Try { catch_param, .. } => catch_param.as_deref() == Some(name),
        _ => false,
    };
    here || (super::desugar_with::walk::stmt_children_ref(s).into_iter())
        .any(|c| lexical_in(c, name))
}

fn rename_var(s: &mut Stmt, from: &str, to: &str) {
    if let Stmt::LetDecl {
        name, is_var: true, ..
    } = s
        && name == from
    {
        *name = to.to_string();
    }
    for c in super::desugar_with::walk::stmt_children(s) {
        rename_var(c, from, to);
    }
}
