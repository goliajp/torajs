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

use std::collections::HashMap;

use crate::ast::{Ast, ExprId, Stmt};
use crate::ast_closure_param_tag::push_child_stmts;
use crate::ast_closure_param_tag_retag::{push_child_stmts_mut, retag_to_cls};
use crate::check::Type;

pub(super) fn retag(ast: &mut Ast, expr_types: &HashMap<ExprId, Type>) {
    let contextual = &ast.contextual_ret_fns;
    let mut stack: Vec<&mut Stmt> = ast.stmts.iter_mut().collect();
    while let Some(s) = stack.pop() {
        if let Stmt::FnDecl {
            name,
            return_type,
            body,
            ..
        } = s
            && !contextual.contains(name.as_str())
            && crate::ast_closure_param_tag::is_fnsig_ann(return_type)
            && returns_any(body, expr_types)
            && let Some(ann) = return_type
        {
            retag_to_cls(ann);
        }
        push_child_stmts_mut(s, &mut stack);
    }
}

/// Does a `return` of this body (not of a nested function) hand back a
/// value typed `any`?
fn returns_any(body: &[Stmt], expr_types: &HashMap<ExprId, Type>) -> bool {
    let mut stack: Vec<&Stmt> = body.iter().collect();
    while let Some(s) = stack.pop() {
        match s {
            Stmt::Return(Some(e)) if matches!(expr_types.get(e), Some(Type::Any)) => return true,
            Stmt::FnDecl { .. } => continue,
            _ => push_child_stmts(s, &mut stack),
        }
    }
    false
}
