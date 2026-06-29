//! `Array.isArray()` 0-arg + `Array.of(...vals)` variadic arms
//! extracted from [`crate::check_type_of_call::check`]'s
//! top-level cascade (chunk 224 — seventeenth sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! Both arms share `Array` namespace receiver but differ on
//! method name:
//!
//! - **S204** — `Array.isArray()` 0-arg per ES §23.1.2.2 step
//!   1: missing arg defaults to undefined; undefined is not an
//!   Array, so the predicate is statically false. The
//!   declared `vec![Type::Any]` signature was rejecting the
//!   no-arg form at the generic arity gate, hence the early
//!   route. Non-0-arg `Array.isArray(x)` still uses the
//!   regular sig table.
//! - **`Array.of(...vals)`** — variadic factory that returns
//!   a fresh `Array<T>` with the given values in order. Empty
//!   call requires the caller to use a typed `[]` literal
//!   instead (no element to anchor the type). All args must
//!   unify on the same type.
//!
//! Returns `Some(Ok(_))` on match; `Some(Err(_))` on
//! mismatched arg types / empty `Array.of()`; `None` when
//! callee isn't `Array.{isArray,of}` or `isArray` has args.

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn try_match(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Option<Result<Type, String>> {
    if let Expr::Member { obj, name: m } = ast.get_expr(*callee)
        && let Expr::Ident(ns) = ast.get_expr(*obj)
        && ns == "Array"
    {
        if m == "isArray" && args.is_empty() {
            return Some(Ok(Type::Boolean));
        }
        if m == "of" {
            if args.is_empty() {
                return Some(Err(
                    "Array.of() with zero args needs a typed `[]` literal; \
                 tr can't infer the element type"
                        .into(),
                ));
            }
            let first_ty = match checker.type_of(ast, args[0]) {
                Ok(t) => t,
                Err(e) => return Some(Err(e)),
            };
            for &aid in args.iter().skip(1) {
                let aty = match checker.type_of(ast, aid) {
                    Ok(t) => t,
                    Err(e) => return Some(Err(e)),
                };
                if aty != first_ty {
                    return Some(Err(format!(
                        "Array.of args must agree on element type; first is \
                     {first_ty:?}, later arg is {aty:?}"
                    )));
                }
            }
            return Some(Ok(Type::Array(Box::new(first_ty))));
        }
    }
    None
}
