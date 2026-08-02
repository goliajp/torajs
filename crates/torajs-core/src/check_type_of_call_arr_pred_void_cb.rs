//! Array predicate methods × `Void`-returning callback acceptance
//! wedge (RFC 20260706-test262-bug-corpus RC-1).
//!
//! Per ES §23.1.3.{2,8,9,10,11,30} the predicate's return value goes
//! through ToBoolean — a callback that never returns a value yields
//! `undefined` → `false` on every element. TS's stdlib mirrors this
//! by declaring predicates as `(value) => unknown`. tr's formal sigs
//! declare `(T) => boolean`, so a `Function([...], Void)` actual
//! (`xs.find(function () {})` after the infer-closure ret-seed gate)
//! would strict-reject in the general per-arg loop even though bun
//! runs it. Accept the Void-ret shape here; ssa_lower's predicate /
//! filter arms read the callback's own sig and fold the hit test to
//! the ToBoolean(undefined) constant instead of consuming the void
//! call's (nonexistent) value.
//!
//! Receiver / m_name matrix — `Type::Array(elem)` receiver + m_name in
//! {filter, find, findLast, findIndex, findLastIndex, some, every,
//! map, reduce, reduceRight} + args[0] typed `Function(ps, Void)` with
//! ps a prefix-compatible (elem-or-Any, arity ≤ 1; reduce ≤ 2) param
//! list. Trailing args (thisArg + extras) are type_of'd and dropped
//! per the S270 convention.
//!
//! Returns:
//! - `Some(Ok(...))` — per-method result type (filter → `Array<T>`,
//!   some/every → Boolean, findIndex/findLastIndex → Number,
//!   map → `Array<Any>` of undefined boxes, reduce/reduceRight → Any,
//!   find/findLast → T with the documented zero-sentinel not-found
//!   subset semantics)
//! - `Some(Err(_))` — an arg's `type_of` failed
//! - `None` — shape mismatch; cascade falls through (non-Void rets
//!   keep the strict general-loop check)

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
        "filter"
            | "find"
            | "findLast"
            | "findIndex"
            | "findLastIndex"
            | "some"
            | "every"
            | "map"
            | "reduce"
            | "reduceRight"
    ) {
        return None;
    }
    if args.is_empty() {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(_) => return None,
    };
    let Type::Array(elem) = src_ty else {
        return None;
    };
    let arg0_ty = match checker.type_of(ast, args[0]) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    let Type::Function(ps, ret) = arg0_ty else {
        return None;
    };
    let is_reduce = matches!(m_name.as_str(), "reduce" | "reduceRight");
    // rotation 284 — a predicate's return folds through ToBoolean
    // (ES §23.1.3.{8-11,30}), so the seven predicate methods admit
    // ANY callback return type, not just Void (`cb` returning 1/0
    // counters or heap values is legal JS; TS stdlib spells these
    // `=> unknown`). ssa_lower's predicate/filter arms coerce the
    // ret and release an owned one. map/reduce keep the Void-only
    // admit — their ret feeds the result element / accumulator, a
    // different contract. A Boolean ret keeps the strict general
    // loop (exact-sig fast path unchanged).
    if m_name == "map" || is_reduce {
        if !matches!(*ret, Type::Void) {
            return None;
        }
    } else if matches!(*ret, Type::Boolean | Type::Any) {
        // Boolean and Any rets already pass the strict general loop
        // (exact sig / callback-subtype Any widening) — and that path
        // carries the thisArg-promotion records this early route does
        // not (a `this`-using cb claimed here answered `this ===
        // thisArg` false: every/15.4.4.16-5-* regressed to exit 1).
        // Only the rets the general loop REJECTS take this lane.
        return None;
    }
    // Full spec arity — (elem, index, srcArray); reducers lead with acc.
    if ps.len() > if is_reduce { 4 } else { 3 } {
        return None;
    }
    // reduce's (acc, cur) slots take the boxed-undefined acc / any elem
    // — no slot check; the single-param predicates/map gate on elem.
    if !is_reduce && ps.len() == 1 && ps[0] != *elem && ps[0] != Type::Any && *elem != Type::Any {
        return None;
    }
    // S270 — type_of + drop trailing args (reduce's args[1] is the
    // real initialValue; ssa_lower boxes it into the Any acc slot).
    for &t in args.iter().skip(1) {
        if let Err(e) = checker.type_of(ast, t) {
            return Some(Err(e));
        }
    }
    let r = match m_name.as_str() {
        "filter" => Type::Array(elem),
        "some" | "every" => Type::Boolean,
        "findIndex" | "findLastIndex" => Type::Number,
        // map — every element becomes `undefined`; reduce — the fed-
        // back accumulator is `undefined` after the first call.
        "map" => Type::Array(Box::new(Type::Any)),
        "reduce" | "reduceRight" => Type::Any,
        _ => *elem, // find | findLast — zero-sentinel not-found subset semantics
    };
    Some(Ok(r))
}
