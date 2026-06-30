//! V3-18 Nullable<T> param match wedge extracted from
//! [`crate::check_type_of_call::check`]'s per-arg type-
//! check loop body (chunk 308 — hundredth sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! TS spec §3.9.2.4: an optional parameter widens to
//! `T | undefined`; the torajs subset models the optional
//! shape as `Type::Nullable<T>`. A `Nullable<T>` formal
//! must therefore accept both a `T`-typed actual and a
//! `Null` actual without dropping to the strict-equality
//! fallback.
//!
//! Non-Nullable param → false (the caller falls through to
//! strict-equality / S133 callback / class-method-prefix
//! checks).
//!
//! Pure: no mutation, no side effects.
//! Returns `true` iff the V3-18 carve-out applies.

use crate::check::Type;

pub(crate) fn matches(param_ty: &Type, arg_ty: &Type) -> bool {
    if let Type::Nullable(inner) = param_ty {
        arg_ty == &Type::Null || arg_ty == inner.as_ref()
    } else {
        false
    }
}
