//! `Iterator.zip(iterables, options)` (proposal-joint-iteration) —
//! checker wedge (RFC 20260730-iterator-global 刀 5b). Groupby-family
//! shape: admit any arguments (the kernel runs the spec's own
//! TypeError gates at runtime, including the zero-argument
//! `undefined is not an object` case), answer `Type::Any`.

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn try_match(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Option<Result<Type, String>> {
    let Expr::Member { obj, name: m } = ast.get_expr(*callee) else {
        return None;
    };
    if m != "zip" {
        return None;
    }
    let Expr::Ident(ns) = ast.get_expr(*obj) else {
        return None;
    };
    if ns != "Iterator" {
        return None;
    }
    for &aid in args {
        if let Err(e) = checker.type_of(ast, aid) {
            return Some(Err(e));
        }
    }
    Some(Ok(Type::Any))
}
