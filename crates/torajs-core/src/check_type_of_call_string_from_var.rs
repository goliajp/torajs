//! `String.fromCharCode(...codes)` + `String.fromCodePoint(
//! ...codes)` variadic arms (arity ≠ 1) extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 225 — eighteenth sub-batch of check_type_of_call.rs
//! per-shape decomposition).
//!
//! Both static methods on `String` accept a variadic code
//! list. Each code is a `Number`; result is a `String`. The
//! single-arg case still goes through the general
//! `Type::Function` table for the intrinsic call; this arm
//! only intercepts when arity ≠ 1.
//!
//! **S340** — variadic `Any` arg per ES §22.1.2.{1,2} step 2:
//! `ToNumber`/`ToUint16` accept arbitrary-typed input.
//! `fromCodePoint` inherits its `RangeError` throw shape via
//! the runtime helper `str_from_code_point` (pending throw +
//! emit_throw_check propagates), so the `Any` path inherits
//! the same throw semantics for free. Sister to S329
//! (fromCharCode Any).
//!
//! 0-arg yields the empty-result `String` directly per
//! spec §22.1.2.{1,2} step 1.
//!
//! Rotation 463 — the per-shape admission list (`Number | Any`)
//! became the coercion it always described: every operand shape
//! reaches `ToUint16` / `ToNumber`, so `String.fromCharCode("6",
//! "5")` builds a string instead of failing to typecheck.
//!
//! Returns `Some(Ok(String))` on match; `Some(Err(_))` when an
//! argument's own type_of fails; `None` when callee isn't
//! `String.{fromCharCode,fromCodePoint}` or arity is exactly
//! 1 (handled by the general table).

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
        && ns == "String"
        && (m == "fromCharCode" || m == "fromCodePoint")
        && args.len() != 1
    {
        if args.is_empty() {
            return Some(Ok(Type::String));
        }
        // Every code is COERCED (§22.1.2.1 `ToUint16` / §22.1.2.2
        // `ToNumber`), so no shape is refused here — but each one is
        // still type_of'd, because a type error INSIDE an argument
        // stays loud.
        for &aid in args {
            if let Err(e) = checker.type_of(ast, aid) {
                return Some(Err(e));
            }
        }
        return Some(Ok(Type::String));
    }
    None
}
