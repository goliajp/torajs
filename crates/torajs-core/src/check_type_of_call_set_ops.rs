//! `Set.{isSubsetOf,isSupersetOf,isDisjointFrom,union,
//! intersection,difference,symmetricDifference}(other,
//! ...trailing)` Set ES2025 setops-receiver trailing-arg
//! ignore wedge arm extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 274 — sixty-seventh sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! S318 — Set ES2025 setops trailing-arg ignore per ES
//! §24.2.5.{4-10}: spec sig is 1-arg (other SetLike);
//! trailing args silent-drop. Pre-S318 the fixed
//! (`Type::Set`, `"isSubsetOf"|...`) method-table sig
//! `vec![Type::Set]` rejected `args.len() >= 2` with
//! "expected 1 argument(s), got M". typecheck-drop
//! `args[1..]` mirror in ssa_lower (replace
//! `debug_assert_eq!(args.len(), 1)` carve-out with
//! lower-and-drop loop after args[0] lower).
//!
//! Receiver / m_name / arity matrix:
//! - `Type::Set` + m_name ∈ {isSubsetOf, isSupersetOf,
//!   isDisjointFrom} + args.len() >= 2 → eval all args →
//!   `Some(Ok(Type::Boolean))`
//! - `Type::Set` + m_name ∈ {union, intersection, difference,
//!   symmetricDifference} + args.len() >= 2 → eval all args →
//!   `Some(Ok(Type::Set))`
//!
//! Returns:
//! - `Some(Ok(Type::Boolean))` for predicate setops success
//! - `Some(Ok(Type::Set))` for constructor setops success
//! - `Some(Err(_))` on receiver / arg type_of failure
//! - `None` otherwise (non-Member callee, m_name not in
//!   setops set, args.len() < 2, or non-Set receiver —
//!   cascade falls through to the S315
//!   Object.getOwnPropertyDescriptor wedge and beyond)

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
        "isSubsetOf"
            | "isSupersetOf"
            | "isDisjointFrom"
            | "union"
            | "intersection"
            | "difference"
            | "symmetricDifference"
    ) {
        return None;
    }
    if args.len() < 2 {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(src_ty, Type::Set) {
        return None;
    }
    for &a in args.iter() {
        if let Err(e) = checker.type_of(ast, a) {
            return Some(Err(e));
        }
    }
    Some(Ok(match m_name.as_str() {
        "isSubsetOf" | "isSupersetOf" | "isDisjointFrom" => Type::Boolean,
        _ => Type::Set,
    }))
}
