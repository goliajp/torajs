//! `Array.from(iter, mapFn?)` polymorphic early-route arm
//! extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 213 — seventh sub-batch of check_type_of_call.rs
//! per-shape decomposition).
//!
//! `Array.from` is polymorphic over receiver and arity in
//! ways the static-method table can't express:
//!
//! - S132 — `Array.from(arrLike)` 1-arg: receiver-polymorphic
//!   over String / typed Array / Set. The static fn-sig at
//!   ~2993 in check.rs is fixed to `(String) → Array<String>`
//!   for compile-time arrayLike-from-string lowering; for a
//!   typed Array<T> input, return Array<T> directly so a
//!   shallow-copy emit can pick it up.
//! - S141 — `Array.from(set)` per ES §23.1.2.1 + §24.2.3.13
//!   (Set iterator protocol yields values). Set storage tags
//!   each entry as Any (untyped), so result is `Array<Any>`.
//! - S151 — `Array.from(iter, mapFn)` 2-arg: mapFn shape
//!   `(elem) → R` or `(elem, idx) → R`; result is `Array<R>`
//!   where R = mapFn's ret. iter accepts the same 3
//!   substrates as 1-arg.
//! - S275 — widen `args.len() == 2` to `>= 2` per ES
//!   §23.1.2.1: spec accepts `Array.from(iter, mapFn,
//!   thisArg)`; trailing args past thisArg silent-drop. tora's
//!   closures don't bind `this`, so thisArg + further
//!   trailing are typecheck-and-dropped.
//!
//! Returns `Some(Ok(_))` on match, `Some(Err(_))` on arg
//! shape mismatch within the matched 2+ arg path, `None`
//! when callee isn't `Array.from` OR when the 1-arg path
//! falls through to the static-sig path (String overload).

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

/// B6 刀 3 — the checker/lowering SHARED verdict for `Array.from(src,
/// mapFn, …)`: `true` routes the any-tier kernel
/// (`__torajs_array_from_dyn` — spec-exact «kValue, k» call shape,
/// real thisArg binding, per-index Get/map interleave, runtime
/// non-callable TypeError), `false` keeps the typed fast lane
/// (String / typed Array / Set source, Function mapFn, no thisArg —
/// the devirt map loop, no observable-shape difference). Both layers
/// key on this ONE predicate so admit and lowering cannot drift.
pub(crate) fn from_mapfn_routes_kernel(src_ty: &Type, mapfn_ty: &Type, argn: usize) -> bool {
    !matches!(src_ty, Type::String | Type::Array(_) | Type::Set)
        || !matches!(mapfn_ty, Type::Function(..))
        || argn >= 3
}

pub(crate) fn try_match(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Option<Result<Type, String>> {
    // proposal-array-from-async §2.1.1 — `Array.fromAsync(items,
    // mapfn?)` sync-source MVP. Every input type is admitted: the
    // runtime step kernel resolves iterable vs array-like
    // (§23.1.2.1 step-3 shape), a plain number answers
    // `Promise<[]>`, and null / undefined reject with the
    // length-read TypeError. Promise elements unwrap per step 5.e.
    // The 2-arg form admits any mapfn type too — step 2's
    // IsCallable check runs BEFORE iteration in the kernel, so a
    // non-callable rejects with the spec TypeError at runtime.
    // args[2..] (thisArg + trailing) typecheck-and-drop per the
    // Array.from S275 posture.
    if let Expr::Member {
        obj: ns_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && m_name == "fromAsync"
        && let Expr::Ident(ns) = ast.get_expr(*ns_id)
        && ns == "Array"
    {
        if args.is_empty() {
            return Some(Err(
                "Array.fromAsync: expects at least 1 arg (the items source)".to_string(),
            ));
        }
        for &a in args {
            if let Err(e) = checker.type_of(ast, a) {
                return Some(Err(e));
            }
        }
        return Some(Ok(Type::Promise(Box::new(Type::Array(Box::new(
            Type::Any,
        ))))));
    }
    if let Expr::Member {
        obj: ns_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && m_name == "from"
        && let Expr::Ident(ns) = ast.get_expr(*ns_id)
        && ns == "Array"
    {
        if args.len() == 1 {
            let arg_ty = match checker.type_of(ast, args[0]) {
                Ok(t) => t,
                Err(e) => return Some(Err(e)),
            };
            if let Type::Array(elem) = &arg_ty {
                return Some(Ok(Type::Array(elem.clone())));
            }
            // S141 — `Array.from(set)` per ES §23.1.2.1 + §24.2.3.13
            // (Set iterator protocol yields values). Set storage
            // tags each entry as Any (untyped), so the result is
            // `Array<Any>` — print/spread/eq paths already handle
            // Any-tagged elements uniformly.
            if matches!(arg_ty, Type::Set) {
                return Some(Ok(Type::Array(Box::new(Type::Any))));
            }
            // `Array.from(map / m.keys() / m.values() / m.entries() /
            // set.values())` — the collection and its iterator cells
            // drive the same unified runtime iteration protocol as
            // `[...x]` spread; the materialized product is `Array<Any>`.
            if matches!(arg_ty, Type::Map | Type::MapIter | Type::ArrIter) {
                return Some(Ok(Type::Array(Box::new(Type::Any))));
            }
            // RFC 20260725-getiterator-getmethod knife 5 — an `any`
            // source is whatever it turns out to be at runtime, and
            // §7.4.2 GetIterator is what decides. The same unified
            // protocol `[...x]` already drives answers `Array<Any>`;
            // refusing it here was the last compile-time gate between
            // a user `@@iterator` and this consumption point.
            // A class instance is the ES §23.1.2.1 step 3 iterable
            // branch when it declares `@@iterator` (a generator object
            // always does) and the array-like branch when it carries a
            // `length` instead; ssa-lower routes on exactly that, and
            // both branches materialize `Array<Any>`.
            if matches!(arg_ty, Type::Any | Type::ClassRef(_)) {
                return Some(Ok(Type::Array(Box::new(Type::Any))));
            }
            // ES §23.1.2.1 step 3's non-iterable branch takes ANY
            // object, not just one wearing `length`: the walk asks for
            // `length`, and an object without one answers
            // ToLength(undefined) = 0, so `Array.from({a: 1})` is `[]`
            // rather than an error. A `{length: n}` struct is the same
            // branch with a length that happens to be there.
            if matches!(arg_ty, Type::Struct(_)) {
                return Some(Ok(Type::Array(Box::new(Type::Any))));
            }
            // B6 刀 3 — a nullish source is LEGAL to compile:
            // §23.1.2.1 step 5's GetMethod runs ToObject(items),
            // whose TypeError is a RUNTIME catchable (the
            // items-is-null-throws t262 shape asserts exactly that
            // throw), not a compile refusal.
            if matches!(arg_ty, Type::Null | Type::Undefined) {
                return Some(Ok(Type::Array(Box::new(Type::Any))));
            }
            // Fall through to the static-sig path; the String
            // overload already covered there throws a sensible
            // arity / type mismatch otherwise.
        } else if args.len() >= 2 {
            // S151 — `Array.from(iter, mapFn)` per ES §23.1.2.1.
            // The typed fast lane (String / typed Array / Set source
            // + Function mapFn, exactly 2 args) keeps its precise
            // `Array<R>` verdict for the devirt map loop; every
            // other admitted shape routes the any-tier kernel (B6
            // 刀 3 — `from_mapfn_routes_kernel`, the lowering keys
            // on the SAME predicate), where a non-callable mapfn is
            // the kernel's step-2 RUNTIME TypeError
            // (mapfn-is-not-callable / mapfn-is-symbol) and an
            // explicit thisArg really binds.
            let arg_ty = match checker.type_of(ast, args[0]) {
                Ok(t) => t,
                Err(e) => return Some(Err(e)),
            };
            let fn_ty = match checker.type_of(ast, args[1]) {
                Ok(t) => t,
                Err(e) => return Some(Err(e)),
            };
            for &a in &args[2..] {
                if let Err(e) = checker.type_of(ast, a) {
                    return Some(Err(e));
                }
            }
            if from_mapfn_routes_kernel(&arg_ty, &fn_ty, args.len()) {
                return Some(Ok(Type::Array(Box::new(Type::Any))));
            }
            let Type::Function(_, ret) = &fn_ty else {
                unreachable!("non-Function mapfn routes the kernel");
            };
            return Some(Ok(Type::Array(Box::new((**ret).clone()))));
        }
    }
    None
}
