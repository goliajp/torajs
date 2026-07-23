//! `Object.groupBy(items, callbackFn)` per ES §20.1.2.10 — checker
//! wedge. Admits any items shape (validated by the runtime kernel:
//! Array-of-Any lane; nullish + non-array throw a catchable
//! TypeError) and any callback expression. Answers `Type::Any` —
//! the returned object is null-prototype with dynamically-keyed
//! `Array<Any>` buckets (dynobj shape), so downstream property
//! reads take the runtime-Any lanes.
//!
//! Sister wedge to [`crate::check_type_of_call_string_raw`]: same
//! namespace-static-method routing shape, same argv-arity gate,
//! same trailing-arg permissive stance (spec ignores args beyond
//! `[items, cb]`).

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
    if ns != "Object" {
        return None;
    }
    if args.len() < 2 {
        return Some(Err(
            "Object.groupBy requires (items, callbackFn) per ES §20.1.2.10".into(),
        ));
    }
    for &aid in args {
        if let Err(e) = checker.type_of(ast, aid) {
            return Some(Err(e));
        }
    }
    Some(Ok(Type::Any))
}
