//! `String.repeat(undefined)` 1-arg-undef-count widen arm
//! extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 281 — seventy-fourth sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! S209 — `String.repeat(undefined)` per ES §22.1.3.17
//! step 1: `ToIntegerOrInfinity(undefined) = 0` → return
//! the empty string. Pre-S209 the declared
//! `vec![Type::Number] -> String` sig rejected the typed-
//! Undefined arg with "argument 0: expected Number, got
//! Undefined". ssa_lower_str pushes `ConstI64(0)` for the
//! missing count and routes through `str_repeat` as usual.
//!
//! Receiver / m_name / arity matrix:
//! - `Type::String` + m_name == "repeat" + args.len() == 1
//!   + args[0] typed Undefined:
//!     - returns `Some(Ok(Type::String))`
//!
//! Returns:
//! - `Some(Ok(Type::String))` on success
//! - `Some(Err(_))` on receiver / arg type_of failure
//! - `None` otherwise (non-Member callee, m_name !=
//!   "repeat", args.len() != 1, non-String receiver, or
//!   non-Undefined count — cascade falls through to the
//!   S207 String.replace/replaceAll fewer-than-2-arg widen
//!   wedge and beyond)

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
    if m_name != "repeat" {
        return None;
    }
    if args.len() != 1 {
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
    if matches!(aty, Type::Undefined) {
        return Some(Ok(Type::String));
    }
    None
}
