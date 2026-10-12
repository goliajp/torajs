//! `var` declarations in a generator body.
//!
//! A `var` is one binding for the whole function (§14.3.2,
//! §10.2.11 VarDeclaredNames), however many times and in however
//! deeply nested a block it is written. In a generator every local
//! lifts to a field of the state-machine class, so each `var` name
//! becomes exactly one field: the first site to lift claims it, every
//! later site is an assignment onto it, and none is renamed — the
//! duplicate-`let` rename (J.2.b) is for block-scoped bindings only.
//! The binding is `undefined` until a site stores into it, and a
//! typed field has no `undefined` to start from (a `number` field
//! reads 0), so a `var` keeps its initializer's type only when nothing
//! can read it first: one site, written as an unconditional statement
//! of the body, no reference before it, and no function declaration
//! that reads it (one may be called before the site). Every other
//! `var` takes an `any` slot — the slot `desugar_var_hoist` gives
//! every `var` outside generators — which also holds the differently
//! shaped values several sites may store into the one binding.

use super::LiftCtx;
use crate::ast::free_vars::free_vars_of_body;
use crate::ast::{Ast, Expr, Stmt};
use std::collections::HashMap;

/// Every `var` name of the generator body, mapped to whether its slot
/// may take the initializer's type (see the module doc).
pub(crate) fn var_slots(ast: &Ast, body: &[Stmt]) -> HashMap<String, bool> {
    let mut sites: HashMap<String, usize> = HashMap::new();
    let mut fn_reads: Vec<String> = Vec::new();
    for s in body {
        census(ast, s, &mut sites, &mut fn_reads);
    }
    sites
        .into_iter()
        .map(|(name, n)| {
            let typed = n == 1
                && !fn_reads.contains(&name)
                && body
                    .iter()
                    .position(|s| unconditional_site(s, &name))
                    .is_some_and(|i| !free_vars_of_body(ast, &[], &body[..i]).contains(&name));
            (name, typed)
        })
        .collect()
}

/// Count `var` sites by name and collect what function declarations
/// read, not descending into a nested function or class — their
/// `var`s are their own.
fn census(ast: &Ast, s: &Stmt, sites: &mut HashMap<String, usize>, fn_reads: &mut Vec<String>) {
    match s {
        Stmt::LetDecl {
            name, is_var: true, ..
        } => *sites.entry(name.clone()).or_default() += 1,
        Stmt::FnDecl { .. } => {
            fn_reads.extend(free_vars_of_body(ast, &[], std::slice::from_ref(s)));
        }
        Stmt::ClassDecl { .. } => {}
        _ => {
            for c in crate::ast::desugar_with::walk::stmt_children_ref(s) {
                census(ast, c, sites, fn_reads);
            }
        }
    }
}

/// Whether the body statement `s` declares `name` every time it runs.
fn unconditional_site(s: &Stmt, name: &str) -> bool {
    match s {
        Stmt::LetDecl {
            name: n,
            is_var: true,
            ..
        } => n == name,
        Stmt::For { init: Some(i), .. } => unconditional_site(i, name),
        Stmt::Labeled { body, .. } => unconditional_site(body, name),
        Stmt::Multi(v) => v.iter().any(|s| unconditional_site(s, name)),
        _ => false,
    }
}

/// Lift the `var` at `s`: the first site of its name claims the field,
/// and the site itself becomes the store of its initializer — or
/// nothing, for an initializer-less `var x;`, which never resets a
/// binding that already holds a value.
pub(super) fn lift_var(
    ast: &mut Ast,
    s: &mut Stmt,
    lifted: &mut Vec<(String, String)>,
    ctx: &mut LiftCtx,
) {
    let Stmt::LetDecl {
        name,
        type_ann,
        init,
        ..
    } = s
    else {
        return;
    };
    let (name, init) = (name.clone(), *init);
    let claimed =
        lifted.iter().any(|(n, _)| *n == name) || ctx.params.iter().any(|p| p.name == name);
    if !claimed {
        let t = if ctx.vars.get(&name).is_some_and(|&typed| typed) {
            super::local_ann::local_ann(ast, &name, type_ann.as_ref(), init, ctx)
        } else {
            "any".to_string()
        };
        ctx.binds.insert(name.clone(), t.clone());
        lifted.push((name.clone(), t));
    }
    if matches!(ast.get_expr(init), Expr::Uninit) {
        *s = Stmt::Block(Vec::new());
        return;
    }
    let this_id = ast.add_expr(Expr::This);
    let target = ast.add_expr(Expr::Member { obj: this_id, name });
    *s = Stmt::Expr(ast.add_expr(Expr::Assign {
        target,
        value: init,
    }));
}
