//! `xs.{indexOf,lastIndexOf,includes}(needle, fromIndex)`
//! 2-arg Array-receiver arm extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 239 — thirty-second sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! V3-18 m1.h.49 — JS spec §22.1.3.13 / §22.1.3.14 /
//! §22.1.3.16. Pre-fix tora declared with 1 fixed param;
//! widen here.
//!
//! - **S127-4** — Array<Any> accepts cross-type needle. The
//!   1-arg path falls through the generic arg-unify which
//!   skips equality when param is Any; this dedicated 2-arg
//!   branch matches that semantics rather than enforcing a
//!   strict-eq compare (ssa_lower_str's `arr.indexOf` needle-
//!   pack handles I64/F64/Bool/Ptr/refcounted/Any).
//! - **S217** — accept `Undefined` fromIndex per ES §22.1.3.
//!   {13,14,16}: `ToIntegerOrInfinity(undefined) = 0`. ssa_lower
//!   mirror short-circuits to `ConstI64(0)`.
//! - **S331** — accept `Any` fromIndex per ES §23.1.3.{14,
//!   17,18} step 4: `ToIntegerOrInfinity` already coerces
//!   arbitrary-typed input (NaN→0, ±∞→sat). ssa_lower mirror
//!   routes through `anyv_to_number → coerce_to_i64`.
//!
//! Returns `Type::Boolean` for `includes`; `Type::Number` for
//! `indexOf` / `lastIndexOf`. `Some(Err(_))` on bad needle /
//! fromIndex type; `None` otherwise (non-Array receiver,
//! non-{indexOf,lastIndexOf,includes}, or arity ≠ 2).

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
    if !matches!(m_name.as_str(), "indexOf" | "lastIndexOf" | "includes") || args.len() != 2 {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    let Type::Array(elem) = &src_ty else {
        return None;
    };
    let needle_ty = match checker.type_of(ast, args[0]) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if needle_ty != **elem && !matches!(**elem, Type::Any) {
        return Some(Err(format!(
            "Array.{m_name} arg 0 must match elem type {:?}, got {needle_ty:?}",
            **elem
        )));
    }
    let from_ty = match checker.type_of(ast, args[1]) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if from_ty != Type::Number && from_ty != Type::Undefined && from_ty != Type::Any {
        return Some(Err(format!(
            "Array.{m_name} arg 1 (fromIndex) must be number, got {from_ty:?}"
        )));
    }
    Some(Ok(if m_name == "includes" {
        Type::Boolean
    } else {
        Type::Number
    }))
}
