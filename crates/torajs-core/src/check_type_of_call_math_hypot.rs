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
//! **S271** — accept `Undefined` alongside `Number` per ES
//! §21.3.2.18: `ToNumber(undefined) = NaN`, sum² + sqrt
//! containing NaN → NaN. ssa_lower mirror folds to
//! `ConstF64(NaN)` when any arg is statically Undefined,
//! after eval-and-dropping the non-undef args so trailing
//! side-effect expressions fire.
//!
//! Returns `Some(Ok(Number))` on match; `Some(Err(_))` on
//! arg type mismatch (non-Number-or-Undefined); `None` when
//! callee isn't `Math.hypot`.

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
        for &aid in args {
            let aty = match checker.type_of(ast, aid) {
                Ok(t) => t,
                Err(e) => return Some(Err(e)),
            };
            if !matches!(aty, Type::Number | Type::Undefined) {
                return Some(Err(format!("Math.hypot args must be number, got {aty:?}")));
            }
        }
        return Some(Ok(Type::Number));
    }
    None
}
