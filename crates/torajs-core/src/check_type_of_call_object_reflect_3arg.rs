//! `Object.{hasOwn,is}(target, key, ...trailing)` /
//! `Reflect.{has,get}(target, key, ...trailing)` namespace
//! 3+ arg trailing-arg ignore wedge extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 291 — eighty-third sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! Per ES §20.1.2.13 (Object.hasOwn) / §20.1.2.10 (Object.is)
//! / §28.1.10 (Reflect.has) / §28.1.6 (Reflect.get), useful
//! arity is 2 — `target` and `key`. Tora's namespace arms
//! declared fixed 2-arg sigs that rejected positional
//! trailing operands at the generic arity check.
//! ssa_lower's namespace dispatch only reads `args[0]` /
//! `args[1]` (predicate compare for Object.is, layout scan
//! for Object.hasOwn, dynobj key lookup for Reflect.has/get),
//! so a typecheck-and-drop of trailing args here suffices —
//! no SSA mirror needed.
//!
//! Returns:
//! - `Some(Ok(Type::Boolean))` for `Object.hasOwn` /
//!   `Object.is` / `Reflect.has`
//! - `Some(Ok(Type::Any))` for `Reflect.get`
//! - `Some(Err(_))` on arg `type_of` failure
//! - `None` otherwise (non-Member callee, non-Ident
//!   namespace, ns / m_name not in the supported matrix,
//!   or `args.len() < 3` — cascade falls through to the
//!   primitive-proto sibling and beyond)

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
    let supported = (ns == "Object" && matches!(m_name.as_str(), "hasOwn" | "is"))
        || (ns == "Reflect" && matches!(m_name.as_str(), "has" | "get"));
    if !supported {
        return None;
    }
    if args.len() < 3 {
        return None;
    }
    if let Err(e) = checker.type_of(ast, args[0]) {
        return Some(Err(e));
    }
    if let Err(e) = checker.type_of(ast, args[1]) {
        return Some(Err(e));
    }
    for &arg in args.iter().skip(2) {
        if let Err(e) = checker.type_of(ast, arg) {
            return Some(Err(e));
        }
    }
    Some(Ok(match m_name.as_str() {
        "hasOwn" | "is" | "has" => Type::Boolean,
        "get" => Type::Any,
        _ => unreachable!(),
    }))
}
