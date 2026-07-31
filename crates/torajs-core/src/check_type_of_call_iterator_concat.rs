//! `Iterator.concat(...items)` (proposal-iterator-sequencing) —
//! checker wedge (RFC 20260730-iterator-global 刀 5a). Groupby-family
//! shape: admit any argument count (zero items is a valid empty
//! iterator), answer `Type::Any` — the minted cell is a
//! runtime-dispatched IterHelper with no static type of its own.

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
    if m != "concat" {
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
