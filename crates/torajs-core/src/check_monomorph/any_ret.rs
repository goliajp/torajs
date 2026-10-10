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
//! stub, which a call through the slot cannot tell apart. So does a
//! declaration that hands on what a retagged one returns (`return
//! pick(k)`, `return super.get()`): the value it returns is a cell.

use std::collections::{HashMap, HashSet};

use crate::ast::{Ast, Expr, ExprId, Stmt};
use crate::ast_closure_param_tag::is_fnsig_ann;
use crate::ast_collect_fn_closure_target::fn_returns;
use crate::ast_collect_fn_closure_target::raw_ret::const_lets;
use crate::check::Type;

mod raw;

pub(super) fn retag(ast: &mut Ast, expr_types: &HashMap<ExprId, Type>) {
    let decls = fnsig_decls(ast);
    let mut lane: HashSet<String> = decls
        .iter()
        .filter(|(_, rets, _)| {
            rets.iter()
                .any(|e| matches!(expr_types.get(e), Some(Type::Any)))
        })
        .map(|(name, _, _)| name.clone())
        .collect();
    while !lane.is_empty() {
        let peers = slot_peers(ast, &lane);
        lane.extend(peers);
        let shells: Vec<String> = lane
            .iter()
            .flat_map(|n| crate::ast::shell_names(n))
            .collect();
        lane.extend(shells);
        let more: Vec<String> = decls
            .iter()
            .filter(|(name, rets, lets)| {
                !lane.contains(name) && rets.iter().any(|&e| hands_on(ast, e, lets, &lane, 0))
            })
            .map(|(name, _, _)| name.clone())
            .collect();
        if more.is_empty() {
            break;
        }
        lane.extend(more);
    }
    raw::hand_back_cells(ast, &lane);
    crate::ast_closure_param_tag_retag::retag_fn_return_types(&mut ast.stmts, &lane);
}

/// Every declaration with a `__fn(` return the context did not seed:
/// its name, the values it returns, and its own immutable bindings by
/// name (not those of a nested function).
type FnsigDecl<'a> = (String, Vec<ExprId>, HashMap<&'a str, ExprId>);

fn fnsig_decls(ast: &Ast) -> Vec<FnsigDecl<'_>> {
    let mut out = Vec::new();
    let mut stack: Vec<&Stmt> = ast.stmts.iter().collect();
    while let Some(s) = stack.pop() {
        if let Stmt::FnDecl {
            name,
            return_type,
            body,
            ..
        } = s
            && !ast.contextual_ret_fns.contains(name.as_str())
            && is_fnsig_ann(return_type)
        {
            out.push((name.clone(), fn_returns(body), const_lets(body)));
        }
        crate::ast_closure_param_tag::push_child_stmts(s, &mut stack);
    }
    out
}

/// Does `e` hand on, unchanged, what a function of `lane` returns: its
/// call, an immutable binding of one, or a choice with one among its
/// arms? Bounded by `depth`, since a binding may name itself.
fn hands_on(
    ast: &Ast,
    e: ExprId,
    lets: &HashMap<&str, ExprId>,
    lane: &HashSet<String>,
    depth: usize,
) -> bool {
    if depth > 16 {
        return false;
    }
    let on = |x: ExprId| hands_on(ast, x, lets, lane, depth + 1);
    match ast.get_expr(e) {
        Expr::Call { callee, .. } => {
            matches!(ast.get_expr(*callee), Expr::Ident(f) if lane.contains(f))
        }
        Expr::Ident(x) => lets.get(x.as_str()).is_some_and(|&init| on(init)),
        Expr::As { expr, .. } => on(*expr),
        Expr::Sequence { right, .. } => on(*right),
        Expr::Ternary {
            then_branch,
            else_branch,
            ..
        } => on(*then_branch) || on(*else_branch),
        Expr::Nullish { lhs: a, rhs: b }
        | Expr::BinOp {
            op: crate::ast::BinOp::LAnd | crate::ast::BinOp::LOr,
            left: a,
            right: b,
        } => on(*a) || on(*b),
        _ => false,
    }
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
