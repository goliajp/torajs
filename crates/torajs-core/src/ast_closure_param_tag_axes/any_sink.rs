//! Any-sink axis — a fn-typed parameter whose value the function hands
//! to an `any` slot: a binding or assignment target annotated `any` /
//! `unknown`, the return of a function annotated so, an argument for a
//! parameter of a top-level function annotated so, or a choice that
//! joins it with a scalar literal (`c ? 0 : f`, which the checker joins
//! to `any`).
//!
//! An `any` box carries only the cell shape of a callable: a bare
//! pointer has no fn object to box, so the parameter takes the closure
//! repr and its callers hand it a cell, as for a heap store. Read off
//! the syntax, so an `any` the checker infers elsewhere
//! (`const k = c ? g() : f`, with `g` returning a number) misses the
//! axis and stays refused at the box.

use crate::ast::desugar_async_sm_walk::{visit_expr_children, visit_stmt_exprs};
use crate::ast::{Ast, BinOp, Expr, ExprId, Param, Stmt};
use crate::ast_closure_param_tag::push_child_stmts;
use crate::ast_closure_param_tag_member::passthrough_leaves;
use std::collections::{HashMap, HashSet};

fn is_any_ann(a: &Option<String>) -> bool {
    matches!(a.as_deref().map(str::trim), Some("any" | "unknown"))
}

/// For every top-level function, which of its parameters are annotated
/// `any` / `unknown`.
pub(super) fn any_param_slots(ast: &Ast) -> HashMap<&str, Vec<bool>> {
    crate::ast::toplevel_stmts_flat(ast)
        .into_iter()
        .filter_map(|s| match s {
            Stmt::FnDecl { name, params, .. } => Some((
                name.as_str(),
                params.iter().map(|p| is_any_ann(&p.type_ann)).collect(),
            )),
            _ => None,
        })
        .collect()
}

/// The names the function with `params`, `ret` and `body` hands to an
/// `any` slot, read off its own body (a nested declaration is its own
/// scope).
pub(super) fn any_sunk_names<'a>(
    ast: &'a Ast,
    (params, ret, body): (&'a [Param], &'a Option<String>, &'a [Stmt]),
    any_params: &HashMap<&str, Vec<bool>>,
    out: &mut HashSet<&'a str>,
) {
    let mut anys: HashSet<&str> = params
        .iter()
        .filter(|p| is_any_ann(&p.type_ann))
        .map(|p| p.name.as_str())
        .collect();
    let mut sinks: Vec<ExprId> = Vec::new();
    let mut stack: Vec<&Stmt> = body.iter().collect();
    while let Some(s) = stack.pop() {
        match s {
            Stmt::FnDecl { .. } => continue,
            Stmt::LetDecl {
                name,
                type_ann,
                init,
                ..
            } if is_any_ann(type_ann) => {
                anys.insert(name);
                sinks.push(*init);
            }
            Stmt::Return(Some(e)) if is_any_ann(ret) => sinks.push(*e),
            _ => {}
        }
        push_child_stmts(s, &mut stack);
    }
    for st in body {
        visit_stmt_exprs(st, &mut |e| {
            expr_sinks(ast, e, &anys, any_params, &mut sinks)
        });
    }
    for v in sinks {
        for leaf in passthrough_leaves(ast, v) {
            if let Expr::Ident(n) = ast.get_expr(leaf) {
                out.insert(n);
            }
        }
    }
}

/// The values `e` and its subexpressions hand to an `any` slot.
fn expr_sinks(
    ast: &Ast,
    e: ExprId,
    anys: &HashSet<&str>,
    any_params: &HashMap<&str, Vec<bool>>,
    out: &mut Vec<ExprId>,
) {
    match ast.get_expr(e) {
        Expr::Assign { target, value } => {
            if matches!(ast.get_expr(*target), Expr::Ident(n) if anys.contains(n.as_str())) {
                out.push(*value);
            }
        }
        Expr::Call { callee, args } => {
            if let Expr::Ident(f) = ast.get_expr(*callee)
                && let Some(slots) = any_params.get(f.as_str())
            {
                out.extend(
                    args.iter()
                        .zip(slots)
                        .filter(|(_, any)| **any)
                        .map(|(a, _)| *a),
                );
            }
        }
        Expr::Ternary { .. }
        | Expr::Nullish { .. }
        | Expr::BinOp {
            op: BinOp::LAnd | BinOp::LOr,
            ..
        } => {
            let leaves = passthrough_leaves(ast, e);
            if leaves.iter().any(|&l| is_scalar_literal(ast, l)) {
                out.push(e);
            }
        }
        _ => {}
    }
    visit_expr_children(ast, e, &mut |c| expr_sinks(ast, c, anys, any_params, out));
}

fn is_scalar_literal(ast: &Ast, e: ExprId) -> bool {
    matches!(
        ast.get_expr(e),
        Expr::Number(_) | Expr::String(_) | Expr::Bool(_) | Expr::BigInt { .. }
    )
}
