//! M5.2 class-method receiver subclass prefix-subtype
//! check extracted from [`crate::check_type_of_call::check`]'s
//! per-arg type-check loop body (chunk 309 — hundred-and-
//! first sub-batch of check_type_of_call.rs per-shape
//! decomposition).
//!
//! Class-method dispatch: `arg[0]` is the receiver and may
//! be a SUBCLASS of the declared param type (structural
//! super-set: subclass struct's fields are a prefix-extension
//! of the parent's). The SSA / LLVM layer treats both as
//! `ptr`, so the call is correct as long as the layout
//! prefix matches. The strict-equality check is then
//! skipped at the call site.
//!
//! Non-class-method, non-zero arg index, or arg type that
//! is not a struct prefix subtype of the param type → false
//! (the caller falls through to the strict-equality /
//! V3-18 Nullable / S133 callback checks).
//!
//! Pure: no mutation, no side effects.
//! Returns `true` iff the strict-equality check at the
//! call site should be skipped.

use crate::check::{Type, struct_is_prefix_subtype};

pub(crate) fn skip(is_class_method: bool, i: usize, arg_ty: &Type, param_ty: &Type) -> bool {
    is_class_method && i == 0 && struct_is_prefix_subtype(arg_ty, param_ty)
}
