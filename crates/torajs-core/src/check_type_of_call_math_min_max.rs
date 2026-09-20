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
//! Returns `Some(Ok(Number))` on match; `Some(Err(_))` only when a
//! recursive `type_of` fails; `None` otherwise.

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
        // §21.3.2.{24,25} step 2 is ToNumber on every argument, and
        // ToNumber reaches every value: `Math.max({}, 1)` is NaN,
        // `Math.max({valueOf(){return 7}}, 1)` is 7. S228 / S342
        // admitted Number | Undefined | Any by name and refused the
        // rest; the shape gate is gone. ssa_lower mirrors this
        // through `lower_to_number_operand`.
        for &aid in args {
            if let Err(e) = checker.type_of(ast, aid) {
                return Some(Err(e));
            }
        }
        return Some(Ok(Type::Number));
    }
    None
}
