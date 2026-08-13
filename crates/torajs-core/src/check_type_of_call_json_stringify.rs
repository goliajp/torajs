//! `JSON.stringify(value, replacer?, indent?)` varargs-
//! widening arm extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 217 — eleventh sub-batch of check_type_of_call.rs
//! per-shape decomposition).
//!
//! S311 — ES §25.5.2 silently ignores args past
//! (value, replacer, space). `space` is implemented (the lowering
//! computes a gap Str and the static unfold splices it), and so now
//! is the FUNCTION replacer (§25.5.2.2 step 3 — the lowering routes
//! such a call to `__torajs_anyv_json_stringify_full`).
//!
//! What is still unserved is the ARRAY form's §25.5.2.1 step 4.b
//! PropertyList, and it is refused here rather than ignored, because
//! ignoring it is a SILENT WRONG: `JSON.stringify({a:1,b:2}, ["a"])`
//! answered the unfiltered object instead of `{"a":1}`, and that
//! output is valid JSON no gate can tell from a correct one.
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
                "not yet supported: JSON.stringify array replacer (§25.5.2.1 PropertyList) \
                 — a function replacer and `null` both work"
                    .to_string(),
            ));
        }
        return Some(Ok(Type::String));
    }
    None
}

/// `true` when slot 2's type is the ONE §25.5.2 step 4 shape still
/// unserved — an Array, whose §25.5.2.1 step 4.b PropertyList is not
/// built yet. A callable is served (the lowering routes the call to
/// `__torajs_anyv_json_stringify_full`), and everything else — `null`
/// and `undefined` included — the spec ignores outright: step 4 only
/// looks at the argument when it is an Object. A string or number in
/// that slot is discarded by the spec too, so dropping it is not a
/// wrong answer.
///
/// Type-driven rather than syntactic, because the syntactic version
/// was wrong in the other direction: it refused
/// `JSON.stringify(42, step("t1"))`, where the argument is a string
/// and being ignored is exactly what the spec asks for.
///
/// An `Any` here is not refused and no longer needs to be: the
/// lowering routes it to the same kernel, which tests callability at
/// run time — the residual silent-drop hole rotation 387 recorded is
/// closed for the function form.
fn serves_as_replacer(t: Type) -> bool {
    matches!(t, Type::Array(_))
}
