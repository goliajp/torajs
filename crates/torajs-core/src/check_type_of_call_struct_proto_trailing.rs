//! `s.{toString, valueOf, hasOwnProperty,
//! propertyIsEnumerable, isPrototypeOf}(...trailing)`
//! Struct-instance Object.prototype method trailing-arg
//! ignore wedge extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 289 — eighty-first sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! S304 — Struct-instance `Object.prototype` methods
//! trailing-arg ignore per ES §20.1.3.{2,3,4,5,7}. Useful
//! arity: `toString` / `valueOf` are 0-arg, `hasOwnProperty`
//! / `propertyIsEnumerable` / `isPrototypeOf` are 1-arg.
//! Tora's struct-instance arms (4619-4632 in the dispatch
//! site) declared fixed sigs that rejected trailing operands
//! at the generic arity check. ssa_lower's struct dispatch
//! never reads `args[useful..]` (toString returns
//! `"[object Object]"` const; hasOwnProperty does a layout
//! scan keyed on `args[0]` String literal), so trailing
//! typecheck-and-drop here suffices — no SSA mirror needed.
//! (BigInt.toLocaleString trailing also surfaced during
//! probe but its 0-arg form is itself unsupported by
//! ssa_lower — kept in L3b until the substrate lands.)
//!
//! Receiver / m_name / arity matrix:
//! - `Type::Struct(_)` + m_name ∈ {toString, valueOf}
//!   + args.len() > 0:
//!     - typecheck-drop all args
//!     - returns `Some(Ok(Type::String))` for toString
//!     - returns `Some(Ok(recv_ty.clone()))` for valueOf
//! - `Type::Struct(_)` + m_name ∈ {hasOwnProperty,
//!   propertyIsEnumerable, isPrototypeOf}
//!   + args.len() > 1:
//!     - typecheck-drop all args
//!     - returns `Some(Ok(Type::Boolean))`
//!
//! Returns:
//! - `Some(Ok(_))` on success
//! - `Some(Err(_))` on receiver / arg type_of failure
//! - `None` otherwise (non-Member callee, non-Struct
//!   receiver, m_name not in the supported set, or arity
//!   does not exceed the useful count — cascade falls
//!   through to the S261 Date sibling and beyond)

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn try_match(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Option<Result<Type, String>> {
    let Expr::Member {
        obj: recv_id,
        name: m_name,
    } = ast.get_expr(*callee)
    else {
        return None;
    };
    let recv_ty = match checker.type_of(ast, *recv_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    let useful = match (&recv_ty, m_name.as_str()) {
        (Type::Struct(_), "toString") | (Type::Struct(_), "valueOf") => Some(0usize),
        (Type::Struct(_), "hasOwnProperty")
        | (Type::Struct(_), "propertyIsEnumerable")
        | (Type::Struct(_), "isPrototypeOf") => Some(1usize),
        _ => None,
    };
    let useful = useful?;
    if args.len() <= useful {
        return None;
    }
    for &a in args.iter() {
        if let Err(e) = checker.type_of(ast, a) {
            return Some(Err(e));
        }
    }
    Some(Ok(match m_name.as_str() {
        "toString" => Type::String,
        "toLocaleString" => Type::String,
        "valueOf" => recv_ty.clone(),
        "hasOwnProperty" | "propertyIsEnumerable" | "isPrototypeOf" => Type::Boolean,
        _ => unreachable!(),
    }))
}
