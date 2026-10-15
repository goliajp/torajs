//! Moving a declaration out of a `globalThis.X` read's way.
//!
//! When a declaration of `X` is in scope at a `globalThis.X` read, the
//! bare name there reaches the declaration, not the global object. A
//! consistent rename of every declaration of `X` and of every bare `X`
//! that reaches one of them (the scope walk recorded which) changes no
//! behaviour, and afterwards the bare `X` at the read is the global one
//! — so the read keeps the builtin's static type instead of becoming a
//! dynamic member read.
//!
//! Declined, leaving the read on the member form, whenever the rename
//! could be seen or the walk may not have seen everything: a function
//! or class declaration, a loop variable, an import or a
//! named-evaluation binding (`pinned`); an `X` occurrence or an arrow
//! the walk did not reach (a computed class key lives in a side table);
//! a program that had a `with`; or a name some earlier pass recorded
//! by spelling in a side table.

use crate::ast::desugar_with::walk::stmt_children;
use crate::ast::globalthis_member_scope::Census;
use crate::ast::{Ast, Expr, ExprId, Param, Stmt};

pub(super) fn renamable(ast: &Ast, census: &Census, name: &str) -> bool {
    if census.pinned.contains(name) || ast.has_with_stmt || in_name_tables(ast, name) {
        return false;
    }
    let mut arrows = 0usize;
    for (i, e) in ast.exprs.iter().enumerate() {
        match e {
            Expr::ArrowFn { .. } => arrows += 1,
            Expr::Ident(n) | Expr::New { class_name: n, .. }
                if n == name && !census.reached_refs.contains(&ExprId(i as u32)) =>
            {
                return false;
            }
            _ => {}
        }
    }
    arrows == census.arrows_reached
}

/// The name-keyed tables filled before this pass that can hold a
/// binding's spelling.
fn in_name_tables(ast: &Ast, name: &str) -> bool {
    let n = &name.to_string();
    let in_map = |m: &std::collections::HashMap<String, String>| {
        m.contains_key(n) || m.values().any(|v| v == n)
    };
    ast.obj_rest_names.contains(n)
        || ast.strict_eval_assign_names.contains(n)
        || ast.script_lex_names.contains(n)
        || ast.sloppy_eval_fn_names.contains(n)
        || ast.variadic_value_bindings.contains(n)
        || ast.fnsig_mismatch_bindings.contains(n)
        || ast.closure_argc_locals.contains(n)
        || ast.fnexpr_recv_locals.contains(n)
        || ast.closure_argv_locals.contains(n)
        || ast.yieldstar_async_vars.contains(n)
        || ast.dup_param_positions.iter().any(|(_, p)| p == n)
        || in_map(&ast.closure_self_names)
        || in_map(&ast.closure_dstr_names)
        || in_map(&ast.nested_fn_names)
        || in_map(&ast.genexpr_names)
}

/// Rename every declaration of `name` and every bare `name` that
/// reaches one; answers the new spelling.
pub(super) fn rename(
    ast: &mut Ast,
    census: &Census,
    name: &str,
    taken: impl Fn(&Ast, &str) -> bool,
) -> String {
    let fresh = (0usize..)
        .map(|k| format!("{name}_gt{k}"))
        .find(|c| !taken(ast, c))
        .expect("an unused spelling exists");
    for eid in census.user_refs.get(name).into_iter().flatten() {
        match &mut ast.exprs[eid.0 as usize] {
            Expr::Ident(n) | Expr::New { class_name: n, .. } => *n = fresh.clone(),
            _ => {}
        }
    }
    let mut stmts = std::mem::take(&mut ast.stmts);
    for s in stmts.iter_mut() {
        rename_binders(s, name, &fresh);
    }
    ast.stmts = stmts;
    for e in ast.exprs.iter_mut() {
        if let Expr::ArrowFn { params, body, .. } = e {
            rename_params(params, name, &fresh);
            for s in body.iter_mut() {
                rename_binders(s, name, &fresh);
            }
        }
    }
    fresh
}

fn rename_params(params: &mut [Param], old: &str, new: &str) {
    for p in params.iter_mut().filter(|p| p.name == old) {
        p.name = new.to_string();
    }
}

fn rename_binders(s: &mut Stmt, old: &str, new: &str) {
    match s {
        Stmt::LetDecl { name, .. } | Stmt::UsingDecl { name, .. } if name == old => {
            *name = new.to_string();
        }
        Stmt::Try {
            catch_param: Some(p),
            ..
        } if p == old => *p = new.to_string(),
        Stmt::FnDecl { params, .. } => rename_params(params, old, new),
        Stmt::ClassDecl {
            ctor,
            methods,
            static_methods,
            ..
        } => {
            if let Some(c) = ctor {
                rename_params(&mut c.params, old, new);
            }
            for m in methods.iter_mut().chain(static_methods.iter_mut()) {
                rename_params(&mut m.params, old, new);
            }
        }
        _ => {}
    }
    for c in stmt_children(s) {
        rename_binders(c, old, new);
    }
}
