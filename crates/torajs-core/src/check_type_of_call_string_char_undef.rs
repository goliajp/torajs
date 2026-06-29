//! `s.{at,charAt,charCodeAt,codePointAt}(undefined)` 1-arg
//! String-receiver undef-index arm extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 244 — thirty-seventh sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! S222 — per ES §22.1.3.{1,2,3,4} step 2-3:
//! `ToIntegerOrInfinity(undefined) = 0`, so an explicit
//! undef index behaves as the 0-arg / index=0 form.
//! ssa_lower mirror short-circuits the `Undefined` slot to
//! `ConstI64(0)`.
//!
//! Returns `Type::String` for `at` / `charAt`,
//! `Type::Number` for `charCodeAt` / `codePointAt`, only
//! when the receiver is `Type::String` AND the index arg is
//! `Type::Undefined`. `Some(Err(_))` on recursive `type_of`
//! failure. `None` for other shapes (cascade falls through
//! to the S332 Any-widen arm or the strict method table).

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
        "at" | "charAt" | "charCodeAt" | "codePointAt"
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
    if !matches!(aty, Type::Undefined) {
        return None;
    }
    Some(Ok(if matches!(m_name.as_str(), "charAt" | "at") {
        Type::String
    } else {
        Type::Number
    }))
}
