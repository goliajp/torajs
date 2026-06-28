//! `Type::Promise` instance-method arms extracted from
//! [`crate::check_type_of_member::check`]'s top-level
//! `match (&obj_ty, name) { ... }` (chunk 198 — eighth sub-batch
//! of check_type_of_member.rs per-type-family decomposition;
//! mirrors chunks 191-197 try_match shape).
//!
//! Covers the three `(Type::Promise(inner), …)` arms:
//! - `.then(cb)` — cb sig `(v: T) => T`; returns `Promise<T>`
//! - `.catch(cb)` — cb sig `(reason: T) => T`; returns `Promise<T>`
//! - `.finally(cb)` — cb sig `() => void`; returns `Promise<T>`
//!
//! `.then` and `.catch` are gated on `T ∈ Number | String |
//! Boolean | Any` — the i64-roundtrippable primitive set the
//! `__torajs_promise_then_simple` / `_catch_simple` runtime
//! helpers pack through. Heap T (Array / Struct / Date) lands
//! with the closure-cb substrate (T-15.g.5+).
//!
//! The pre-match dispatch at the head of
//! `check_type_of_member::check` for `if let Type::Promise(inner)
//! = &obj_ty` (the structural getter forwarding) stays in the
//! main module — it doesn't drive a method arm.
//!
//! Returns `Some(Ok(_))` on hit, `None` when `(obj_ty, name)`
//! doesn't match.

use crate::check::Type;

pub(crate) fn try_match(obj_ty: &Type, name: &str) -> Option<Result<Type, String>> {
    let ty = match (obj_ty, name) {
        /* T-15.g.3 / T-19.g (v0.5.0) — `Promise<T>.then(cb)`
         * chains. cb signature is `(v: T) => T` (same T
         * in/out — no generic U yet). T ∈ Number / String
         * / Boolean — the three i64-roundtrippable
         * primitives the runtime helper
         * `__torajs_promise_then_simple` packs through.
         * Heap T (Array / Struct / Date) deferred to
         * T-15.g.5+ alongside the closure-cb substrate.
         *
         * T-19.k (v0.5.0) — `Promise<T>.catch(onRejected)`.
         * cb sig is `(reason: T) => T` — same shape as
         * .then's onFulfilled. Returns a Promise<T> that
         * resolves with cb's return value on rejection,
         * or passes through source's value on fulfillment.
         * Same T scope as .then.
         *
         * P10.7 — `Promise<Any>` participates same as
         * the i64-roundtrippable primitives: cb is
         * `(v: Any) => Any`; the existing runtime helpers
         * take / return i64 (NaN-box AnyValue at the SSA
         * layer is i64-sized). */
        (Type::Promise(inner), "then" | "catch")
            if matches!(
                **inner,
                Type::Number | Type::String | Type::Boolean | Type::Any
            ) =>
        {
            let elem = (**inner).clone();
            Type::Function(
                vec![Type::Function(vec![elem.clone()], Box::new(elem.clone()))],
                Box::new(Type::Promise(Box::new(elem))),
            )
        }
        /* T-19.k — `Promise<T>.finally(onFinally)`. cb sig
         * is `() => void` per spec — no value passed in,
         * cb's return ignored. Returns a Promise<T> with
         * the same state + value as the source (after
         * cb runs). cb runs on either settled state. */
        (Type::Promise(inner), "finally") => Type::Function(
            vec![Type::Function(vec![], Box::new(Type::Void))],
            Box::new(Type::Promise(inner.clone())),
        ),
        _ => return None,
    };
    Some(Ok(ty))
}
