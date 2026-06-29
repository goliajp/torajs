//! `Object.{getPrototypeOf,isExtensible,isSealed,
//! preventExtensions,seal}(obj, ...trailing)` Object-namespace
//! meta-method trailing-arg ignore wedge arm extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 271 — sixty-fourth sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! S265 — Object.{getPrototypeOf,isExtensible,isSealed,
//! preventExtensions,seal}(obj, ...trailing) per ES
//! §20.1.2.{12,13,14,16,18,20}. Each method's fixed sig
//! (`vec![Type::Any]`) rejected 1+ trailing args; ssa_lower
//! for the 4 anyv_* helpers already drops args[1..] via
//! `for a in args.iter().skip(1) { let _ = checker.lower_expr(*a); }`;
//! getPrototypeOf gets the same treatment in the matching
//! widen below. Returns the original Any (proto/cell) or
//! Boolean per the underlying sig.
//!
//! Arity gate: `args.len() >= 2` (trailing-widening; pure
//! 1-arg shapes still hit the strict method table below).
//!
//! Returns:
//! - `Some(Ok(Type::Boolean))` for {isExtensible, isSealed}
//! - `Some(Ok(Type::Any))` for {getPrototypeOf,
//!   preventExtensions, seal}
//! - `Some(Err(_))` on receiver / arg type_of failure
//! - `None` otherwise (non-Member callee, m_name not in
//!   the 5-method allowlist, args.len() < 2, or
//!   non-Object("Object") receiver — cascade falls through
//!   to the Set/Map instance method / general method
//!   dispatch arms below)

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn try_match(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Option<Result<Type, String>> {
    let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
    else {
        return None;
    };
    if !matches!(
        m_name.as_str(),
        "getPrototypeOf" | "isExtensible" | "isSealed" | "preventExtensions" | "seal"
    ) || args.len() < 2
    {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(src_ty, Type::Object("Object")) {
        return None;
    }
    for &arg in args.iter() {
        if let Err(e) = checker.type_of(ast, arg) {
            return Some(Err(e));
        }
    }
    Some(Ok(match m_name.as_str() {
        "isExtensible" | "isSealed" => Type::Boolean,
        _ => Type::Any,
    }))
}
