//! Classes declared where a binding gets a per-pass local.
//!
//! A class declaration is not hoisted: §15.7.14 evaluates it when its
//! statement runs, and its members close over the environment of that
//! moment. So a class is hosted like an arrow: the local stands in
//! front of its declaration and the members read it. Member bodies are
//! statement lists the expression walks of the parent module do not
//! reach, hence the walks here.

use super::scan::{assigns_deep, collect_deep, collect_deep_stmt, stmt_roots};
use crate::ast::desugar_with::walk::{stmt_children, stmt_children_ref};
use crate::ast::{Ast, ExprId, Param, StaticInit, Stmt};
use std::collections::HashMap;

/// The bodies of the class `s`, with the parameters each binds: the
/// constructor, the methods and accessors, the static blocks.
fn members(s: &Stmt) -> Vec<(&[Param], &[Stmt])> {
    let Stmt::ClassDecl {
        ctor,
        methods,
        static_methods,
        static_init,
        ..
    } = s
    else {
        return Vec::new();
    };
    let mut out: Vec<(&[Param], &[Stmt])> = Vec::new();
    if let Some(c) = ctor {
        out.push((&c.params, &c.body));
    }
    for m in methods.iter().chain(static_methods) {
        out.push((&m.params, &m.body));
    }
    for i in static_init {
        if let StaticInit::Block(b) = i {
            out.push((&[], b));
        }
    }
    out
}

/// The static field initializers of the class `s`.
fn static_fields(s: &Stmt) -> Vec<ExprId> {
    match s {
        Stmt::ClassDecl { static_init, .. } => static_init
            .iter()
            .filter_map(|i| match i {
                StaticInit::Field(f) => Some(f.init),
                StaticInit::Block(_) => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// The classes declared under `s`, `s` included, outside function
/// declarations.
fn classes_under(s: &Stmt) -> Vec<&Stmt> {
    match s {
        Stmt::ClassDecl { .. } => vec![s],
        Stmt::FnDecl { .. } => Vec::new(),
        _ => stmt_children_ref(s)
            .into_iter()
            .flat_map(classes_under)
            .collect(),
    }
}

/// Whether a member of a class declared under `s` reads `name`.
pub(super) fn class_reads(ast: &Ast, s: &Stmt, name: &str) -> bool {
    classes_under(s).into_iter().any(|c| {
        crate::ast::free_vars::free_vars_of_body(ast, &[], std::slice::from_ref(c))
            .iter()
            .any(|n| n == name)
    })
}

/// Whether a member of a class declared under `s` assigns `name`.
pub(super) fn class_assigns(ast: &Ast, s: &Stmt, name: &str) -> bool {
    classes_under(s).into_iter().any(|c| {
        let mut roots = static_fields(c);
        for (_, body) in members(c) {
            for st in body {
                stmt_roots(st, &mut roots);
            }
        }
        assigns_deep(ast, &roots, name)
    })
}

/// Rename the references `renames` names in the members of every
/// class declared under `s`, past a member that rebinds the name.
pub(super) fn rename_in_classes(ast: &mut Ast, s: &mut Stmt, renames: &HashMap<String, String>) {
    match s {
        Stmt::FnDecl { .. } => {}
        Stmt::ClassDecl { .. } => {
            for e in static_fields(s) {
                crate::ast::nested_fns_idents::rewrite_idents_in_expr(ast, e, renames, true);
            }
            let Stmt::ClassDecl {
                ctor,
                methods,
                static_methods,
                static_init,
                ..
            } = s
            else {
                return;
            };
            let mut bodies: Vec<(&[Param], &mut Vec<Stmt>)> = Vec::new();
            if let Some(c) = ctor {
                bodies.push((&c.params, &mut c.body));
            }
            for m in methods.iter_mut().chain(static_methods.iter_mut()) {
                bodies.push((&m.params, &mut m.body));
            }
            for i in static_init.iter_mut() {
                if let StaticInit::Block(b) = i {
                    bodies.push((&[], b));
                }
            }
            for (params, body) in bodies {
                let inner: HashMap<String, String> = renames
                    .iter()
                    .filter(|(k, _)| !crate::ast::nested_fns_idents::arrow_rebinds(params, body, k))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                crate::ast::nested_fns_idents::rewrite_idents_in_body(ast, body, &inner, true);
                for st in body.iter_mut() {
                    rename_in_classes(ast, st, &inner);
                }
            }
        }
        _ => {
            for c in stmt_children(s) {
                rename_in_classes(ast, c, renames);
            }
        }
    }
}

/// Whether a declaration under `s` rebinds `name`, the parameters and
/// declarations of class members aside: [`rename_in_classes`] stops
/// at those itself, so they end no rename's reach.
pub(super) fn rebinds_past_members(s: &Stmt, name: &str) -> bool {
    if !crate::ast::rebinds_in_stmt(s, name) {
        return false;
    }
    let mut bare = s.clone();
    strip_members(&mut bare);
    crate::ast::rebinds_in_stmt(&bare, name)
}

fn strip_members(s: &mut Stmt) {
    match s {
        Stmt::ClassDecl {
            ctor,
            methods,
            static_methods,
            static_init,
            ..
        } => {
            *ctor = None;
            methods.clear();
            static_methods.clear();
            static_init.clear();
        }
        Stmt::FnDecl { .. } => {}
        _ => {
            for c in stmt_children(s) {
                strip_members(c);
            }
        }
    }
}

/// The references to `marker` in the members of the class `s` itself.
pub(super) fn own_class_refs(ast: &Ast, s: &Stmt, marker: &str) -> Vec<ExprId> {
    let mut out = Vec::new();
    for e in static_fields(s) {
        collect_deep(ast, e, marker, &mut out);
    }
    for (_, body) in members(s) {
        for st in body {
            collect_deep_stmt(ast, st, marker, &mut out);
        }
    }
    out
}
