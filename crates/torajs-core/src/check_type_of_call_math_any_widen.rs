//! `Math.<unary/binary>(Any[, Any])` widen arm extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 242 — thirty-fifth sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! S342 — `Math.<unary/binary>(Any[, Any])` per ES §21.3.2.*:
//! every `Math` method that takes Number args applies
//! `ToNumber`, which accepts arbitrary-typed input. The
//! method-table sigs `vec![Type::Number]` /
//! `vec![Type::Number, Type::Number]` at the regular Math
//! dispatch site strict-rejected `o: any` operands. Widen
//! here so the ssa_lower mirror routes `Any` through
//! `anyv_to_number → F64 → Math helper`.
//!
//! Per-arg check: `Any` or `Number` passes; everything else
//! (`String` / `Bool` / `Object` / ...) falls through to the
//! strict method-table gate (returns `None` here, lets the
//! cascade keep evaluating).
//!
//! `min` / `max` variadic stays one carve-out — the iter loop
//! covers 0..N args uniformly.
//!
//! Returns `Some(Ok(Type::Number))` only when at least one
//! `Any` arg is seen AND every arg is `Number | Any`.
//! `Some(Err(_))` only if recursive `type_of` on an arg fails.
//! `None` for non-`Math.*` callee, non-matched method name,
//! or all-Number args (let the strict table handle it).

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn try_match(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Option<Result<Type, String>> {
    let Expr::Member {
        obj: ns_id,
        name: m_name,
    } = ast.get_expr(*callee)
    else {
        return None;
    };
    let Expr::Ident(ns) = ast.get_expr(*ns_id) else {
        return None;
    };
    if ns != "Math"
        || !matches!(
            m_name.as_str(),
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
                | "pow"
                | "min"
                | "max"
                | "atan2"
        )
    {
        return None;
    }
    let mut any_seen = false;
    let mut all_ok = true;
    for &aid in args {
        let aty = match checker.type_of(ast, aid) {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        if matches!(aty, Type::Any) {
            any_seen = true;
        } else if aty != Type::Number {
            all_ok = false;
            break;
        }
    }
    if any_seen && all_ok {
        return Some(Ok(Type::Number));
    }
    None
}
