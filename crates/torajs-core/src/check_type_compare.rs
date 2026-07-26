//! Type-shape comparison helpers pulled out of [`crate::check`] as
//! chunk-320 of the check.rs god-file decomp.
//!
//! Two pure functions, no `Checker` state:
//!
//! - `unify_ternary` — V3-18 wedge: ternary branch unification. Returns
//!   the join type if `t` and `e` can unify (equal types, `T | null`
//!   widen, mixed-Any wedge S129-1), else `None`.
//! - `struct_is_prefix_subtype` — M5.2: structural-prefix subtyping for
//!   class-method receiver arguments. `Dog` (fields: name, bark_count)
//!   is a valid receiver for an `Animal`-typed method (fields: name)
//!   because the `name` field sits at the same offset in both layouts.
//!
//! Both grouped here as the "type-shape comparison" family. Re-exported
//! back into `crate::check` so callers
//! (`check_type_of_ternary::unify_ternary`,
//! `check_type_of_call_class_method_subtype::struct_is_prefix_subtype`)
//! keep their import paths.

use crate::check::Type;

pub(crate) fn unify_ternary(t: &Type, e: &Type) -> Option<Type> {
    if t == e {
        return Some(t.clone());
    }
    // S129-1 mixed-Any wedge — mirror of S128-5 LAnd/LOr / S127-4
    // indexOf 2-arg Any-escape. `b ? typed : (x: any)` widens the
    // result to Any; ssa-lower NaN-boxes the non-Any branch so the
    // shared __tern slot stays uniform.
    if matches!(t, Type::Any) || matches!(e, Type::Any) {
        return Some(Type::Any);
    }
    match (t, e) {
        // S2.27 (RFC 20260727-dstr-assignment 刀 5) — an Undefined
        // branch against any other type widens to Any, the S129-1
        // shape (ssa-lower boxes both branches into the shared
        // __tern slot; the Undefined side carries its ANY_UNDEF tag
        // through the expr-aware box). This is the destructuring
        // default's guard ternary over a MONOMORPHIC
        // `Array<Undefined>` source (`let [u = 13] = [undefined]`),
        // which used to reject with "branches differ". TS spells the
        // join `T | undefined`; Any is the same safe widening the
        // mixed-Any wedge already applies.
        (Type::Undefined, _) | (_, Type::Undefined) => Some(Type::Any),
        (Type::Null, other) | (other, Type::Null) => Some(Type::Nullable(Box::new(other.clone()))),
        (Type::Nullable(inner), other) | (other, Type::Nullable(inner)) => {
            if inner.as_ref() == other {
                Some(Type::Nullable(inner.clone()))
            } else {
                None
            }
        }
        _ => None,
    }
}

pub(crate) fn struct_is_prefix_subtype(arg: &Type, param: &Type) -> bool {
    match (arg, param) {
        (Type::Struct(arg_fields), Type::Struct(param_fields)) => {
            if param_fields.len() > arg_fields.len() {
                return false;
            }
            for (i, (pn, pt)) in param_fields.iter().enumerate() {
                let (an, at) = &arg_fields[i];
                if an != pn || at != pt {
                    return false;
                }
            }
            true
        }
        _ => false,
    }
}
