//! `String.replace(...args)` / `String.replaceAll(...args)`
//! fewer-than-2-arg widen wedge arm extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 282 — seventy-fifth sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! S207 — `String.replace` / `String.replaceAll` with
//! fewer-than-2 args per ES §22.1.3.18 / §22.1.3.19 step 4
//! + step 6a:
//!   searchString = ToString(searchValue ?? undefined)
//!                = "undefined"
//!   replaceValue = ToString(replaceValue ?? undefined)
//!                = "undefined"
//! Pre-S207 the declared `(Any, Any) -> String` sig
//! rejected 0/1-arg shapes at the strict-arity gate; widen
//! here and let ssa_lower_str push the missing "undefined"
//! interns so the helper sees a valid Str needle /
//! replacement. Bun-aligned: 0-arg → identity unless
//! haystack contains "undefined"; 1-arg → first match of
//! needle replaced by the literal "undefined".
//!
//! Note: every present arg is `type_of`'d so its result is
//! populated in `expr_types`. ssa_lower_str detects typed-
//! undefined operands via `expr_types.get` and substitutes
//! the interned "undefined" literal instead of emitting a
//! non-Str operand the helper would deref as a Str pointer.
//!
//! Receiver / m_name / arity matrix:
//! - `Type::String` + m_name ∈ {replace, replaceAll}
//!   + args.len() < 2:
//!     - args[..] type_of'd for side-effect (typed-undefined
//!       detection in ssa_lower_str)
//!     - returns `Some(Ok(Type::String))`
//!
//! Returns:
//! - `Some(Ok(Type::String))` on success
//! - `Some(Err(_))` on receiver / arg type_of failure
//! - `None` otherwise (non-Member callee, m_name not in
//!   {replace, replaceAll}, args.len() >= 2, or non-String
//!   receiver — cascade falls through to the S339
//!   Array.with(Any idx, val) Any-index widen and beyond)

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn try_match(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Option<Result<Type, String>> {
    let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
    else {
        return None;
    };
    if m_name != "replace" && m_name != "replaceAll" {
        return None;
    }
    if args.len() >= 2 {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(src_ty, Type::String) {
        return None;
    }
    for &aid in args {
        if let Err(e) = checker.type_of(ast, aid) {
            return Some(Err(e));
        }
    }
    Some(Ok(Type::String))
}
