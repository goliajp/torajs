//! `Array.prototype.with(index, value, ...trailing)` 3+ arg
//! trailing-arg wedge extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 284 — seventy-sixth sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! S283 — `Array.prototype.with(index, value, ...trailing)`
//! trailing-arg ignore per ES §23.1.3.39. Spec reads only
//! index + value; tora's `arr_with` intrinsic is 2-arg only.
//! Widen check.rs to accept `args.len() >= 3`; ssa_lower
//! mirror widens the `args.len() == 2` gate to `>= 2` and
//! lowers `args[2..]` for side effects (S272 idiom). The
//! Any-index basic-2-arg case is handled by the S339 sibling
//! `check_type_of_call_array_with_any` above this wedge;
//! this arm stays strict-Number for `args[0]` and only
//! activates on `args.len() >= 3`.
//!
//! Receiver / m_name / arity matrix:
//! - `Type::Array(elem)` + m_name == "with" +
//!   args.len() >= 3:
//!     - args[0] non-Number: `Some(Err(_))`
//!     - args[1] elem-mismatch (non-Any elem):
//!       `Some(Err(_))`
//!     - otherwise typecheck-drop args[2..] and return
//!       `Some(Ok(Type::Array(elem)))`
//!
//! Returns:
//! - `Some(Ok(Type::Array(_)))` on success
//! - `Some(Err(_))` on receiver / arg type_of failure or
//!   index / value-arg type mismatch
//! - `None` otherwise (non-Member callee, m_name != "with",
//!   args.len() < 3, or non-Array receiver — cascade falls
//!   through to the S282 String.{replace,replaceAll,split}
//!   trailing wedge and beyond)

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
    if m_name != "with" {
        return None;
    }
    if args.len() < 3 {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    let Type::Array(elem) = &src_ty else {
        return None;
    };
    let inner = (**elem).clone();
    let aty0 = match checker.type_of(ast, args[0]) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(aty0, Type::Number) {
        return Some(Err(format!(
            "Array.with arg 0 (index) must be number, got {aty0:?}"
        )));
    }
    let aty1 = match checker.type_of(ast, args[1]) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if aty1 != inner && !matches!(inner, Type::Any) {
        return Some(Err(format!(
            "Array.with arg 1 (value) must match elem type {:?}, got {aty1:?}",
            inner
        )));
    }
    for &aid in &args[2..] {
        if let Err(e) = checker.type_of(ast, aid) {
            return Some(Err(e));
        }
    }
    Some(Ok(Type::Array(Box::new(inner))))
}
