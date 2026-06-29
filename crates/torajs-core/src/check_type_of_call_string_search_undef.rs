//! `String.{indexOf,lastIndexOf,includes,startsWith,
//! endsWith,search}(undefined)` 1-arg-undef-needle widen arm
//! extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 233 — twenty-sixth sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! **S235** — per ES §22.1.3.{8,10,5,21,7,16} step 1-3:
//! `ToString(undefined) = "undefined"`. The `Type::Function`
//! arm declares the needle as `String` and would reject the
//! typed-`Undefined` operand; widen the 1-arg dispatch here.
//! ssa_lower_str's `undef_to_str_at_arg0` mirror substitutes
//! the interned `"undefined"` literal for the helper's
//! `(Str, Str)` ABI.
//!
//! Result is `Type::Boolean` for the membership predicates
//! (`includes / startsWith / endsWith`); `Type::Number` for
//! the index lookups (`indexOf / lastIndexOf / search`).
//!
//! Returns `Some(Ok(_))` on String-receiver + Undefined-
//! needle match; `Some(Err(_))` on type_of error; `None`
//! otherwise (non-String receiver, non-Undefined needle, or
//! arity ≠ 1).

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
        "indexOf" | "lastIndexOf" | "includes" | "startsWith" | "endsWith" | "search"
    ) || args.len() != 1
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
    let needle_ty = match checker.type_of(ast, args[0]) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(needle_ty, Type::Undefined) {
        return None;
    }
    Some(Ok(
        if matches!(m_name.as_str(), "includes" | "startsWith" | "endsWith") {
            Type::Boolean
        } else {
            Type::Number
        },
    ))
}
