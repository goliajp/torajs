//! `s.{at,charAt,charCodeAt,codePointAt,repeat}(Any)` 1-arg
//! String-receiver Any-widen arm extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 245 — thirty-eighth sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! S332 — per ES §22.1.3.{1,2,3,4,17} step 2-3 (or step 1 for
//! `repeat`): `ToIntegerOrInfinity` accepts arbitrary-typed
//! input. The method-table sig `(Number) -> X` rejected
//! explicit `o: any` operands at typecheck; widen here so
//! the ssa_lower mirror routes Any through
//! `anyv_to_number → coerce_to_i64 → helper`. Sister to
//! S329 (`fromCharCode` Any).
//!
//! Returns `Type::String` for `at` / `charAt` / `repeat`,
//! `Type::Number` for `charCodeAt` / `codePointAt`, only
//! when the receiver is `Type::String` AND the arg is
//! `Type::Any`. `Some(Err(_))` on recursive `type_of`
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
        "at" | "charAt" | "charCodeAt" | "codePointAt" | "repeat"
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
    let aty = match checker.type_of(ast, args[0]) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(aty, Type::Any) {
        return None;
    }
    Some(Ok(
        if matches!(m_name.as_str(), "charAt" | "at" | "repeat") {
            Type::String
        } else {
            Type::Number
        },
    ))
}
