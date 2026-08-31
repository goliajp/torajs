//! `Math.hypot(...)` variadic arm extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 222 — fifteenth sub-batch of check_type_of_call.rs
//! per-shape decomposition).
//!
//! Per JS spec §21.3.2.18: `Math.hypot()` returns `+0`,
//! `Math.hypot(x)` returns `|x|`, 2+ args use libm hypot
//! pairwise. The general `Type::Function` check would reject
//! the variadic shape (fixed-arity check). V3-18 m1.h.56
//! dropped the artificial 1-arg minimum.
//!
//! **S271 / rotation 544** — §21.3.2.18 step 2 is ToNumber
//! on each element, which reaches every value, so there is
//! no shape to gate: `Math.hypot('3', '4')` is 5 and
//! `hypot({}, 1)` is NaN. ssa_lower runs each arg through
//! `lower_to_number_operand`, and keeps the statically-
//! Undefined fold to `ConstF64(NaN)` (same answer, no call)
//! after eval-and-dropping the non-undef args so trailing
//! side-effect expressions fire.
//!
//! Returns `Some(Ok(Number))` on match; `Some(Err(_))` when
//! an argument expression fails to type; `None` when callee
//! isn't `Math.hypot`.

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
        && m == "hypot"
    {
        // Rotation 544 — §21.3.2.18 step 2 is ToNumber on each
        // coerced element, so every shape reaches it:
        // `Math.hypot('3', '4')` is 5, `hypot(null, 4)` is 4,
        // `hypot(true, 0)` is 1 and `hypot({}, 1)` is NaN. The gate
        // that stood here admitted Number and Undefined and refused
        // the rest by name; ssa_lower runs each arg through
        // `lower_to_number_operand`.
        for &aid in args {
            if let Err(e) = checker.type_of(ast, aid) {
                return Some(Err(e));
            }
        }
        return Some(Ok(Type::Number));
    }
    None
}
