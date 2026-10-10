//! A fn-typed return over a value the checker typed `any`
//! (`function pick(c: boolean): (n: number) => number { const m = (c
//! && inc) || h; return m; }`).
//!
//! An `any` box only ever carries the cell shape of a callable, so a
//! `__fn(` return would hand the caller the cell where it calls a bare
//! address. The closure-param pass retags such a return `__cls(` when
//! the returned value is closure-shaped by its syntax, and the
//! lowerer's `effective_ret_ty` when it is a binding annotated `any`;
//! an `any` the checker infers is invisible to both. With the types in
//! hand, the return is retagged here, on the AST the lowerer reads.
//! A contextually seeded return is left alone: its callers take the
//! signature from the context, not from the declaration.
//!
//! Every declaration that is called the same way as a retagged one
//! goes along, since it was copied or matched before the check: its
//! `__forward_*` / `__fwdrecv_*` shells, which return what it returns,
//! and the other rows of its vtable slot with their `__dispatch_*`
//! stub, which a call through the slot cannot tell apart.

use std::collections::{HashMap, HashSet};

use crate::ast::{Ast, ExprId, Stmt};
use crate::ast_closure_param_tag::is_fnsig_ann;
use crate::ast_closure_param_tag_retag::{
    push_child_stmts_mut, retag_fn_return_types, retag_to_cls,
};
use crate::check::Type;

pub(super) fn retag(ast: &mut Ast, expr_types: &HashMap<ExprId, Type>) {
    let contextual = &ast.contextual_ret_fns;
    let mut retagged: HashSet<String> = HashSet::new();
    let mut stack: Vec<&mut Stmt> = ast.stmts.iter_mut().collect();
    while let Some(s) = stack.pop() {
        if let Stmt::FnDecl {
            name,
            return_type,
            body,
            ..
        } = s
            && !contextual.contains(name.as_str())
            && is_fnsig_ann(return_type)
            && returns_any(body, expr_types)
            && let Some(ann) = return_type
        {
            retag_to_cls(ann);
            retagged.insert(name.clone());
        }
        push_child_stmts_mut(s, &mut stack);
    }
    let mut peers = slot_peers(ast, &retagged);
    peers.extend(
        retagged
            .iter()
            .chain(peers.clone().iter())
            .flat_map(|n| crate::ast::shell_names(n)),
    );
    if !peers.is_empty() {
        retag_fn_return_types(&mut ast.stmts, &peers);
    }
}

/// Does a `return` of this body (not of a nested function) hand back a
/// value typed `any`?
fn returns_any(body: &[Stmt], expr_types: &HashMap<ExprId, Type>) -> bool {
    crate::ast_collect_fn_closure_target::fn_returns(body)
        .iter()
        .any(|e| matches!(expr_types.get(e), Some(Type::Any)))
}

/// The other rows of every vtable slot a retagged method sits in,
/// plus the slot's dispatch stubs.
fn slot_peers(ast: &Ast, retagged: &HashSet<String>) -> HashSet<String> {
    let anns: HashMap<&str, &Option<String>> = crate::ast::toplevel_stmts_flat(ast)
        .into_iter()
        .filter_map(|s| match s {
            Stmt::FnDecl {
                name, return_type, ..
            } => Some((name.as_str(), return_type)),
            _ => None,
        })
        .collect();
    let mut out = HashSet::new();
    for (m, by_root) in crate::ast::vtable_slot::slot_groups_by_name(ast) {
        let mut hit = false;
        for rows in by_root.values() {
            let fnsig: Vec<&String> = rows
                .iter()
                .filter(|r| anns.get(r.as_str()).is_some_and(|a| is_fnsig_ann(a)))
                .collect();
            if fnsig.is_empty() || !rows.iter().any(|r| retagged.contains(r)) {
                continue;
            }
            out.extend(fnsig.into_iter().cloned());
            hit = true;
        }
        if hit {
            out.extend(crate::ast::vtable_slot::dispatch_stub_names(
                anns.keys()
                    .map(|k| k.to_string())
                    .collect::<Vec<_>>()
                    .iter(),
                &m,
            ));
        }
    }
    out
}
