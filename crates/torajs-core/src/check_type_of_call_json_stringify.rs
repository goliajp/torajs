//! `JSON.stringify(value, replacer?, indent?)` varargs-
//! widening arm extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 217 — eleventh sub-batch of check_type_of_call.rs
//! per-shape decomposition).
//!
//! S311 — ES §25.5.2 silently ignores args past
//! (value, replacer, space). `space` is implemented (the lowering
//! computes a gap Str and the static unfold splices it); the
//! `replacer` is NOT, and slot 2 used to be lowered-and-dropped —
//! which made a written replacer a SILENT WRONG rather than a
//! missing feature. Measured against bun:
//! `JSON.stringify({a:1,b:2}, (k, v) => typeof v === "number" ? v*100 : v)`
//! answered `{"a":1,"b":2}` instead of `{"a":100,"b":200}`, and the
//! array form `JSON.stringify({a:1,b:2}, ["a"])` answered the
//! unfiltered object instead of `{"a":1}`. Both looked like passes.
//!
//! So a replacer that is not syntactically `null` / `undefined` is
//! now REFUSED here, until §25.5.2.1's PropertyList and §25.5.2.2
//! step 3's `Call(replacerFunction, holder, «key, value»)` are
//! actually served (roadmap). The refusal is deliberately coarse —
//! a variable that merely happens to hold `null` is refused too,
//! because nothing at compile time distinguishes it from one holding
//! a function. Over-refusal costs one loud message; the answer it
//! replaces was wrong output the caller had no way to notice, which
//! the design principles rank worst.
//!
//! `JSON.stringify(v, null, 2)` / `(v, undefined, 2)` — the ordinary
//! way to reach `space` — keep working, which is why the gate's
//! stringify fixtures are untouched.
//!
//! Returns `Some(Ok(String))` on match; `Some(Err(_))` on
//! arg typecheck failure or an unserved replacer; `None` when callee
//! isn't `JSON.stringify` or args is empty.

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn try_match(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Option<Result<Type, String>> {
    if let Expr::Member { obj, name } = ast.get_expr(*callee)
        && let Expr::Ident(ns) = ast.get_expr(*obj)
        && ns == "JSON"
        && name == "stringify"
        && !args.is_empty()
    {
        for &aid in args {
            if let Err(e) = checker.type_of(ast, aid) {
                return Some(Err(e));
            }
        }
        if let Some(&rep) = args.get(1)
            && !is_absent_replacer(ast, rep)
        {
            return Some(Err(
                "not yet supported: JSON.stringify replacer (§25.5.2.1 PropertyList / \
                 §25.5.2.2 step 3) — pass `null` for the 2-arg and 3-arg (space) forms"
                    .to_string(),
            ));
        }
        return Some(Ok(Type::String));
    }
    None
}

/// `true` when slot 2 spells "no replacer" — §25.5.2 step 4 only
/// consults the argument when it is an object, so `null` and
/// `undefined` mean the plain serialization the static unfold
/// already produces byte-identically.
///
/// Syntactic on purpose: this is the one shape that can be proven
/// absent without running the program, and admitting anything else
/// would re-open the silent wrong.
fn is_absent_replacer(ast: &Ast, rep: ExprId) -> bool {
    matches!(ast.get_expr(rep), Expr::Null)
        || matches!(ast.get_expr(rep), Expr::Ident(n) if n == "undefined")
}
