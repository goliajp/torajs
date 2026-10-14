//! A class declared inside a generator body: what of it reads the
//! generator's lifted locals, and through which base.

use super::{ParamMap, RefBase, rewrite_params_in_expr, rewrite_params_in_stmt};
use crate::ast::{Ast, Param, Stmt};

/// Rewrite the lifted-local reads of the class `s`.
///
/// Its computed member keys (and computed static-field inits) live in
/// the `class_computed_keys` / `class_computed_static_fields` side
/// tables, not on the stmt tree — a lifted-local read inside the
/// brackets (`[k]`, a hoisted `__yx_N` yield temp most of all) kept its
/// old name while the binding became a state-machine field (the
/// cpn-class-*-yield t262 family). They evaluate in the ENCLOSING scope
/// per §15.7.14, so the lifted-field mapping applies verbatim.
///
/// The member bodies read a lifted local through the `__gself` alias
/// (`rewrite_member_body`).
pub(super) fn rewrite_class(
    ast: &mut Ast,
    s: &Stmt,
    pset: &ParamMap,
    visited: &mut std::collections::HashSet<u32>,
) {
    let Stmt::ClassDecl {
        name,
        ctor,
        methods,
        static_methods,
        static_init,
        ..
    } = s
    else {
        return;
    };
    let keys: Vec<crate::ast::ExprId> = ast
        .class_computed_keys
        .iter()
        .filter(|((c, _), _)| c == name)
        .map(|(_, &e)| e)
        .chain(
            ast.class_computed_static_fields
                .iter()
                .filter(|(c, _, _)| c == name)
                .map(|(_, _, init)| *init),
        )
        .collect();
    for k in keys {
        rewrite_params_in_expr(ast, k, pset, visited);
    }
    // inside its body the class name is the class's own binding
    // (§15.7.14 step 4), not the generator field a generator-local
    // class value is also kept in
    let inner = member_map(pset, |k| k == name);
    let members = ctor.iter().map(|c| (&c.params, &c.body)).chain(
        methods
            .iter()
            .chain(static_methods)
            .map(|m| (&m.params, &m.body)),
    );
    for (params, body) in members {
        rewrite_member_body(ast, params, body, &inner, visited);
    }
    for init in static_init {
        match init {
            crate::ast::StaticInit::Field(f) => {
                rewrite_params_in_expr(ast, f.init, &inner, visited)
            }
            crate::ast::StaticInit::Block(body) => {
                rewrite_member_body(ast, &[], body, &inner, visited)
            }
        }
    }
}

/// A class member's body and parameter defaults (§15.7.14 evaluates
/// its methods, constructor and static initializers inside the class's
/// scope, which encloses the generator's locals). Its `this` is the
/// instance or the class, never the generator, so a lifted local is
/// read through the `__gself` alias as in a function expression, minus
/// the names the member rebinds itself.
fn rewrite_member_body(
    ast: &mut Ast,
    params: &[Param],
    body: &[Stmt],
    pset: &ParamMap,
    visited: &mut std::collections::HashSet<u32>,
) {
    // §10.2.11: a parameter default sees the parameters, not the body
    let outer = member_map(pset, |k| params.iter().any(|p| p.name == k));
    if !outer.fields.is_empty() {
        for d in params.iter().filter_map(|p| p.default) {
            rewrite_params_in_expr(ast, d, &outer, visited);
        }
    }
    let sub = member_map(pset, |k| {
        crate::ast::nested_fns_idents::arrow_rebinds(params, body, k)
    });
    if !sub.fields.is_empty() {
        for s in body {
            rewrite_params_in_stmt(ast, s, &sub, visited);
        }
    }
}

fn member_map(pset: &ParamMap, rebinds: impl Fn(&str) -> bool) -> ParamMap {
    ParamMap {
        fields: pset
            .fields
            .iter()
            .filter(|(k, _)| !rebinds(k))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
        base: match &pset.base {
            RefBase::This => RefBase::SelfAlias,
            b => b.clone(),
        },
    }
}
