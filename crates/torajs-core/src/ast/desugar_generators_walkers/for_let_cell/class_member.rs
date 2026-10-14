//! Classes declared where a binding gets a per-pass local.
//!
//! A class declaration is not hoisted: §15.7.14 evaluates it when its
//! statement runs, and its members close over the environment of that
//! moment. So a class is hosted like an arrow: the local stands in
//! front of the statement that makes it and the members read it. The
//! class may sit in that statement directly, in an arrow the statement
//! makes, or in a member of another such class. Member bodies are
//! statement lists the expression walks of the parent module do not
//! reach, hence the walks here.

use super::scan::{arrows_in, assigned, collect_deep_stmt};
use crate::ast::desugar_with::walk::{stmt_children, stmt_children_ref, stmt_exprs};
use crate::ast::nested_fns_idents::{
    arrow_rebinds, rewrite_idents_in_body, rewrite_idents_in_expr,
};
use crate::ast::scope_chain::list_names;
use crate::ast::{Ast, Expr, ExprId, Param, StaticInit, Stmt};
use std::collections::HashMap;

type Renames = HashMap<String, String>;

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

/// What the members of the class `s` evaluate: the static field
/// initializers and parameter defaults, and the member bodies.
pub(super) fn member_roots(s: &Stmt) -> (Vec<ExprId>, Vec<&Stmt>) {
    let mut exprs = static_fields(s);
    let mut stmts = Vec::new();
    for (params, body) in members(s) {
        exprs.extend(params.iter().filter_map(|p| p.default));
        stmts.extend(body);
    }
    (exprs, stmts)
}

/// The classes declared under `s`, `s` included, outside function
/// declarations and closures.
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
        let mut idents = Vec::new();
        collect_deep_stmt(ast, c, name, &mut idents);
        assigned(ast, &idents)
    })
}

/// Rename the references `renames` names in the members of every
/// class declared under `s` — in `s` itself, in the closures it makes,
/// in the members of another such class — past a scope that rebinds
/// the name. The statements and closures around those classes are
/// the plain ident rewrite's.
pub(super) fn rename_in_classes(ast: &mut Ast, s: &mut Stmt, renames: &Renames) {
    if renames.is_empty() {
        return;
    }
    for e in stmt_exprs(s) {
        in_arrows(ast, e, renames);
    }
    match s {
        Stmt::FnDecl { .. } => {}
        Stmt::ClassDecl { .. } => in_members(ast, s, renames),
        Stmt::Block(v) => in_list(ast, v, renames, &[]),
        Stmt::Try {
            body,
            catch_param,
            catch_body,
            finally_body,
            ..
        } => {
            in_list(ast, body, renames, &[]);
            let param: Vec<String> = catch_param.iter().cloned().collect();
            in_list(ast, catch_body, renames, &param);
            if let Some(f) = finally_body {
                in_list(ast, f, renames, &[]);
            }
        }
        Stmt::Switch { cases, default, .. } => {
            // one scope for every clause
            let mut bound = Vec::new();
            for c in cases.iter() {
                list_names(&c.body, &mut bound);
            }
            if let Some(d) = default.as_deref() {
                list_names(d, &mut bound);
            }
            let inner = without(renames, |k| bound.iter().any(|b| b == k));
            for c in cases.iter_mut() {
                in_arrows(ast, c.value, &inner);
                for st in c.body.iter_mut() {
                    rename_in_classes(ast, st, &inner);
                }
            }
            for st in default.iter_mut().flatten() {
                rename_in_classes(ast, st, &inner);
            }
        }
        Stmt::For { init: Some(i), .. } => {
            let mut bound = Vec::new();
            list_names(std::slice::from_ref(i.as_ref()), &mut bound);
            let inner = without(renames, |k| bound.iter().any(|b| b == k));
            for c in stmt_children(s) {
                rename_in_classes(ast, c, &inner);
            }
        }
        Stmt::ForOf {
            var_name, i_ident, ..
        } => {
            let inner = without(renames, |k| k == var_name || k == i_ident);
            for c in stmt_children(s) {
                rename_in_classes(ast, c, &inner);
            }
        }
        Stmt::ForOfSplitIter { var_name, .. } => {
            let inner = without(renames, |k| k == var_name);
            for c in stmt_children(s) {
                rename_in_classes(ast, c, &inner);
            }
        }
        _ => {
            for c in stmt_children(s) {
                rename_in_classes(ast, c, renames);
            }
        }
    }
}

/// [`rename_in_classes`] over what `s` holds, a binding `s` declares
/// itself included: the `for (let …)` whose binding is being renamed.
pub(super) fn rename_in_parts(ast: &mut Ast, s: &mut Stmt, renames: &Renames) {
    for e in stmt_exprs(s) {
        in_arrows(ast, e, renames);
    }
    for c in stmt_children(s) {
        rename_in_classes(ast, c, renames);
    }
}

/// A block-like list: a name it declares lexically, or `bound`, is
/// its own binding below.
fn in_list(ast: &mut Ast, list: &mut [Stmt], renames: &Renames, bound: &[String]) {
    let mut names = bound.to_vec();
    list_names(list, &mut names);
    let inner = without(renames, |k| names.iter().any(|b| b == k));
    for st in list.iter_mut() {
        rename_in_classes(ast, st, &inner);
    }
}

/// The members of the class `s`: each body and its parameter defaults
/// read the enclosing binding unless the member rebinds the name.
fn in_members(ast: &mut Ast, s: &mut Stmt, renames: &Renames) {
    for e in static_fields(s) {
        rewrite_idents_in_expr(ast, e, renames, true);
        in_arrows(ast, e, renames);
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
        // §10.2.11: a default sees the parameters, not the body
        let outer = without(renames, |k| params.iter().any(|p| p.name == k));
        for d in params.iter().filter_map(|p| p.default) {
            rewrite_idents_in_expr(ast, d, &outer, true);
            in_arrows(ast, d, &outer);
        }
        let inner = without(renames, |k| arrow_rebinds(params, body, k));
        rewrite_idents_in_body(ast, body, &inner, true);
        for st in body.iter_mut() {
            rename_in_classes(ast, st, &inner);
        }
    }
}

/// The classes declared in the bodies of the closures under `e`.
fn in_arrows(ast: &mut Ast, e: ExprId, renames: &Renames) {
    for a in arrows_in(ast, &[e]) {
        let Expr::ArrowFn { params, body, .. } = ast.get_expr(a) else {
            continue;
        };
        let inner = without(renames, |k| arrow_rebinds(params, body, k));
        if inner.is_empty() {
            continue;
        }
        let mut body = body.clone();
        for st in body.iter_mut() {
            rename_in_classes(ast, st, &inner);
        }
        if let Expr::ArrowFn { body: slot, .. } = &mut ast.exprs[a.0 as usize] {
            *slot = body;
        }
    }
}

fn without(renames: &Renames, drop: impl Fn(&str) -> bool) -> Renames {
    renames
        .iter()
        .filter(|(k, _)| !drop(k))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
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

/// The references to `marker` in the members of the class `s` itself,
/// the classes and closures inside them included.
pub(super) fn own_class_refs(ast: &Ast, s: &Stmt, marker: &str) -> Vec<ExprId> {
    let mut out = Vec::new();
    if matches!(s, Stmt::ClassDecl { .. }) {
        collect_deep_stmt(ast, s, marker, &mut out);
    }
    out
}
