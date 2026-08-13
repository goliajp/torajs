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
//! So a replacer whose CHECKED TYPE is one §25.5.2 step 4 would
//! consult — a callable or an array — is now REFUSED here, until
//! that step's `Call(replacerFunction, holder, «key, value»)` and
//! §25.5.2.1's PropertyList are actually served (roadmap).
//!
//! The bar is the checked type, not the spelling. A first attempt
//! refused everything not syntactically `null` / `undefined` and the
//! gate caught it: `JSON.stringify(42, step("t1"))` passes a STRING
//! there, which step 4 discards, so ignoring it is the correct
//! answer and refusing it broke a program tr had right.
//!
//! `JSON.stringify(v, null, 2)` / `(v, undefined, 2)` — the ordinary
//! way to reach `space` — keep working.
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
        let mut rep_ty = None;
        for (n, &aid) in args.iter().enumerate() {
            match checker.type_of(ast, aid) {
                Err(e) => return Some(Err(e)),
                Ok(t) => {
                    if n == 1 {
                        rep_ty = Some(t);
                    }
                }
            }
        }
        if rep_ty.is_some_and(serves_as_replacer) {
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

/// `true` when slot 2's type is one §25.5.2 step 4 would actually
/// CONSULT — a callable or an array. Everything else, `null` and
/// `undefined` included, the spec ignores outright: step 4 only looks
/// at the argument when it is an Object, and step 4.a only treats it
/// as a PropertyList when it is an Array. A string or number in that
/// slot is discarded by the spec too, so dropping it is not a wrong
/// answer and those calls keep working.
///
/// Type-driven rather than syntactic, because the syntactic version
/// was wrong in the other direction: it refused
/// `JSON.stringify(42, step("t1"))`, where the argument is a string
/// and being ignored is exactly what the spec asks for.
///
/// A replacer arriving as `Any` is NOT refused, and that leaves the
/// residual hole: an `any`-typed binding holding a function is still
/// dropped silently. Narrowing that needs the runtime check, which
/// arrives with the implementation itself — refusing all of `Any`
/// here would reject far more correct programs than it protects.
fn serves_as_replacer(t: Type) -> bool {
    matches!(t, Type::Function(_, _) | Type::Array(_))
}
