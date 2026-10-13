//! A census slot handed what a call returns: `f = mk2(1)` with `mk2(k:
//! number)` returning `(y?: number) => (y === undefined ? 5 : y) + k`.
//! The slot holds the closures the callee's returns build, so it keeps
//! its declared return when each of them does; a closure reads its
//! captures through the bindings the callee's parameters spell.

use std::collections::HashMap;

use crate::ast::{Ast, Expr, ExprId, Stmt};
use crate::check::{Checker, Type};

/// Does every closure the function `callee` names returns still return
/// `ret` when called through a slot typed `ann`?
pub(super) fn returns_declared(
    checker: &Checker,
    ast: &Ast,
    ann: &str,
    callee: ExprId,
    ret: &Type,
) -> bool {
    let Expr::Ident(g) = ast.get_expr(callee) else {
        return false;
    };
    if ast.async_fns.contains(g) || ast.async_generator_fns.contains(g) {
        return false;
    }
    let Some((params, body)) = ast.stmts.iter().find_map(|s| match s {
        Stmt::FnDecl {
            name,
            params,
            body,
            is_generator: false,
            ..
        } if name == g => Some((params, body)),
        _ => None,
    }) else {
        return false;
    };
    let outer: HashMap<String, String> = (params.iter())
        .filter_map(|p| Some((p.name.clone(), p.type_ann.clone()?)))
        .collect();
    let mut returned = Vec::new();
    if !returns(body, &mut returned) || returned.is_empty() {
        return false;
    }
    returned.iter().all(|e| match ast.get_expr(*e) {
        Expr::Closure { fn_name, .. } => {
            super::fn_returns_declared(checker, ast, ann, fn_name, &outer, ret)
        }
        _ => false,
    })
}

/// Collect the values `body` returns; false when one returns nothing.
fn returns(body: &[Stmt], out: &mut Vec<ExprId>) -> bool {
    let mut ok = true;
    crate::ast::walk_stmts(body, &mut |s| match s {
        Stmt::Return(Some(e)) => out.push(*e),
        Stmt::Return(None) => ok = false,
        _ => {}
    });
    ok
}
