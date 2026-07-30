//! `Iterator.from(O)` per ES §27.1.6.2 — checker wedge (RFC
//! 20260730-iterator-global 刀 4). Groupby-family shape: admit any
//! single value, answer `Type::Any` — the derived iterator is a
//! runtime-dispatched cell (helper / builtin iterator / the input
//! itself), with no static type of its own.

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
    if m != "from" {
        return None;
    }
    let Expr::Ident(ns) = ast.get_expr(*obj) else {
        return None;
    };
    if ns != "Iterator" {
        return None;
    }
    if args.is_empty() {
        return Some(Err(
            "Iterator.from requires a value per ES §27.1.6.2".into(),
        ));
    }
    for &aid in args {
        if let Err(e) = checker.type_of(ast, aid) {
            return Some(Err(e));
        }
    }
    Some(Ok(Type::Any))
}
