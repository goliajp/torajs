//! `BigInt.{asIntN,asUintN}(bits, value, ...trailing)`
//! BigInt-namespace 2-arg-spec trailing-arg ignore wedge
//! arm extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 276 — sixty-ninth sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! S314 — `BigInt.{asIntN,asUintN}(bits, value, ...trailing)`
//! per ES §21.2.2.{1,2}: spec sig is 2-arg; trailing args
//! MUST be silently ignored. Pre-S314 the fixed
//! (`Type::Object("BigInt")`, m) method-table sig rejected
//! `args.len() >= 3` with "expected 2 argument(s), got M".
//! typecheck-drop trailing + ssa_lower mirror widens gate
//! + lower-and-drop trailing.
//!
//! Receiver / m_name / arity matrix:
//! - `Type::Object("BigInt")` + m_name ∈ {asIntN, asUintN}
//!   + args.len() >= 3:
//!     - args[0] (bits) must be `Type::Number` else
//!       `Some(Err("BigInt.{m_name} arg 0 (bits) must be
//!       number, got {aty0:?}"))`
//!     - args[1] (value) must be `Type::BigInt` else
//!       `Some(Err("BigInt.{m_name} arg 1 (value) must be
//!       bigint, got {aty1:?}"))`
//!     - args[2..] type_of'd for side-effect validation
//!     - returns `Some(Ok(Type::BigInt))`
//!
//! Returns:
//! - `Some(Ok(Type::BigInt))` on success
//! - `Some(Err(_))` on receiver / arg type_of failure or
//!   typed-arg mismatch
//! - `None` otherwise (non-Member callee, m_name not in
//!   {asIntN, asUintN}, args.len() < 3, or non-BigInt
//!   receiver — cascade falls through to the S309
//!   Object.fromEntries wedge and beyond)

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
    if !matches!(m_name.as_str(), "asIntN" | "asUintN") {
        return None;
    }
    if args.len() < 3 {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(src_ty, Type::Object("BigInt")) {
        return None;
    }
    let aty0 = match checker.type_of(ast, args[0]) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(aty0, Type::Number) {
        return Some(Err(format!(
            "BigInt.{m_name} arg 0 (bits) must be number, got {aty0:?}"
        )));
    }
    let aty1 = match checker.type_of(ast, args[1]) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(aty1, Type::BigInt) {
        return Some(Err(format!(
            "BigInt.{m_name} arg 1 (value) must be bigint, got {aty1:?}"
        )));
    }
    for &a in args.iter().skip(2) {
        if let Err(e) = checker.type_of(ast, a) {
            return Some(Err(e));
        }
    }
    Some(Ok(Type::BigInt))
}
