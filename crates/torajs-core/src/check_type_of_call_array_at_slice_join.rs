//! `arr.{at,slice,join}(useful, ...trailing)` Array-receiver
//! trailing-arg ignore wedge extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 294 — eighty-sixth sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! Per ES §23.1.3.{1,28,16}: `Array.prototype.at` /
//! `Array.prototype.slice` / `Array.prototype.join` each
//! reserve slots past their useful args but tora's helpers
//! consume only 1/2/1 args respectively. Trailing operand
//! type_of'd for side effects then dropped at lower-time
//! (ssa_lower's `args[N]` slot beyond the useful count is
//! never read). The per-method m_name dispatch widens each
//! arity gate (`== useful` → `>= useful + 1`) so step()-
//! style side-effect exprs fire per ES eval-then-discard
//! semantics.
//!
//! Useful-arg matrix:
//! - `at` (useful=1): args.len() >= 2 — `at(index, ...trail)`
//! - `slice` (useful=2): args.len() >= 3 — `slice(start, end, ...trail)`
//! - `join` (useful=1): args.len() >= 2 — `join(sep, ...trail)`
//!
//! Returns:
//! - `Some(Ok(elem))` for `at` (returns receiver element type)
//! - `Some(Ok(Array(elem)))` for `slice` (returns same shape)
//! - `Some(Ok(Type::String))` for `join` (gated on elem ∈
//!   {String, Number, Boolean, Any})
//! - `Some(Err(_))` on receiver / arg `type_of` failure or
//!   on a useful-arg shape violation
//!   (`Array.at arg 0 must be number` / `Array.slice arg N
//!   must be number` / `Array.join arg 0 must be string`)
//! - `None` otherwise (non-Member callee, m_name outside
//!   {at, slice, join}, receiver not Array, args.len()
//!   below the per-method trailing threshold, or `join`
//!   receiver element type outside the supported set —
//!   cascade falls through to the index-search-trailing
//!   sibling and beyond)

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
    if !matches!(m_name.as_str(), "at" | "slice" | "join") {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    let Type::Array(elem) = &src_ty else {
        return None;
    };
    if m_name == "at" && args.len() >= 2 {
        // §23.1.3.1 step 2 ToIntegerOrInfinity — see the 1-arg
        // sibling; the trailing-arg spelling took the same gate.
        if let Err(e) = checker.type_of(ast, args[0]) {
            return Some(Err(e));
        }
        for &a in args.iter().skip(1) {
            if let Err(e) = checker.type_of(ast, a) {
                return Some(Err(e));
            }
        }
        return Some(Ok((**elem).clone()));
    }
    if m_name == "slice" && args.len() >= 3 {
        let aty0 = match checker.type_of(ast, args[0]) {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        if !matches!(aty0, Type::Number | Type::Undefined) {
            return Some(Err(format!(
                "Array.slice arg 0 must be number, got {aty0:?}"
            )));
        }
        let aty1 = match checker.type_of(ast, args[1]) {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        if !matches!(aty1, Type::Number | Type::Undefined) {
            return Some(Err(format!(
                "Array.slice arg 1 must be number, got {aty1:?}"
            )));
        }
        for &a in args.iter().skip(2) {
            if let Err(e) = checker.type_of(ast, a) {
                return Some(Err(e));
            }
        }
        return Some(Ok(Type::Array(elem.clone())));
    }
    // No element-type guard — see `check_type_of_call_array_join`:
    // the list was the kernel table, and the kernel is no longer the
    // only lane.
    if m_name == "join" && args.len() >= 2 {
        let aty0 = match checker.type_of(ast, args[0]) {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        if !matches!(aty0, Type::String | Type::Undefined) {
            return Some(Err(format!(
                "Array.join arg 0 must be string, got {aty0:?}"
            )));
        }
        for &a in args.iter().skip(1) {
            if let Err(e) = checker.type_of(ast, a) {
                return Some(Err(e));
            }
        }
        return Some(Ok(Type::String));
    }
    None
}
