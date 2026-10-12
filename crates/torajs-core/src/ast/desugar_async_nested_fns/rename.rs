//! The rename a lift applies to the body it emptied, scope by scope.
//!
//! A reference is renamed only where the lifted name is still the one
//! in scope: a block, loop head, `catch` clause, `switch` or function
//! that binds the same name hides the lifted declaration below it.
//! `function* m() { function f() {} { const f = () => 7; yield f(); } }`
//! must keep reading the block's `f`. Function bodies hide by
//! parameters and by every `var` and lexical binding they own; a block
//! hides by its own lexical bindings.

use std::collections::{HashMap, HashSet};

use crate::ast::desugar_with::walk::{expr_children, stmt_children, stmt_children_ref, stmt_exprs};
use crate::ast::{Ast, ClassMethod, Expr, ExprId, Param, StaticInit, Stmt};

type Renames = HashMap<String, String>;

/// Rename in the body that owned the lifted declarations; its own
/// bindings are the lifted ones, so nothing here hides them.
pub(super) fn rename_in_owner(ast: &mut Ast, body: &mut [Stmt], renames: &Renames) {
    for s in body.iter_mut() {
        rename_stmt(ast, s, renames);
    }
}

fn without(renames: &Renames, bound: &HashSet<String>) -> Renames {
    renames
        .iter()
        .filter(|(k, _)| !bound.contains(k.as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

/// The names a statement list binds at its own level: `let` / `const` /
/// `using`, functions and classes (a `var` is collected by its function).
fn lexical_names(list: &[Stmt], out: &mut HashSet<String>) {
    for s in list {
        match s {
            Stmt::LetDecl { name, is_var, .. } if !is_var => {
                out.insert(name.clone());
            }
            Stmt::UsingDecl { name, .. }
            | Stmt::FnDecl { name, .. }
            | Stmt::ClassDecl { name, .. } => {
                out.insert(name.clone());
            }
            Stmt::Multi(v) => lexical_names(v, out),
            _ => {}
        }
    }
}

/// Every `var` under `s` that belongs to the enclosing function
/// (`stmt_children_ref` does not enter a nested function or class).
fn var_names(s: &Stmt, out: &mut HashSet<String>) {
    match s {
        Stmt::LetDecl { name, is_var, .. } if *is_var => {
            out.insert(name.clone());
        }
        _ => {
            for c in stmt_children_ref(s) {
                var_names(c, out);
            }
        }
    }
}

/// Rename in a function scope: parameters (defaults included, they
/// see the parameters), its `var`s and its top-level lexical bindings
/// hide the lifted names.
fn rename_function(ast: &mut Ast, params: &[Param], body: &mut [Stmt], renames: &Renames) {
    let mut bound: HashSet<String> = params.iter().map(|p| p.name.clone()).collect();
    for s in body.iter() {
        var_names(s, &mut bound);
    }
    lexical_names(body, &mut bound);
    let own = without(renames, &bound);
    if own.is_empty() {
        return;
    }
    for p in params {
        if let Some(d) = p.default {
            rename_expr(ast, d, &own);
        }
    }
    for s in body.iter_mut() {
        rename_stmt(ast, s, &own);
    }
}

/// Rename in a block-scoped statement list.
fn rename_block(ast: &mut Ast, list: &mut [Stmt], renames: &Renames) {
    let mut bound = HashSet::new();
    lexical_names(list, &mut bound);
    let own = without(renames, &bound);
    if own.is_empty() {
        return;
    }
    for s in list.iter_mut() {
        rename_stmt(ast, s, &own);
    }
}

fn rename_method(ast: &mut Ast, m: &mut ClassMethod, renames: &Renames) {
    rename_function(ast, &m.params, &mut m.body, renames);
}

fn rename_stmt(ast: &mut Ast, s: &mut Stmt, renames: &Renames) {
    match s {
        Stmt::FnDecl { params, body, .. } => rename_function(ast, params, body, renames),
        Stmt::ClassDecl {
            parent,
            static_init,
            ctor,
            methods,
            static_methods,
            ..
        } => {
            if let Some(p) = parent {
                rename_expr(ast, *p, renames);
            }
            if let Some(c) = ctor.as_mut() {
                rename_function(ast, &c.params, &mut c.body, renames);
            }
            for m in methods.iter_mut().chain(static_methods.iter_mut()) {
                rename_method(ast, m, renames);
            }
            for i in static_init.iter_mut() {
                match i {
                    StaticInit::Field(f) => rename_expr(ast, f.init, renames),
                    StaticInit::Block(b) => rename_function(ast, &[], b, renames),
                }
            }
        }
        Stmt::Block(v) => rename_block(ast, v, renames),
        Stmt::For { init, .. } => {
            let mut bound = HashSet::new();
            if let Some(i) = init.as_deref() {
                lexical_names(std::slice::from_ref(i), &mut bound);
            }
            let own = without(renames, &bound);
            for e in stmt_exprs(s) {
                rename_expr(ast, e, &own);
            }
            for c in stmt_children(s) {
                rename_stmt(ast, c, &own);
            }
        }
        Stmt::ForOf {
            var_name,
            elem_expr,
            body,
            forin_obj,
            ..
        } => {
            rename_expr(ast, *elem_expr, renames);
            if let Some(o) = forin_obj {
                rename_expr(ast, *o, renames);
            }
            let own = without(renames, &HashSet::from([var_name.clone()]));
            rename_stmt(ast, body, &own);
        }
        Stmt::ForOfSplitIter {
            var_name,
            parent,
            sep,
            body,
        } => {
            rename_expr(ast, *parent, renames);
            rename_expr(ast, *sep, renames);
            let own = without(renames, &HashSet::from([var_name.clone()]));
            rename_stmt(ast, body, &own);
        }
        Stmt::Try {
            body,
            catch_param,
            catch_body,
            finally_body,
            ..
        } => {
            rename_block(ast, body, renames);
            let own = without(renames, &catch_param.iter().cloned().collect());
            rename_block(ast, catch_body, &own);
            if let Some(f) = finally_body {
                rename_block(ast, f, renames);
            }
        }
        // the cases share one block scope, their tests included
        Stmt::Switch {
            scrutinee,
            cases,
            default,
            ..
        } => {
            rename_expr(ast, *scrutinee, renames);
            let mut bound = HashSet::new();
            for c in cases.iter() {
                lexical_names(&c.body, &mut bound);
            }
            if let Some(d) = default.as_deref() {
                lexical_names(d, &mut bound);
            }
            let own = without(renames, &bound);
            for c in cases.iter_mut() {
                rename_expr(ast, c.value, &own);
                for s2 in c.body.iter_mut() {
                    rename_stmt(ast, s2, &own);
                }
            }
            if let Some(d) = default {
                for s2 in d.iter_mut() {
                    rename_stmt(ast, s2, &own);
                }
            }
        }
        _ => {
            for e in stmt_exprs(s) {
                rename_expr(ast, e, renames);
            }
            for c in stmt_children(s) {
                rename_stmt(ast, c, renames);
            }
        }
    }
}

/// Rename the references in one expression tree; a function value's
/// body is a function scope of its own.
fn rename_expr(ast: &mut Ast, eid: ExprId, renames: &Renames) {
    let mut seen: HashSet<ExprId> = HashSet::new();
    let mut stack = vec![eid];
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        match ast.get_expr(id) {
            Expr::Ident(n) => {
                if let Some(nm) = renames.get(n) {
                    ast.exprs[id.0 as usize] = Expr::Ident(nm.clone());
                }
            }
            Expr::ArrowFn { params, body, .. } => {
                let params = params.clone();
                let mut taken = body.clone();
                rename_function(ast, &params, &mut taken, renames);
                if let Expr::ArrowFn { body: slot, .. } = &mut ast.exprs[id.0 as usize] {
                    *slot = taken;
                }
            }
            _ => stack.extend(expr_children(ast, id)),
        }
    }
}
