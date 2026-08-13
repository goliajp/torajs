//! `JSON.stringify(value, replacer?, indent?)` varargs-
//! widening arm extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 217 — eleventh sub-batch of check_type_of_call.rs
//! per-shape decomposition).
//!
//! S311 — ES §25.5.2 silently ignores args past
//! (value, replacer, space). All three are now served: `space`
//! through the gap Str the lowering computes, and both replacer
//! forms — §25.5.2.2 step 3's `Call(replacerFunction, holder, «key,
//! value»)` and §25.5.2.1 step 4.b's PropertyList — through the
//! `__torajs_anyv_json_stringify_full` kernel the lowering routes a
//! replacer-bearing call to.
//!
//! This arm therefore only typechecks its arguments now. It used to
//! REFUSE a replacer, because ignoring one is a silent wrong rather
//! than a missing feature: `JSON.stringify({a:1,b:2}, ["a"])`
//! answered the unfiltered object, and that output is valid JSON no
//! gate can tell from a correct one. The refusal is gone with the
//! thing it stood in for.
//!
//! Returns `Some(Ok(String))` on match; `Some(Err(_))` on an arg
//! typecheck failure; `None` when the callee isn't `JSON.stringify`
//! or args is empty.

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
        for &aid in args.iter() {
            if let Err(e) = checker.type_of(ast, aid) {
                return Some(Err(e));
            }
        }
        return Some(Ok(Type::String));
    }
    None
}
