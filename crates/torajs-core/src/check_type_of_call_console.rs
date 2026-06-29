//! `console.{log,error,warn,info,debug}(...)` varargs-widening
//! arm extracted from [`crate::check_type_of_call::check`]'s
//! top-level cascade (chunk 216 — tenth sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! S328 — WHATWG console §1.1.{2,4}. The standard typecheck
//! path would reject ≠1 args because `Type::Function` has
//! fixed arity; this arm widens to arbitrary arity (args
//! typed `Type::Any` so any value is acceptable) and
//! returns `Type::Void`.
//!
//! Returns `Some(Ok(Void))` on match; `Some(Err(_))` on
//! arg typecheck failure; `None` when callee isn't
//! `console.{log,error,warn,info,debug}`.

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn try_match(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Option<Result<Type, String>> {
    if let Expr::Member { obj, name } = ast.get_expr(*callee)
        && let Expr::Ident(ns) = ast.get_expr(*obj)
        && ns == "console"
        && matches!(name.as_str(), "log" | "error" | "warn" | "info" | "debug")
    {
        for &aid in args {
            if let Err(e) = checker.type_of(ast, aid) {
                return Some(Err(e));
            }
        }
        return Some(Ok(Type::Void));
    }
    None
}
