//! `xs.with(Any idx, val)` Any-index widen arm extracted
//! from [`crate::check_type_of_call::check`]'s top-level
//! cascade (chunk 283 — seventy-fifth sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! S339 — `xs.with(Any idx, val)` per ES §23.1.3.39 step 2:
//! `ToIntegerOrInfinity` accepts arbitrary-typed input. The
//! method-table sig `(Number, T) -> Array<T>` at the Array
//! dispatch site strict-rejected `o: any` operands at
//! typecheck ('argument 0: expected Number, got Any').
//! Widen here so the ssa_lower mirror routes Any through
//! `anyv_to_number → coerce_to_i64 → arr_with` helper.
//! Sister to S331/S332/S333/S334/S335. Covers both basic
//! 2-arg case and trailing-arg ≥3 case (the S283 sibling
//! stays strict-Number for non-Any `args[0]`).
//!
//! Receiver / m_name / arity matrix:
//! - `Type::Array(elem)` + m_name == "with" +
//!   args.len() >= 2 + args[0] typed Any:
//!     - args[1] elem-mismatch (non-Any elem):
//!       `Some(Err(_))`
//!     - otherwise typecheck-drop args[2..] and return
//!       `Some(Ok(Type::Array(elem)))`
//!
//! Returns:
//! - `Some(Ok(Type::Array(_)))` on success
//! - `Some(Err(_))` on receiver / arg type_of failure or
//!   value-arg elem mismatch
//! - `None` otherwise (non-Member callee, m_name != "with",
//!   args.len() < 2, non-Array receiver, or non-Any
//!   `args[0]` — cascade falls through to the S283
//!   trailing-ignore wedge and beyond)

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
    if args.len() < 2 {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    let Type::Array(elem) = &src_ty else {
        return None;
    };
    let aty0 = match checker.type_of(ast, args[0]) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    // §23.1.3.39 step 2 coerces `index`; `Number` is the strict
    // table's own business, everything else lands here.
    if matches!(aty0, Type::Number) {
        return None;
    }
    let inner = (**elem).clone();
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
