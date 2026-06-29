//! `xs.slice([start [, end]])` 0-/1-/2-arg arm (Array
//! receiver) extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 230 — twenty-third sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! V3-18 m1.h.35 — JS spec §22.1.3.25:
//! - `xs.slice()` = `xs.slice(0, xs.length)`
//! - `xs.slice(start)` = `xs.slice(start, xs.length)`
//!
//! Pre-fix tora declared slice with 2 fixed params so 0/1-arg
//! calls hit the arity check; widen to `args.len() <= 2`
//! here and let ssa_lower fill the defaults at lower-time.
//!
//! - **S213** — explicit `undefined` for either start or end
//!   per ES §23.1.3.27 step 1-2 (start=undefined → 0,
//!   end=undefined → len).
//! - **S334** — `xs.slice(Any [, Any])` per ES
//!   §23.1.3.{28,27}: ToIntegerOrInfinity accepts arbitrary-
//!   typed input. Sister to S332/S333. ssa_lower mirror
//!   routes Any through anyv_to_number → coerce_to_i64 →
//!   helper.
//!
//! Returns `Some(Ok(Array<T>))` on Array-receiver match;
//! `Some(Err(_))` on non-Number/Undefined/Any arg; `None`
//! when callee isn't a `.slice(...)` on an Array, leaves the
//! String-receiver case to the general dispatch.

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
    if m_name != "slice" || args.len() > 2 {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    let Type::Array(elem) = &src_ty else {
        return None;
    };
    for &aid in args {
        let aty = match checker.type_of(ast, aid) {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        if matches!(aty, Type::Undefined | Type::Any) {
            continue;
        }
        if aty != Type::Number {
            return Some(Err(format!("Array.slice arg must be number, got {aty:?}")));
        }
    }
    Some(Ok(Type::Array(Box::new((**elem).clone()))))
}
