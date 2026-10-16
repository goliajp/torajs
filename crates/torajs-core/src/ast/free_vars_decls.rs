//! Declaration-body scopes for the free-vars walk — the parent walks
//! statements and expressions in place; this sibling answers what a
//! whole declaration BODY (a fn's or a class's own scope) leaves
//! free in the enclosing scope.

use super::class_computed_owner::owns;
use super::free_vars::walk_stmt;
use super::free_vars::{Bound, FreeSink};
use super::free_vars_expr::walk_expr;
use super::free_vars_hoisted_names::{hoist_list_names, hoist_var_names};
use super::hoist_gen_fn_exprs_self_write::ctor_field_inits;
use super::{Ast, ClassCtor, ClassMethod, Expr, ExprId, Param, StaticInit, Stmt};

/// A nested `function` declaration's body walk. The decl's own name is
/// hoist-bound by the enclosing list walk. Its body is a scope of its
/// own: params and the decl's `arguments` quasi-binding bind, and
/// whatever stays free inside is free in the enclosing scope too — a
/// nested decl reading an outer local makes the enclosing closure
/// capture it. Ignoring the body (the pre-fix behavior) both reported
/// the decl's NAME as a phantom capture and hid its real ones.
///
/// `__this` binds too: a `function` declaration binds its own `this`
/// (§10.2.1.1 non-lexical [[ThisMode]]), so the body's `__this` — the
/// desugar_classes pass-2 spelling — is NOT free in the enclosing
/// scope. Without this bound entry an enclosing lifted closure carries
/// a phantom `__this` capture its definition scope cannot supply, and
/// the checker rejects the whole closure (unknown identifier) even
/// though the nested decl itself promotes fine.
pub(super) fn walk_fn_decl(
    ast: &Ast,
    params: &[Param],
    body: &[Stmt],
    bound: &mut Bound<'_>,
    out: &mut dyn FreeSink,
) {
    let saved = bound.names.len();
    for p in params {
        bound.names.push(p.name.clone());
    }
    bound.names.push("arguments".into());
    bound.names.push("__this".into());
    walk_param_defaults(ast, params, bound, out);
    hoist_list_names(body, &mut bound.names);
    hoist_var_names(body, &mut bound.names);
    for s in body {
        walk_stmt(ast, s, bound, out);
    }
    bound.names.truncate(saved);
}

/// A class declaration's body walk. The module resolver's hidden-
/// dependency census walks PRE-desugar statements, where class bodies
/// still exist (the old "already split into FnDecls" assumption only
/// holds for the arrow-lift caller, which runs after desugar_classes
/// — a body here reports its REAL frees, strictly more correct than
/// ignoring it). Each callable body is a fn scope; instance-field
/// inits already live in the ctor body
/// (`finalize_class_field_inits`), see [`walk_ctor`].
///
/// The computed keys and computed static-field initializers live in
/// side tables keyed by the class name, and a name is not an identity
/// (two functions may each declare `class K`), so a row counts only
/// when `decl_id` owns its sentinel. The keys are walked in arena
/// order. A static initializer's `this` is the class, so it binds
/// `__this` like a member does. The class's own name is the
/// caller's: a walk from the list holding the class binds it, and a
/// walk of the declaration alone reports its self-references.
#[allow(clippy::too_many_arguments)]
pub(super) fn walk_class_decl(
    ast: &Ast,
    name: &str,
    decl_id: u32,
    parent: &Option<ExprId>,
    ctor: &Option<ClassCtor>,
    methods: &[ClassMethod],
    static_methods: &[ClassMethod],
    static_init: &[StaticInit],
    bound: &mut Bound<'_>,
    out: &mut dyn FreeSink,
) {
    let saved = bound.names.len();
    if let Some(pid) = parent {
        walk_expr(ast, *pid, bound, out);
    }
    let mut keys: Vec<ExprId> = ast
        .class_computed_keys
        .iter()
        .filter(|((c, sent), _)| c == name && owns(ast, sent, decl_id))
        .map(|(_, &e)| e)
        .collect();
    keys.sort_by_key(|e| e.0);
    for k in keys {
        walk_expr(ast, k, bound, out);
    }
    if let Some(c) = ctor {
        walk_ctor(ast, name, c, bound, out);
    }
    for m in methods.iter().chain(static_methods.iter()) {
        walk_fn_decl(ast, &m.params, &m.body, bound, out);
    }
    bound.names.push("__this".into());
    for si in static_init {
        match si {
            StaticInit::Field(f) => walk_expr(ast, f.init, bound, out),
            StaticInit::Block(v) => {
                let inner = bound.names.len();
                hoist_list_names(v, &mut bound.names);
                hoist_var_names(v, &mut bound.names);
                for s in v {
                    walk_stmt(ast, s, bound, out);
                }
                bound.names.truncate(inner);
            }
        }
    }
    for (c, sent, init) in &ast.class_computed_static_fields {
        if c == name && owns(ast, sent, decl_id) {
            walk_expr(ast, *init, bound, out);
        }
    }
    bound.names.truncate(saved);
}

/// The parameter defaults of one function, walked in its own frame.
/// §10.2.11 evaluates a default in the function's parameter scope:
/// every parameter name is bound there (reading a later one is a
/// runtime TDZ error, never an outer reference) and so are the
/// function's own `arguments` and `this`, while the body's
/// declarations are not — so the caller walks these after binding the
/// params and before hoisting the body's names. A default is a read
/// of the scope the function was DEFINED in, which is what makes a
/// name in it a capture.
pub(super) fn walk_param_defaults(
    ast: &Ast,
    params: &[Param],
    bound: &mut Bound<'_>,
    out: &mut dyn FreeSink,
) {
    for d in params.iter().filter_map(|p| p.default) {
        // an optional `x?: T` slot's pad sentinel names no binding
        if matches!(ast.get_expr(d), Expr::Ident(n) if n.starts_with(super::UNDEF_SLOT_MARKER)) {
            continue;
        }
        walk_expr(ast, d, bound, out);
    }
}

/// A constructor body, with the instance field initializers the parser
/// spliced into it walked in the class scope (§15.7.10): the
/// constructor's parameters and declarations do not reach them, so a
/// name they share with the initializers is still free there. The
/// statements are walked in body order either way.
fn walk_ctor(ast: &Ast, class: &str, c: &ClassCtor, bound: &mut Bound<'_>, out: &mut dyn FreeSink) {
    let fields = ctor_field_inits(ast, class, &c.body);
    if fields.is_empty() {
        walk_fn_decl(ast, &c.params, &c.body, bound, out);
        return;
    }
    let saved = bound.names.len();
    for p in &c.params {
        bound.names.push(p.name.clone());
    }
    bound.names.push("arguments".into());
    bound.names.push("__this".into());
    walk_param_defaults(ast, &c.params, bound, out);
    hoist_list_names(&c.body, &mut bound.names);
    hoist_var_names(&c.body, &mut bound.names);
    for (i, s) in c.body.iter().enumerate() {
        if !fields.contains(&i) {
            walk_stmt(ast, s, bound, out);
            continue;
        }
        let frame = bound.names.split_off(saved);
        bound.names.push("__this".into());
        hoist_list_names(&c.body[fields.clone()], &mut bound.names);
        walk_stmt(ast, s, bound, out);
        bound.names.truncate(saved);
        bound.names.extend(frame);
    }
    bound.names.truncate(saved);
}
