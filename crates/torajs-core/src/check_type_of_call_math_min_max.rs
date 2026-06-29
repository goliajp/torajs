//! `Math.{min,max}(...)` variadic arm extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 229 — twenty-second sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! V3-18 m1.h.24 — JS spec §21.3.2.24/25:
//! - `Math.max()` returns `-Infinity`,
//! - `Math.min()` returns `+Infinity` (identity element of
//!   the reduction),
//! - `Math.max(x)` returns `x`.
//!
//! Drops the artificial 2-arg minimum the declared
//! `vec![Number,Number]` signature would impose.
//!
//! - **S228** — accept `Undefined` args per ES NaN-
//!   propagation: any undef arg coerces to NaN
//!   (`ToNumber(undefined)=NaN`) and Math.min/max with any
//!   NaN operand returns NaN. ssa_lower mirror folds the call
//!   when any arg is undef.
//! - **S342** — accept `Any` args per ES §21.3.2.{24,25}
//!   ToNumber: arbitrary-typed input is accepted. ssa_lower's
//!   variadic Math.min/max path routes Any through
//!   `anyv_to_number → F64` (math_to_f64 closure).
//!
//! Returns `Some(Ok(Number))` on match; `Some(Err(_))` on
//! non-Number/Undefined/Any arg; `None` otherwise.

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn try_match(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Option<Result<Type, String>> {
    if let Expr::Member { obj, name: m } = ast.get_expr(*callee)
        && let Expr::Ident(ns) = ast.get_expr(*obj)
        && ns == "Math"
        && (m == "min" || m == "max")
    {
        for &aid in args {
            let aty = match checker.type_of(ast, aid) {
                Ok(t) => t,
                Err(e) => return Some(Err(e)),
            };
            if !matches!(aty, Type::Number | Type::Undefined | Type::Any) {
                return Some(Err(format!("Math.{m} args must be number, got {aty:?}")));
            }
        }
        return Some(Ok(Type::Number));
    }
    None
}
