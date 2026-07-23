//! `Map.groupBy(items, callbackFn)` per ES §24.2.2.4 — checker
//! wedge. Sister to [`crate::check_type_of_call_object_group_by`];
//! same admit-any-args shape, but the accumulator is a Map (keys
//! retain runtime type via SameValueZero — no ToPropertyKey
//! coercion). Answers `Type::Map` — the returned Map has any-typed
//! keys and `Array<Any>` values; downstream reads ride the runtime
//! Map lanes.

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
    if m != "groupBy" {
        return None;
    }
    let Expr::Ident(ns) = ast.get_expr(*obj) else {
        return None;
    };
    if ns != "Map" {
        return None;
    }
    if args.len() < 2 {
        return Some(Err(
            "Map.groupBy requires (items, callbackFn) per ES §24.2.2.4".into(),
        ));
    }
    for &aid in args {
        if let Err(e) = checker.type_of(ast, aid) {
            return Some(Err(e));
        }
    }
    Some(Ok(Type::Map))
}
