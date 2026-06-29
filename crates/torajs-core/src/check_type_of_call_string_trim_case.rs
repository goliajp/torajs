//! `s.{trim,trimStart,trimEnd,trimLeft,trimRight,
//! toUpperCase,toLowerCase,toWellFormed,isWellFormed}(
//! ...trailing)` 0-arg trailing-arg ignore arm extracted
//! from [`crate::check_type_of_call::check`]'s top-level
//! cascade (chunk 247 — fortieth sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! S281 — per ES §22.1.3.{30,32,33,28,29,34,10}. Spec-
//! defined 0-arg methods; spec reserves no positional slots
//! so any trailing operand is silently ignored. tora's
//! helpers are 0-arg only (recv only); typecheck-and-drop
//! trailing here; ssa_lower mirror lowers each operand for
//! side effects then drops the value (S272 idiom — never
//! push into argv). toLocale{Upper,Lower}Case is excluded
//! (S140 sig + drop_args path own those — they take a
//! locales arg and already silent-drop it).
//!
//! Returns `Type::Boolean` for `isWellFormed`, `Type::String`
//! otherwise, only when the receiver is `Type::String` AND
//! `!args.is_empty()`. `Some(Err(_))` on recursive `type_of`
//! failure. `None` otherwise (cascade falls through to the
//! strict method table).

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
    if !matches!(
        m_name.as_str(),
        "trim"
            | "trimStart"
            | "trimEnd"
            | "trimLeft"
            | "trimRight"
            | "toUpperCase"
            | "toLowerCase"
            | "toWellFormed"
            | "isWellFormed"
    ) || args.is_empty()
    {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(src_ty, Type::String) {
        return None;
    }
    for &a in args {
        if let Err(e) = checker.type_of(ast, a) {
            return Some(Err(e));
        }
    }
    Some(Ok(if m_name == "isWellFormed" {
        Type::Boolean
    } else {
        Type::String
    }))
}
