//! `xs.copyWithin(target[, start[, end]])` Array-receiver
//! 1-3-arg arm extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 252 — forty-fifth sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! V3-18 wedge — `Array.copyWithin` with 1 or 2 args per
//! JS spec §22.1.3.3:
//!   `xs.copyWithin(target)`             = (target, 0, len)
//!   `xs.copyWithin(target, start)`      = (target, start, len)
//!   `xs.copyWithin(target, start, end)` = (target, start, end)
//!
//! Pre-fix tora declared the method with a fixed 3-arg
//! signature so `xs.copyWithin(0, 2)` failed at the arity
//! check. SSA lower already had the 3-arg code path; the
//! call-site fix additionally fills the missing start (= 0)
//! / end (= len) defaults at the SSA layer.
//!
//! S219 — accept Undefined for any arg per ES §23.1.3.4:
//! target/start go through ToIntegerOrInfinity (undef → 0),
//! end===undefined takes the omitted default (len).
//! ssa_lower mirror short-circuits each undef slot to its
//! spec default before relative_to_len.
//!
//! S335 — `xs.copyWithin(Any, Any, Any)` per ES §23.1.3.4:
//! each arg goes through ToIntegerOrInfinity which accepts
//! arbitrary-typed input. Sister to S334.
//!
//! Returns `Some(Ok(Type::Array(elem)))` when the receiver
//! is `Type::Array<T>` AND 1 <= args.len() <= 3 AND every
//! arg is `Number | Undefined | Any`. `Some(Err(_))` on
//! arg type mismatch (exhaustive — does NOT fall through)
//! or recursive `type_of` failure. `None` otherwise (non-
//! copyWithin callee, arity 0 or >3, or non-Array receiver
//! — cascade falls through).

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
    if m_name != "copyWithin" || args.is_empty() || args.len() > 3 {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    let Type::Array(elem) = &src_ty else {
        return None;
    };
    for (i, a) in args.iter().enumerate() {
        let a_ty = match checker.type_of(ast, *a) {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        if a_ty != Type::Number && a_ty != Type::Undefined && a_ty != Type::Any {
            return Some(Err(format!(
                "Array.copyWithin arg {i} must be number, got {a_ty:?}"
            )));
        }
    }
    Some(Ok(Type::Array(elem.clone())))
}
