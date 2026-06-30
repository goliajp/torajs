//! `b.toString(...trailing)` / `b.toLocaleString(...trailing)`
//! Boolean / Symbol / String primitive-receiver trailing-arg
//! ignore wedge extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 290 — eighty-second sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! Per ES §19.3.3.{2,3} (Boolean), §20.4.3.{2,3} (Symbol),
//! §22.1.3.{25,26} (String): `toString` / `toLocaleString`
//! on primitive wrappers are useful-arity 0 (no formal
//! parameter is consumed). Tora's primitive arms declared
//! fixed 0-arg sigs, so any positional trailing operand
//! tripped the generic arity check before this wedge.
//! ssa_lower's primitive dispatch only reads the receiver
//! (boolean → "true"/"false" const, symbol → description
//! lookup, string → receiver itself), so a typecheck-and-
//! drop of all trailing args here suffices — no SSA mirror
//! needed.
//!
//! Returns:
//! - `Some(Ok(Type::String))` when the receiver is
//!   `Type::Boolean | Type::Symbol | Type::String`, m_name
//!   is `toString` or `toLocaleString`, and at least one
//!   arg is present (typechecking each arg via
//!   `checker.type_of` and discarding the result)
//! - `Some(Err(_))` on receiver / arg `type_of` failure
//! - `None` otherwise (non-Member callee, m_name not in the
//!   supported set, args empty, or receiver outside the
//!   {Boolean, Symbol, String} matrix — cascade falls
//!   through to the S304 Struct-prototype sibling and
//!   beyond)

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn try_match(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Option<Result<Type, String>> {
    let Expr::Member {
        obj: recv_id,
        name: m_name,
    } = ast.get_expr(*callee)
    else {
        return None;
    };
    if !matches!(m_name.as_str(), "toString" | "toLocaleString") {
        return None;
    }
    if args.is_empty() {
        return None;
    }
    let recv_ty = match checker.type_of(ast, *recv_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(recv_ty, Type::Boolean | Type::Symbol | Type::String) {
        return None;
    }
    for &arg in args.iter() {
        if let Err(e) = checker.type_of(ast, arg) {
            return Some(Err(e));
        }
    }
    Some(Ok(Type::String))
}
