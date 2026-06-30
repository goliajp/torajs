//! `Object.{entries,freeze,isFrozen,values}(obj, ...trailing)`
//! Object-namespace 1-useful-arg trailing-arg ignore wedge
//! extracted from [`crate::check_type_of_call::check`]'s
//! top-level cascade (chunk 292 — eighty-fourth sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! Per ES §20.1.2.{5,7,16,22}: `Object.entries` /
//! `Object.freeze` / `Object.isFrozen` / `Object.values`
//! all consume only `obj` (1 useful arg). Tora's namespace
//! arms declared fixed 1-arg sigs that rejected positional
//! trailing operands at the generic arity check.
//! ssa_lower's namespace dispatch only reads `args[0]`
//! (entries/values walk the dynobj key/value table,
//! freeze writes a frozen-flag bit, isFrozen reads it),
//! so a typecheck-and-drop of trailing args here suffices —
//! no SSA mirror needed.
//!
//! Returns:
//! - `Some(Ok(Type::Array(Array(Any))))` for `entries`
//! - `Some(Ok(arg0_ty))` for `freeze` (returns the receiver
//!   in spec, so propagates the receiver's type)
//! - `Some(Ok(Type::Boolean))` for `isFrozen`
//! - `Some(Ok(Type::Array(Any)))` for `values`
//! - `Some(Err(_))` on arg `type_of` failure
//! - `None` otherwise (non-Member callee, non-Ident ns,
//!   ns != "Object", m_name outside the matrix, or
//!   `args.len() < 2` — cascade falls through to the
//!   Object/Reflect 3+arg sibling and beyond)

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
    if ns != "Object" {
        return None;
    }
    if !matches!(
        m_name.as_str(),
        "entries" | "freeze" | "isFrozen" | "values"
    ) {
        return None;
    }
    if args.len() < 2 {
        return None;
    }
    let arg0_ty = match checker.type_of(ast, args[0]) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    for &arg in args.iter().skip(1) {
        if let Err(e) = checker.type_of(ast, arg) {
            return Some(Err(e));
        }
    }
    Some(Ok(match m_name.as_str() {
        "entries" => Type::Array(Box::new(Type::Array(Box::new(Type::Any)))),
        "freeze" => arg0_ty,
        "isFrozen" => Type::Boolean,
        "values" => Type::Array(Box::new(Type::Any)),
        _ => unreachable!(),
    }))
}
