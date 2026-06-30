//! `String.search(needle, ...trailing)` trailing-arg ignore
//! wedge arm extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 280 — seventy-third sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! S324 — `String.search(needle, ...trailing)` trailing-arg
//! ignore per ES §22.1.3.20. Spec reads only `needle`;
//! tora's declared `(String) -> Number` sig at the original
//! method-table site rejected 2+ arg calls at strict arity.
//! Mirrors the S240-family widen (1-useful methods):
//! typecheck-and-drop args[1..] and let the existing
//! `(String) -> Number` arm pick up the result type.
//! ssa_lower_str dispatch loop adds `"search"` to the S240
//! 1-useful trailing-drop list so step()-style trailing
//! args fire and the `(Str, Str)` helper ABI never sees
//! them.
//!
//! Receiver / m_name / arity matrix:
//! - `Type::String` + m_name == "search" + args.len() >= 2:
//!     - args[0] (needle) must be `Type::String` or
//!       `Type::Undefined` else `Some(Err("String.search
//!       arg 0 must be string, got {needle_ty:?}"))`
//!     - args[1..] type_of'd for side-effect validation
//!     - returns `Some(Ok(Type::Number))`
//!
//! Returns:
//! - `Some(Ok(Type::Number))` on success
//! - `Some(Err(_))` on receiver / arg type_of failure or
//!   typed-arg mismatch
//! - `None` otherwise (non-Member callee, m_name !=
//!   "search", args.len() < 2, or non-String receiver —
//!   cascade falls through to the S209 String.repeat
//!   undef wedge and beyond)

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
    if m_name != "search" {
        return None;
    }
    if args.len() < 2 {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(src_ty, Type::String) {
        return None;
    }
    let needle_ty = match checker.type_of(ast, args[0]) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(needle_ty, Type::String | Type::Undefined) {
        return Some(Err(format!(
            "String.search arg 0 must be string, got {needle_ty:?}"
        )));
    }
    for &a in args.iter().skip(1) {
        if let Err(e) = checker.type_of(ast, a) {
            return Some(Err(e));
        }
    }
    Some(Ok(Type::Number))
}
