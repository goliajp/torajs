//! `Object.{keys,getOwnPropertyNames}(obj, ...trailing)` /
//! `Reflect.ownKeys(obj, ...trailing)` namespace 1-useful-
//! arg trailing-arg ignore wedge extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 293 — eighty-fifth sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! Per ES §20.1.2.{17,22} / §28.1.11: `Object.keys` /
//! `Object.getOwnPropertyNames` / `Reflect.ownKeys` all
//! consume only `obj` (1 useful arg) and return an array of
//! own-property keys. Tora's namespace arms declared fixed
//! 1-arg sigs that rejected positional trailing operands at
//! the generic arity check. ssa_lower's namespace dispatch
//! only reads `args[0]` (dynobj key-table walk; emit site
//! ssa_lower.rs ~18745), so a typecheck-and-drop of trailing
//! args here suffices — no SSA mirror needed.
//!
//! Returns:
//! - `Some(Ok(Type::Array(String)))` for all three methods
//!   (ES spec returns an `Array<string>` of own keys)
//! - `Some(Err(_))` on arg `type_of` failure
//! - `None` otherwise (non-Member callee, non-Ident ns, ns /
//!   m_name not in the supported matrix, or
//!   `args.len() < 2` — cascade falls through to the
//!   Object.{entries,freeze,...} sibling and beyond)

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
    let supported = (ns == "Object" && (m_name == "keys" || m_name == "getOwnPropertyNames"))
        || (ns == "Reflect" && m_name == "ownKeys");
    if !supported {
        return None;
    }
    if args.len() < 2 {
        return None;
    }
    if let Err(e) = checker.type_of(ast, args[0]) {
        return Some(Err(e));
    }
    for &arg in args.iter().skip(1) {
        if let Err(e) = checker.type_of(ast, arg) {
            return Some(Err(e));
        }
    }
    Some(Ok(Type::Array(Box::new(Type::String))))
}
