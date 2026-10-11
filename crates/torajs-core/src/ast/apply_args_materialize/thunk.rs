//! A top-level function's default that stays on the call-site pad and
//! reads a name.
//!
//! The pad pastes the default expression into each caller's argument
//! list, so its names resolve in the CALLER's scope. §10.2.11 evaluates
//! a default in the callee's own scope, which for a top-level function
//! sits directly on the top level: a caller with a local of the same
//! name read that local instead —
//!
//! ```text
//! const k = 1;
//! function f(y: number = k) { return y; }
//! function g() { const k = 2; return f(); }   // bun: 1, the pad: 2
//! ```
//!
//! The defaults that stay on the pad are the typed ones (the guard
//! lane would widen the slot to `any`), so the fix keeps the pad and
//! changes only where its names resolve: the default moves into a
//! top-level function of its own, and the pad calls that. Its names
//! now resolve where the declaration was written, and the slot keeps
//! its type. A default that reads anything only the callee's frame
//! binds — a parameter, its `arguments`, its `this` — is not touched
//! here; those are the guard lane's (see [`body_safe_default`] and the
//! parent's `refs_prior_param`).

use super::*;
use crate::ast::{Param, free_vars};
use std::collections::HashMap;

/// See the module doc. Runs after the guard conversion, so a default
/// that moved into a body is already the `undefined` pad and is not
/// seen.
pub(super) fn scope_pads_to_top_level(ast: &mut Ast, global_fns: &[String]) {
    let mut plan: Vec<(usize, usize, ExprId, String)> = Vec::new();
    for (si, s) in ast.stmts.iter().enumerate() {
        let Stmt::FnDecl {
            params,
            type_params,
            ..
        } = s
        else {
            continue;
        };
        for (pi, p) in params.iter().enumerate() {
            // the function's return type is the slot's own, so the pad
            // stays as typed as the default was; a slot typed by a type
            // parameter has no spelling outside its function
            if let Some(d) = p.default
                && let Some(ann) = p.type_ann.as_deref()
                && !names_a_type_param(ann, type_params)
                && p.name != "__yield_arg"
                && reads_definition_scope(ast, d, params, global_fns)
            {
                plan.push((si, pi, d, ann.to_string()));
            }
        }
    }
    // a constructor and its construct factory carry the same default
    // node, and both have to call the same function
    let mut calls: HashMap<ExprId, ExprId> = HashMap::new();
    let mut minted: Vec<Stmt> = Vec::new();
    let mut next = 0;
    for (si, pi, d, ann) in plan {
        let call = match calls.get(&d) {
            Some(c) => *c,
            None => {
                let name = fresh_name(ast, global_fns, &mut next);
                minted.push(Stmt::FnDecl {
                    name: name.clone(),
                    type_params: Vec::new(),
                    params: Vec::new(),
                    return_type: Some(ann),
                    body: vec![Stmt::Return(Some(d))],
                    is_generator: false,
                    span: crate::lexer::Span { start: 0, end: 0 },
                });
                let callee = ast.add_expr(Expr::Ident(name));
                let c = ast.add_expr(Expr::Call {
                    callee,
                    args: Vec::new(),
                });
                calls.insert(d, c);
                c
            }
        };
        if let Stmt::FnDecl { params, .. } = &mut ast.stmts[si] {
            params[pi].default = Some(call);
        }
    }
    ast.stmts.extend(minted);
}

/// Whether default `d` of a fn with `params` reads a name, and every
/// name it reads is one the top level can supply.
fn reads_definition_scope(ast: &Ast, d: ExprId, params: &[Param], global_fns: &[String]) -> bool {
    if !body_safe_default(ast, d, global_fns, &[], true) {
        return false;
    }
    let free = free_vars::free_vars_of_body(ast, &[], std::slice::from_ref(&Stmt::Expr(d)));
    !free.is_empty()
        && free.iter().all(|n| {
            !n.starts_with("__") && n != "arguments" && !params.iter().any(|p| p.name == *n)
        })
}

fn names_a_type_param(ann: &str, type_params: &[String]) -> bool {
    ann.split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$'))
        .any(|w| type_params.iter().any(|t| t == w))
}

fn fresh_name(ast: &Ast, global_fns: &[String], next: &mut usize) -> String {
    loop {
        let n = format!("__dflt_scope_{next}");
        *next += 1;
        if !ast.source_dunder_idents.contains(&n) && !global_fns.contains(&n) {
            return n;
        }
    }
}
