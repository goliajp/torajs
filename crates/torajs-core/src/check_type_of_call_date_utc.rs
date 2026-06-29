//! `Date.UTC(...)` 1-6 arg overload early-route arm extracted
//! from [`crate::check_type_of_call::check`]'s top-level
//! cascade (chunk 214 — eighth sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! S153 — ES §21.4.2.21 `Date.UTC(year, month?, date?,
//! hours?, minutes?, seconds?, ms?)` returns the time value
//! (Number). The static sig in `check_globals` pins arity 7
//! (Number×7 → Number); this arm accepts the trailing-
//! defaults overloads (1..=6 args). The 7-arg form keeps
//! using the static-sig path unchanged.
//!
//! Returns `Some(Ok(Number))` on match; `Some(Err(_))` on
//! arg typecheck failure; `None` when callee isn't
//! `Date.UTC` or arg count is outside 1..=6.

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn try_match(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Option<Result<Type, String>> {
    if let Expr::Member {
        obj: ns_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && m_name == "UTC"
        && let Expr::Ident(ns) = ast.get_expr(*ns_id)
        && ns == "Date"
        && (1..=6).contains(&args.len())
    {
        for a in args {
            if let Err(e) = checker.type_of(ast, *a) {
                return Some(Err(e));
            }
        }
        return Some(Ok(Type::Number));
    }
    None
}
