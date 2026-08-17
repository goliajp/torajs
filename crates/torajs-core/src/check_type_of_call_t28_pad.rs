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
//! `arity_pad_count` entry stashed here.
//!
//! Returns:
//! - `Some(Ok(ret))` — wedge fired (trailing all-Any), each
//!   supplied arg type_of'd, `arity_pad_count[eid]` set to
//!   the missing count, and the call typechecks to `ret`
//!   (cloned from the callee's return type)
//! - `Some(Err(_))` — any supplied arg's `type_of` failed
//! - `None` — wedge did not fire (effective_args.len() >=
//!   params.len(), or trailing slots are not all Type::Any);
//!   caller falls through to the Date setter / splice
//!   defaults and the strict-arity check that follows

use crate::ast::{Ast, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn try_pad(
    checker: &mut Checker,
    ast: &Ast,
    eid: ExprId,
    params: &[Type],
    effective_args: &[ExprId],
    ret: &Type,
) -> Option<Result<Type, String>> {
    if effective_args.len() >= params.len() {
        return None;
    }
    // 424-01 — an optional trailing param (`(p?: number) =>`,
    // spelled Nullable) also binds undefined when the call omits it
    // (§10.2.1.4). Only the Nullable shapes that MATERIALIZE as Any
    // may take the ANY_UNDEF pad (`nullable_inner_boxes`'s scalar
    // set); a pointer-shaped Nullable slot carries a per-type
    // sentinel instead and keeps the strict-arity error (recorded
    // residual).
    let trailing_all_padable = params[effective_args.len()..].iter().all(|t| {
        matches!(t, Type::Any)
            || matches!(t, Type::Nullable(inner)
                if matches!(inner.as_ref(), Type::Number | Type::Boolean))
    });
    if !trailing_all_padable {
        return None;
    }
    for arg_id in effective_args.iter() {
        if let Err(e) = checker.type_of(ast, *arg_id) {
            return Some(Err(e));
        }
    }
    checker
        .arity_pad_count
        .insert(eid, params.len() - effective_args.len());
    Some(Ok(ret.clone()))
}
