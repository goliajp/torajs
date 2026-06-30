//! `WeakMap.{set,get,has,delete}(key[, value], ...trailing)` /
//! `WeakSet.{add,has,delete}(value, ...trailing)`
//! WeakMap/WeakSet-receiver instance-method trailing-arg
//! ignore wedge arm extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 273 — sixty-sixth sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! S301 — WeakMap/WeakSet instance method trailing-arg
//! ignore per ES §24.{3,4}.3.*. Useful arity is set:2 /
//! others:1; fixed-sig declarations rejected trailing args
//! with "expected N argument(s), got M". typecheck-and-drop
//! args[useful..] mirror in ssa_lower (the WeakMap/WeakSet
//! lower dispatch loop also caps useful when building
//! full_args). Same family as S264 Set/Map block (see
//! [`crate::check_type_of_call_mapset_query`]).
//!
//! Receiver / m_name / useful-arity matrix:
//! - `Type::WeakMap` + `set` → useful = 2
//! - `Type::WeakMap` + `{get,has,delete}` → useful = 1
//! - `Type::WeakSet` + `{add,has,delete}` → useful = 1
//! - other → None (skip)
//!
//! When `args.len() > useful`: eval every arg for
//! side-effect validation, return the spec ret type.
//!
//! Returns:
//! - `Some(Ok(Type::Void))` for WeakMap.set / WeakSet.add
//! - `Some(Ok(Type::Nullable(Box::new(Type::Any))))` for
//!   WeakMap.get
//! - `Some(Ok(Type::Boolean))` for {WeakMap,WeakSet}.{has,delete}
//! - `Some(Err(_))` on receiver / arg type_of failure
//! - `None` otherwise (non-Member callee, receiver/m_name
//!   not in the matrix, or `args.len() <= useful` — cascade
//!   falls through to the S318 set-ops wedge below)

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
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    let useful = match (&src_ty, m_name.as_str()) {
        (Type::WeakMap, "set") => Some(2),
        (Type::WeakMap, "get") | (Type::WeakMap, "has") | (Type::WeakMap, "delete") => Some(1),
        (Type::WeakSet, "add") | (Type::WeakSet, "has") | (Type::WeakSet, "delete") => Some(1),
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
    let ret = match (&src_ty, m_name.as_str()) {
        (Type::WeakMap, "set") => Type::Void,
        (Type::WeakMap, "get") => Type::Nullable(Box::new(Type::Any)),
        (Type::WeakSet, "add") => Type::Void,
        _ => Type::Boolean,
    };
    Some(Ok(ret))
}
