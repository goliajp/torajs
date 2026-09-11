//! Every name the program binds anywhere — what
//! [`super::hoist_gen_fn_exprs_wrapper`] asks before it calls a free
//! variable of a generator expression a capture (rotation 635).
//!
//! The free-variable walk is name-only: a name the body reads that
//! neither its params nor the module's top-level bindings supply is
//! reported as a capture, whether or not any enclosing scope binds
//! it. For a written closure the checker settles that later, at the
//! construction site, where the enclosing scopes are in view
//! (§6.2.5.5 — a capture that resolves nowhere takes the
//! undeclared-read lane). A generator expression is hoisted before
//! any scope exists, so the hoist has to settle it here: a name no
//! scope in the whole program binds cannot be an enclosing local. It
//! is an implicit global or an undeclared reference — exactly what
//! the hoisted body's own global / undeclared lanes answer, and what
//! the declaration form (lifted, never routed through captures)
//! already does with it. `yield = 1` inside a sloppy nested function
//! of a generator expression is the test262 shape.
//!
//! Binding forms covered: let / const / var / using, function names
//! and params (declarations, arrows, class ctors and methods), class
//! names, catch params, for-of heads, import bindings, and every
//! fn-expression self-name (bound in its own function env, declared
//! nowhere). Static blocks and nested bodies are entered; missing a
//! form here can only turn a compile-time refusal into a runtime
//! ReferenceError, never into a silent answer.

use std::collections::HashSet;

use super::desugar_with::walk::stmt_children_ref;
use super::{Ast, Expr, Param, StaticInit, Stmt};

pub(super) fn bound_names_anywhere(ast: &Ast) -> HashSet<String> {
    let mut out: HashSet<String> = HashSet::new();
    for s in &ast.stmts {
        collect_stmt(s, &mut out);
    }
    // Arrow bodies hang off arena slots the statement spine never
    // reaches; a flat scan covers every nesting depth.
    for e in &ast.exprs {
        if let Expr::ArrowFn { params, body, .. } = e {
            push_params(params, &mut out);
            for s in body {
                collect_stmt(s, &mut out);
            }
        }
    }
    out.extend(ast.fn_expr_self_names.values().cloned());
    out
}

fn push_params(params: &[Param], out: &mut HashSet<String>) {
    for p in params {
        out.insert(p.name.clone());
    }
}

fn collect_list(list: &[Stmt], out: &mut HashSet<String>) {
    for s in list {
        collect_stmt(s, out);
    }
}

fn collect_stmt(s: &Stmt, out: &mut HashSet<String>) {
    match s {
        Stmt::LetDecl { name, .. } | Stmt::UsingDecl { name, .. } => {
            out.insert(name.clone());
        }
        Stmt::ForOf { var_name, .. } | Stmt::ForOfSplitIter { var_name, .. } => {
            out.insert(var_name.clone());
        }
        Stmt::Try {
            catch_param: Some(c),
            ..
        } => {
            out.insert(c.clone());
        }
        Stmt::FnDecl {
            name, params, body, ..
        } => {
            out.insert(name.clone());
            push_params(params, out);
            collect_list(body, out);
        }
        Stmt::ClassDecl {
            name,
            ctor,
            methods,
            static_methods,
            static_init,
            ..
        } => {
            out.insert(name.clone());
            if let Some(c) = ctor {
                push_params(&c.params, out);
                collect_list(&c.body, out);
            }
            for m in methods.iter().chain(static_methods.iter()) {
                push_params(&m.params, out);
                collect_list(&m.body, out);
            }
            for si in static_init {
                if let StaticInit::Block(b) = si {
                    collect_list(b, out);
                }
            }
        }
        Stmt::ImportDecl {
            default,
            namespace,
            named,
            ..
        } => {
            out.extend(default.iter().cloned());
            out.extend(namespace.iter().cloned());
            for (imported, local) in named {
                out.insert(imported.clone());
                out.extend(local.iter().cloned());
            }
        }
        _ => {}
    }
    // The read-only walk stops at nested fn / class bodies, which the
    // arms above enter on purpose.
    for c in stmt_children_ref(s) {
        collect_stmt(c, out);
    }
}
