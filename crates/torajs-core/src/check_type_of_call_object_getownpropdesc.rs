//! `Object.getOwnPropertyDescriptor(obj, key, ...trailing)`
//! Object-namespace 2-arg-spec trailing-arg ignore wedge
//! arm extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 275 — sixty-eighth sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! S315 — `Object.getOwnPropertyDescriptor(obj, key, ...trailing)`
//! per ES §20.1.2.10: spec sig is 2-arg; trailing args MUST
//! be silently ignored. Pre-S315 the fixed (`Type::Object(
//! "Object")`, `"getOwnPropertyDescriptor"`) method-table
//! sig rejected `args.len() >= 3` with "expected 2
//! argument(s), got M". typecheck-drop trailing + ssa_lower
//! mirror widens `== 2` → `>= 2` and adds `skip(2)` loop.
//!
//! Receiver / m_name / arity matrix:
//! - `Type::Object("Object")` + m_name = "getOwnPropertyDescriptor"
//!   + args.len() >= 3:
//!     - args[0] (obj: Any) type_of'd
//!     - args[1] (key) must be `Type::String` else
//!       `Some(Err("...key must be string, got {aty1:?}"))`
//!     - args[2..] type_of'd for side-effect validation
//!     - returns `Some(Ok(Type::Any))`
//!
//! Returns:
//! - `Some(Ok(Type::Any))` on success
//! - `Some(Err(_))` on receiver / arg type_of failure or
//!   non-string key
//! - `None` otherwise (non-Member callee, m_name not
//!   "getOwnPropertyDescriptor", args.len() < 3, or non-Object
//!   receiver — cascade falls through to the S314
//!   BigInt.{asIntN,asUintN} wedge and beyond)

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
    if m_name != "getOwnPropertyDescriptor" {
        return None;
    }
    if args.len() < 3 {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(src_ty, Type::Object("Object")) {
        return None;
    }
    if let Err(e) = checker.type_of(ast, args[0]) {
        return Some(Err(e));
    }
    let aty1 = match checker.type_of(ast, args[1]) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(aty1, Type::String) {
        return Some(Err(format!(
            "Object.getOwnPropertyDescriptor arg 1 (key) must be string, got {aty1:?}"
        )));
    }
    for &a in args.iter().skip(2) {
        if let Err(e) = checker.type_of(ast, a) {
            return Some(Err(e));
        }
    }
    Some(Ok(Type::Any))
}
