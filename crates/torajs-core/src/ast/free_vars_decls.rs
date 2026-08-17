//! Declaration-body scopes for the free-vars walk — the parent walks
//! statements and expressions in place; this sibling answers what a
//! whole declaration BODY (a fn's or a class's own scope) leaves
//! free in the enclosing scope.

use super::free_vars::{hoist_fn_decl_names, walk_expr, walk_stmt};
use super::{Ast, ClassCtor, ClassMethod, ExprId, Param, StaticInit, Stmt};

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
    bound: &mut Vec<String>,
    out: &mut Vec<String>,
) {
    let saved = bound.len();
    for p in params {
        bound.push(p.name.clone());
    }
    bound.push("arguments".into());
    bound.push("__this".into());
    hoist_fn_decl_names(body, bound);
    for s in body {
        walk_stmt(ast, s, bound, out);
    }
    bound.truncate(saved);
}

/// A class declaration's body walk. The module resolver's hidden-
/// dependency census walks PRE-desugar statements, where class bodies
/// still exist (the old "already split into FnDecls" assumption only
/// holds for the arrow-lift caller, which runs after desugar_classes
/// — a body here reports its REAL frees, strictly more correct than
/// ignoring it). Each callable body is a fn scope; instance-field
/// inits already live in the ctor body
/// (`finalize_class_field_inits`).
#[allow(clippy::too_many_arguments)]
pub(super) fn walk_class_decl(
    ast: &Ast,
    parent: &Option<ExprId>,
    ctor: &Option<ClassCtor>,
    methods: &[ClassMethod],
    static_methods: &[ClassMethod],
    static_init: &[StaticInit],
    bound: &mut Vec<String>,
    out: &mut Vec<String>,
) {
    if let Some(pid) = parent {
        walk_expr(ast, *pid, bound, out);
    }
    if let Some(c) = ctor {
        walk_fn_decl(ast, &c.params, &c.body, bound, out);
    }
    for m in methods.iter().chain(static_methods.iter()) {
        walk_fn_decl(ast, &m.params, &m.body, bound, out);
    }
    for si in static_init {
        match si {
            StaticInit::Field(f) => walk_expr(ast, f.init, bound, out),
            StaticInit::Block(v) => {
                let saved = bound.len();
                hoist_fn_decl_names(v, bound);
                for s in v {
                    walk_stmt(ast, s, bound, out);
                }
                bound.truncate(saved);
            }
        }
    }
}
