//! `Set.add(value, ...trailing)` / `Map.set(key, value,
//! ...trailing)` Set/Map-receiver trailing-arg ignore wedge
//! arm extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 266 — fifty-ninth sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! S248 — Set.add / Map.set trailing-arg ignore per
//! ES §24.2.3.1 (Set.prototype.add) / §23.1.3.9
//! (Map.prototype.set). Spec adds the useful args then
//! returns the receiver for chained idiom; tora widens
//! the dispatch to accept trailing operands and drops
//! them at lower-time (ssa-lower debug-assert widens
//! `== N` → `>= N`; receiver-chain rc_inc unchanged).
//! Same narrow-shape as S243-S247.
//!
//! Receiver / m_name / arity matrix:
//! - `Type::Set` + `add` + args.len() >= 2 → eval args[0]
//!   + drop args[1..] → `Some(Ok(Type::Set))`
//! - `Type::Map` + `set` + args.len() >= 3 → eval
//!   args[0..=1] + drop args[2..] → `Some(Ok(Type::Map))`
//!
//! Returns:
//! - `Some(Ok(Type::Set))` for Set.add success
//! - `Some(Ok(Type::Map))` for Map.set success
//! - `Some(Err(_))` on receiver / arg type_of failure
//! - `None` otherwise (non-Member callee, m_name not in
//!   {add, set}, receiver/m_name/arity mismatch — cascade
//!   falls through to the Object.create / general method
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
    if !matches!(m_name.as_str(), "add" | "set") {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    if matches!(src_ty, Type::Set) && m_name == "add" && args.len() >= 2 {
        if let Err(e) = checker.type_of(ast, args[0]) {
            return Some(Err(e));
        }
        for &arg in &args[1..] {
            if let Err(e) = checker.type_of(ast, arg) {
                return Some(Err(e));
            }
        }
        return Some(Ok(Type::Set));
    }
    if matches!(src_ty, Type::Map) && m_name == "set" && args.len() >= 3 {
        if let Err(e) = checker.type_of(ast, args[0]) {
            return Some(Err(e));
        }
        if let Err(e) = checker.type_of(ast, args[1]) {
            return Some(Err(e));
        }
        for &arg in &args[2..] {
            if let Err(e) = checker.type_of(ast, arg) {
                return Some(Err(e));
            }
        }
        return Some(Ok(Type::Map));
    }
    None
}
