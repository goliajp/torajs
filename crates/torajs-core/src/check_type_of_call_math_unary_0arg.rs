//! `Math.<unary>()` 0-arg arms extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 227 — twentieth sub-batch of check_type_of_call.rs
//! per-shape decomposition).
//!
//! **S203** — Math unary methods 0-arg per ES §21.3.2.* step
//! 1: missing arg defaults to undefined →
//! `ToNumber(undefined) = NaN` → `Math.<f>(NaN) = NaN`. The
//! declared `vec![Type::Number]` signature rejected the
//! no-arg form at the generic arity gate; accept it and let
//! ssa_lower emit the static NaN return (no helper Call).
//!
//! Covers: sqrt, abs, floor, ceil, log, exp, sign, round,
//! trunc, sin, cos, tan, asin, acos, atan, log2, log10, cbrt,
//! sinh, cosh, tanh, asinh, acosh, atanh, expm1, log1p,
//! clz32, fround, f16round.
//!
//! Returns `Some(Ok(Number))` on match; `None` otherwise.

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn try_match(
    _checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Option<Result<Type, String>> {
    if let Expr::Member { obj, name: m } = ast.get_expr(*callee)
        && let Expr::Ident(ns) = ast.get_expr(*obj)
        && ns == "Math"
        && args.is_empty()
        && matches!(
            m.as_str(),
            "sqrt"
                | "abs"
                | "floor"
                | "ceil"
                | "log"
                | "exp"
                | "sign"
                | "round"
                | "trunc"
                | "sin"
                | "cos"
                | "tan"
                | "asin"
                | "acos"
                | "atan"
                | "log2"
                | "log10"
                | "cbrt"
                | "sinh"
                | "cosh"
                | "tanh"
                | "asinh"
                | "acosh"
                | "atanh"
                | "expm1"
                | "log1p"
                | "clz32"
                | "fround"
                | "f16round"
        )
    {
        return Some(Ok(Type::Number));
    }
    None
}
