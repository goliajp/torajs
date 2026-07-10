//! Chunk 762 — struct-param Nullable-field covariance wedge for
//! [`crate::check_type_of_call::general`]'s per-arg type-check
//! loop.
//!
//! The let-decl lane admits `{ inner: { a: 3 } }` into a declared
//! `{ inner?: Inner }` slot through the assignability lattice
//! (`is_assignable_to_resolved`'s Struct arm recurses into the
//! Nullable field arm, which accepts `T` / `Undefined` / `Null`).
//! The call-arg lane fell through to strict equality and rejected
//! both the value-carrying and the explicit-undefined literal
//! shapes of the same pairing.
//!
//! Gated to Struct×Struct so every other param family keeps its
//! deliberately-loud strict check (Any→heap, scalar mismatches).
//!
//! Pure: no mutation, no side effects.
//! Returns `true` iff the covariance carve-out applies.

use crate::check::{Checker, Type};
use crate::check_assignable::is_assignable_to_resolved;

pub(crate) fn matches(checker: &Checker, param_ty: &Type, arg_ty: &Type) -> bool {
    matches!(param_ty, Type::Struct(_))
        && matches!(arg_ty, Type::Struct(_))
        && is_assignable_to_resolved(
            param_ty,
            arg_ty,
            &checker.aliases,
            &checker.generic_alias_decls,
        )
}
