//! `(Set|Map).{has,delete}(key, ...trailing)` /
//! `Map.get(key, ...trailing)` / `(Set|Map).clear(...trailing)`
//! Set/Map-receiver instance-method trailing-arg ignore
//! wedge arm extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 272 — sixty-fifth sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! S264 — Set/Map instance method trailing-arg ignore per
//! ES §24.2.3.{4,5,7} (Set.{delete,clear,has}) +
//! §23.1.3.{3,4,6,7} (Map.{delete,clear,get,has}). Each
//! method's fixed sig (`vec![Any]` / `Vec::new()`) rejected
//! 1+ trailing args; ssa_lower's strict
//! `debug_assert_eq!(args.len(), 1)` / `args.is_empty()`
//! becomes a `>= N` floor in the matching widen below.
//! (Set.add / Map.set already covered by S248 — see
//! [`crate::check_type_of_call_mapset_add_set`].)
//!
//! Receiver / m_name / arity matrix:
//! - `Type::{Set,Map}` + `{has,delete}` + args.len() >= 2
//!   → eval all args → `Some(Ok(Type::Boolean))`
//! - `Type::Map` + `get` + args.len() >= 2 → eval all args
//!   → `Some(Ok(Type::Nullable(Box::new(Type::Any))))`
//! - `Type::{Set,Map}` + `clear` + !args.is_empty()
//!   → eval all args → `Some(Ok(Type::Void))`
//!
//! Returns:
//! - `Some(Ok(Type::Boolean))` for has/delete success
//! - `Some(Ok(Type::Nullable(Box::new(Type::Any))))` for
//!   Map.get success
//! - `Some(Ok(Type::Void))` for clear success
//! - `Some(Err(_))` on receiver / arg type_of failure
//! - `None` otherwise (non-Member callee, m_name not in
//!   {has, delete, get, clear}, receiver/m_name/arity
//!   mismatch — cascade falls through to the WeakMap/WeakSet
//!   wedge S301 and beyond)

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
    if !matches!(m_name.as_str(), "has" | "delete" | "get" | "clear") {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    // (Set|Map).{has,delete} accept >= 2 (key + trail).
    if matches!(src_ty, Type::Set | Type::Map)
        && matches!(m_name.as_str(), "has" | "delete")
        && args.len() >= 2
    {
        for &arg in args.iter() {
            if let Err(e) = checker.type_of(ast, arg) {
                return Some(Err(e));
            }
        }
        return Some(Ok(Type::Boolean));
    }
    // Map.get accepts >= 2 (key + trail).
    if matches!(src_ty, Type::Map) && m_name == "get" && args.len() >= 2 {
        for &arg in args.iter() {
            if let Err(e) = checker.type_of(ast, arg) {
                return Some(Err(e));
            }
        }
        return Some(Ok(Type::Nullable(Box::new(Type::Any))));
    }
    // (Set|Map).clear accept >= 1 (trail only).
    if matches!(src_ty, Type::Set | Type::Map) && m_name == "clear" && !args.is_empty() {
        for &arg in args.iter() {
            if let Err(e) = checker.type_of(ast, arg) {
                return Some(Err(e));
            }
        }
        return Some(Ok(Type::Void));
    }
    None
}
