//! `xs.at(idx)` 1-arg Array-receiver arm extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 240 — thirty-third sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! S225 — typed `Array<T>.at(undefined)` per ES §23.1.3.1
//! step 2-3: `ToIntegerOrInfinity(undefined) = 0`, returns
//! `arr[0]`. Accept `Undefined` alongside `Number`; ssa_lower
//! mirror short-circuits `idx = undefined` to `ConstI64(0)`
//! before `coerce_to_i64` (matches S222 charAt early-
//! intercept idiom).
//!
//! Returns `Type::Array` element type on match;
//! `Some(Err(_))` on bad idx type; `None` for non-Array
//! receiver, non-`at`, or arity ≠ 1.

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
    if m_name != "at" || args.len() != 1 {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    let Type::Array(elem) = &src_ty else {
        return None;
    };
    let idx_ty = match checker.type_of(ast, args[0]) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(idx_ty, Type::Number | Type::Undefined) {
        return Some(Err(format!(
            "Array.at arg 0 must be number, got {idx_ty:?}"
        )));
    }
    Some(Ok((**elem).clone()))
}
