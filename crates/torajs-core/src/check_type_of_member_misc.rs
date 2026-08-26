//! `Type::Struct` / `Type::Function` / `Type::Any` single-type
//! instance-method arms extracted from
//! [`crate::check_type_of_member::check`]'s top-level
//! `match (&obj_ty, name) { ... }` (chunk 199 — ninth sub-batch
//! of check_type_of_member.rs per-type-family decomposition;
//! mirrors chunks 191-198 try_match shape).
//!
//! Covers:
//! - `Type::Struct(_)` Object.prototype methods (5 arm groups:
//!   hasOwnProperty / propertyIsEnumerable / isPrototypeOf /
//!   valueOf / toString / constructor — V3-18 m2.d)
//! - `Type::Function(..)` built-in members (length / name +
//!   T-27 catch-all `(Type::Function(..), _) → Type::Any`)
//! - `Type::Any` single-type arms (valueOf / toString /
//!   isPrototypeOf / constructor + P3.2 catch-all
//!   `(Type::Any, _) → Type::Any`)
//!
//! Mixed-type arms involving these types (prim+Any union
//! hasOwnProperty / propertyIsEnumerable for instance) stay in
//! the main match.
//!
//! Returns `Some(Ok(_))` on hit, `None` when `(obj_ty, name)`
//! doesn't match. Per-type catch-alls (`(Type::Any, _)` and
//! `(Type::Function(..), _)`) live at the bottom of the
//! per-type `match` block so explicit arms still get priority.

use crate::check::Type;

pub(crate) fn try_match(obj_ty: &Type, name: &str) -> Option<Result<Type, String>> {
    let ty = match (obj_ty, name) {
        // V3-18 m2.d — class-instance Object.prototype
        // methods. Same shape as namespace ctors:
        //   .hasOwnProperty(k)         → true if k is a
        //                                 declared field
        //                                 (compile-time
        //                                 layout lookup).
        //   .propertyIsEnumerable(k)   → same as
        //                                 hasOwnProperty
        //                                 (instance fields
        //                                 are enumerable).
        //   .isPrototypeOf(x)          → false (no real
        //                                 prototype chain).
        //   .valueOf()                 → identity (the
        //                                 instance).
        //   .toString()                → "[object Object]"
        //                                 (subset stub).
        //   .constructor                → Type::Any.
        (Type::Struct(_), "hasOwnProperty" | "propertyIsEnumerable") => {
            Type::Function(vec![Type::String], Box::new(Type::Boolean))
        }
        (Type::Struct(_), "isPrototypeOf") => {
            Type::Function(vec![Type::Any], Box::new(Type::Boolean))
        }
        (Type::Struct(_), "valueOf") => Type::Function(Vec::new(), Box::new(obj_ty.clone())),
        (Type::Struct(_), "toString") => Type::Function(Vec::new(), Box::new(Type::String)),
        (Type::Struct(_), "constructor") => Type::Any,
        // T-27.c — built-in `length` (Number) and `name`
        // (String) on a Function. length is the param
        // count; name is the lifted FnDecl's name. Both
        // are compile-time constants known from the fn's
        // static signature, so ssa_lower can fold them
        // without runtime dispatch.
        (Type::Function(_, _), "length") => Type::Number,
        (Type::Function(..), "name") => Type::String,
        // RFC 20260721 刀 9 — `fun.prototype` on a fn-typed binding:
        // Any (plain fns materialize the §10.2.5 object at runtime,
        // arrows / async forms read undefined — the static type
        // can't distinguish the flavors).
        (Type::Function(..), "prototype") => Type::Any,
        // RFC 20260721 刀 4 — `fun.constructor`: Any (the runtime
        // kernel keys %Function% vs %AsyncFunction% off the cell's
        // flavor bit).
        (Type::Function(..), "constructor") => Type::Any,
        // Type::Any single-type members. These explicit arms
        // carry richer shape than the P3.2 catch-all
        // `(Type::Any, _) → Type::Any` and so live above it.
        // ssa_lower handles dispatch via dynobj_get_tag/value
        // at runtime.
        //
        // valueOf and toString are deliberately NOT here. On an
        // Any receiver nothing is known about them statically:
        // an own entry can shadow the prototype with a
        // non-callable (`{ toString: undefined }`), so promising
        // Function is a claim the checker cannot back. It also
        // cost bun parity — a Function-typed member read binds
        // to a value ssa_lower cannot materialise, so
        // `const m = o.toString; m()` failed to lower, and
        // `o.toString[0]` was rejected outright. Both run under
        // bun. They fall to the Any catch-all below, which is
        // the same path every other Any member already takes.
        (Type::Any, "isPrototypeOf") => Type::Function(vec![Type::Any], Box::new(Type::Boolean)),
        (Type::Any, "constructor") => Type::Any,
        // P3.2 — Member access on Type::Any returns Type::Any.
        // Static layout unknown at compile time; ssa_lower
        // routes through dynobj_get_tag/value. Missing
        // properties read as undefined per spec.
        (Type::Any, _) => Type::Any,
        // T-27 — Function-as-Object reads. Per ECMAScript
        // §10.2 functions are objects. `f.x` on a closure
        // takes the any-member lane (r505): own expando,
        // then the [[Prototype]] chain, then undefined.
        // Other built-in props (.bind, .call, .apply,
        // .toString, etc.) are L3b T-27.c-rest — not
        // implemented; currently return undefined.
        (Type::Function(..), _) => Type::Any,
        _ => return None,
    };
    Some(Ok(ty))
}
