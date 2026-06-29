//! `JSON.stringify(value, replacer?, indent?)` varargs-
//! widening arm extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 217 — eleventh sub-batch of check_type_of_call.rs
//! per-shape decomposition).
//!
//! S311 — ES §25.5.2 silently ignores args past
//! (value, replacer, space). The full JS spec defines
//! `replacer` as a function or array; tr's stringify ignores
//! anything non-null in slot 2 (no callback support yet —
//! roadmap) and consumes only the indent shape (number or
//! string). All args are typechecked against `Type::Any` so
//! the call is accepted; runtime behavior matches the 1-arg
//! form for now, with indent surface ready for a follow-up
//! ssa_lower pass. ssa_lower mirror loop drops trailing
//! args[1..] (replacer/space substrate L3b).
//!
//! Returns `Some(Ok(String))` on match; `Some(Err(_))` on
//! arg typecheck failure; `None` when callee isn't
//! `JSON.stringify` or args is empty.

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
        return Some(Ok(Type::String));
    }
    None
}
