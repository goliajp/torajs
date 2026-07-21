//! Generic `(Type::Object(_), …)` catch-all arms + the
//! `Type::Object("Symbol")` singleton accessor arms extracted
//! from [`crate::check_type_of_member::check`]'s top-level
//! `match (&obj_ty, name) { ... }` (chunk 204 — fourteenth
//! sub-batch of check_type_of_member.rs per-type-family
//! decomposition; mirrors chunks 191-203 try_match shape).
//!
//! Covers two themes that both gate on a `Type::Object(_)` /
//! `Type::Object("Symbol")` pre-match:
//!
//! V3-18 m2.b — Object.prototype methods exposed on every
//! constructor-namespace object (Number / String / Boolean /
//! Array / etc). Same subset semantics as m2.a on primitives:
//! hasOwnProperty / propertyIsEnumerable always false (no own
//! enum properties tracked), valueOf identity. plus V3-18 m2.c
//! → 2026-05-18 `.prototype` / `.name` / `.length` constructor
//! introspection (subset stubs):
//! - `hasOwnProperty` / `propertyIsEnumerable` →
//!   Function([String], Boolean)
//! - `isPrototypeOf` → Function([Any], Boolean)
//! - `toString` → Function([], String)
//! - `prototype` → Any (Type::Any so subsequent `.X` access
//!   routes through dynobj_get returning ANY_UNDEF for unknown
//!   fields; harmless when consumed by a verifyProperty-style
//!   stub. Pre-fix Type::Null blocked
//!   `verifyProperty(X.prototype.Y, ...)` — the dominant test262
//!   shape — at typecheck time. typeof X.prototype still works
//!   via the typeof-namespace-member arm.)
//! - `name` → String (the constructor's static name)
//! - `length` → Number (the constructor's static arity)
//!
//! T-13.c (v0.4.0) — well-known Symbol singletons.
//! Process-level lazy-init pointers; identity preserved across
//! all access sites. for-of dispatch via
//! `[Symbol.iterator]()` lands with v0.5 (iterator protocol
//! substrate):
//! - `iterator` / `asyncIterator` / `toPrimitive` → Symbol
//!
//! Returns `Some(Ok(_))` on hit, `None` when `(obj_ty, name)`
//! doesn't match any of the above. Catch-alls come first; the
//! Symbol-singleton arm only fires on `iterator` /
//! `asyncIterator` / `toPrimitive`, which don't overlap with
//! the catch-all names, so the ordering preserves the original
//! main-match semantics.

use crate::check::Type;

pub(crate) fn try_match(obj_ty: &Type, name: &str) -> Option<Result<Type, String>> {
    let ty = match (obj_ty, name) {
        (Type::Object(_), "hasOwnProperty") | (Type::Object(_), "propertyIsEnumerable") => {
            Type::Function(vec![Type::String], Box::new(Type::Boolean))
        }
        (Type::Object(_), "isPrototypeOf") => {
            Type::Function(vec![Type::Any], Box::new(Type::Boolean))
        }
        (Type::Object(_), "toString") => Type::Function(Vec::new(), Box::new(Type::String)),
        (Type::Object(_), "prototype") => Type::Any,
        (Type::Object(_), "name") => Type::String,
        (Type::Object(_), "length") => Type::Number,
        (
            Type::Object("Symbol"),
            "iterator" | "asyncIterator" | "toPrimitive" | "hasInstance" | "isConcatSpreadable"
            | "match" | "matchAll" | "replace" | "search" | "species" | "split" | "toStringTag"
            | "unscopables" | "dispose" | "asyncDispose",
        ) => Type::Symbol,
        _ => return None,
    };
    Some(Ok(ty))
}
