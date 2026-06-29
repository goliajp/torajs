//! `Array.isArray(value, ...trailing)` Array-namespace
//! trailing-arg ignore wedge arm extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 269 — sixty-second sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! S267 — Array.isArray(value, ...trailing) per ES
//! §23.1.2.2. The spec uses only step 1's `value`;
//! trailing args silent-drop. tora's check.rs sig
//! `vec![Type::Any] -> Boolean` rejected 1+ trailing
//! with "expected 1 argument(s), got N"; ssa_lower's
//! intercept already routes through args[0] only, so
//! a skip(1) eval-and-drop loop preserves side effects
//! for any trailing expression.
//!
//! Arity gate: `args.len() >= 2` (i.e. trailing-widening;
//! pure 1-arg shape is handled by
//! [`crate::check_type_of_call_array_isarray_of`]).
//!
//! Returns:
//! - `Some(Ok(Type::Boolean))` on Type::Object("Array")
//!   receiver + all args type_of'd successfully
//! - `Some(Err(_))` on receiver / arg type_of failure
//! - `None` otherwise (non-Member callee, m_name != isArray,
//!   args.len() < 2, or non-Array("Array") receiver —
//!   cascade falls through to the RegExp.test/exec /
//!   general method dispatch arms below)

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
    if m_name != "isArray" || args.len() < 2 {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(src_ty, Type::Object("Array")) {
        return None;
    }
    for &arg in args.iter() {
        if let Err(e) = checker.type_of(ast, arg) {
            return Some(Err(e));
        }
    }
    Some(Ok(Type::Boolean))
}
