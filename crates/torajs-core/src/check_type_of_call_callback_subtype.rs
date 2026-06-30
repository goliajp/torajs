//! S133 callback Function subtype check extracted from
//! [`crate::check_type_of_call::check`]'s per-arg type-
//! check loop body (chunk 307 — ninety-ninth sub-batch
//! of check_type_of_call.rs per-shape decomposition).
//!
//! JS spec lets a callback accept fewer args than the
//! formal `Function` param declares (e.g. `Map.forEach`:
//! `(v) =>` is legal even though spec sig is
//! `(v, k, map) => void`). Strict equality on
//! `Type::Function` would reject any shorter callback.
//! Accept when:
//!
//! 1. Actual arity ≤ formal arity (callback may ignore
//!    trailing args)
//! 2. Return type matches, or either side is `Any` (user
//!    callback without type-ann defaults to `Any`, accepts
//!    against typed formal; formal `Any` accepts any typed
//!    actual)
//! 3. Every prefix slot matches with either side being
//!    `Any` (same widening as the return slot)
//!
//! Non-Function param / non-Function arg → false (the
//! caller falls through to strict-equality check).
//!
//! Pure: no mutation, no side effects. Returns `true` iff
//! the subtype carve-out applies.

use crate::check::Type;

pub(crate) fn matches(param_ty: &Type, arg_ty: &Type) -> bool {
    match (param_ty, arg_ty) {
        (Type::Function(formal_ps, formal_ret), Type::Function(actual_ps, actual_ret)) => {
            actual_ps.len() <= formal_ps.len()
                && (formal_ret.as_ref() == actual_ret.as_ref()
                    || matches!(formal_ret.as_ref(), Type::Any)
                    || matches!(actual_ret.as_ref(), Type::Any))
                && actual_ps
                    .iter()
                    .zip(formal_ps.iter())
                    .all(|(a, f)| a == f || matches!(f, Type::Any) || matches!(a, Type::Any))
        }
        _ => false,
    }
}
