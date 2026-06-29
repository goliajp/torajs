//! Mixed-primitive ∪ Any union arms extracted from
//! [`crate::check_type_of_member::check`]'s top-level
//! `match (&obj_ty, name) { ... }` (chunk 205 — fifteenth
//! sub-batch of check_type_of_member.rs per-type-family
//! decomposition; mirrors chunks 191-204 try_match shape).
//!
//! Covers the cross-type-family arms whose `|`-union pattern
//! spans more than one primitive (and in one case includes
//! `Type::Any`), so they can't live in any single primitive's
//! dedicated sibling (chunks 194 prim / 196 array / 197 string
//! etc.):
//!
//! - `(Type::String, "length") | (Type::Array(_), "length")`
//!   → Number (shared by both heap-backed primitives; for
//!   Type::String this hits even though chunk 197 owns the
//!   String method table — `length` is an accessor not a
//!   method, and the Array(_) tail makes the union cross-
//!   family).
//! - `(Type::Number | Type::String | Type::Boolean |
//!    Type::BigInt | Type::Symbol, "constructor")` → Any
//!   (V3-18 m2.c — primitive `.constructor` returns the
//!   wrapper constructor function; subset stub Type::Any
//!   since tora has no first-class function reference for
//!   the namespace ctor).
//! - `(Type::Number | Type::String | Type::Boolean |
//!    Type::BigInt | Type::Symbol | Type::Any,
//!    "hasOwnProperty" | "propertyIsEnumerable")` →
//!   Function([String], Boolean) (V3-18 m2.a — Object.prototype
//!   methods exposed on every primitive via JS's auto-boxing
//!   rules; subset always returns false at lower time via
//!   constant-fold).
//!
//! The Array-as-Object catch-all
//! `(Type::Array(_), name) if name != "length" &&
//!  !is_array_method_name(name)` stays in the main match —
//! it has a guard condition that requires the `is_array_method_name`
//! check helper resolved against `crate::check` (a guard
//! makes the arm unsuitable for the flat-`match` form here).
//!
//! Returns `Some(Ok(_))` on hit, `None` when `(obj_ty, name)`
//! doesn't match any of the above.

use crate::check::Type;

pub(crate) fn try_match(obj_ty: &Type, name: &str) -> Option<Result<Type, String>> {
    let ty = match (obj_ty, name) {
        (Type::String, "length") | (Type::Array(_), "length") => Type::Number,
        (
            Type::Number | Type::String | Type::Boolean | Type::BigInt | Type::Symbol,
            "constructor",
        ) => Type::Any,
        (
            Type::Number | Type::String | Type::Boolean | Type::BigInt | Type::Symbol | Type::Any,
            "hasOwnProperty" | "propertyIsEnumerable",
        ) => Type::Function(vec![Type::String], Box::new(Type::Boolean)),
        _ => return None,
    };
    Some(Ok(ty))
}
