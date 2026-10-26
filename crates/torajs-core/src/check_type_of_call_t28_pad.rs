//! T-28 — Default param missing → undefined wedge extracted
//! from [`crate::check_type_of_call::check`]'s post-cascade
//! generic-call mechanics (chunk 298 — ninetieth sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! Per ES §10.2.1.4: when fewer args are supplied than the
//! callee's declared params, JS sets the missing slots to
//! `undefined`. Only safe when the trailing missing slots
//! are all `Type::Any` (typed slots can't hold undefined);
//! typed missing params still surface as strict-arity errors
//! downstream. ssa_lower pads the missing positions with
//! `ANY_UNDEF` boxes at the call site, keyed off the
//! `arity_pad_count` entry the caller stashes.
//!
//! The supplied arguments are admitted against their parameters
//! as every other call's are (`check_type_of_call::general::args`):
//! the route, the interface witness flow and the face-slot census of
//! an object literal argument do not depend on whether a trailing
//! parameter was left out.

use crate::check::Type;

/// How many trailing parameters the call leaves to the undefined pad,
/// or `None` when it supplies them all or one left out cannot hold
/// undefined.
pub(crate) fn missing(params: &[Type], supplied: usize) -> Option<usize> {
    if supplied >= params.len() {
        return None;
    }
    // 424-01 — an optional trailing param (`(p?: number) =>`,
    // spelled Nullable) also binds undefined when the call omits it
    // (§10.2.1.4). Only the Nullable shapes that MATERIALIZE as Any
    // may take the ANY_UNDEF pad (`nullable_inner_boxes`'s scalar
    // set); a pointer-shaped Nullable slot carries a per-type
    // sentinel instead and keeps the strict-arity error (recorded
    // residual).
    let trailing_all_padable = params[supplied..].iter().all(|t| {
        matches!(t, Type::Any)
            || matches!(t, Type::Nullable(inner)
                if matches!(inner.as_ref(), Type::Number | Type::Boolean))
    });
    trailing_all_padable.then_some(params.len() - supplied)
}
