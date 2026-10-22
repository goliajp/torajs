//! A `Promise<T>` binding initialized with a promise of `any`.
//!
//! `const p: Promise<number> = Promise.resolve(x)` with `x: any` holding
//! `"s"` is a legal program whose annotation has no runtime meaning:
//! §27.2.4.7 settles the promise with `x` as it is, so `await p` is the
//! string. The settled value is not known statically, and code after an
//! `await` has no `any` version to fall back to at run time, so the
//! binding takes the face its initializer has: `Promise<any>`, whose
//! awaits and handlers read the value on the any lanes. The promise slot
//! holds every promise alike, so the lowering keeps it.
//!
//! The mark is the cross-type widen set (`crate::let_widen`), keyed by
//! the initializer, as for the scalar annotations
//! (`super::any_scalar_slot`).

use std::collections::HashMap;

use crate::ast::{Ast, ExprId};
use crate::ast_desugar_builtin_new::promise::context::promise_inner;
use crate::check::Type as T;
use crate::check_type_of_call_promise_then::{is_reject_mint, typed_lane};

/// Whether a binding annotated `ann` joins the widen set keyed by `init`:
/// a scalar annotation over a read of an `any` value holds the box
/// (`super::any_scalar_slot`), a promise annotation takes [`takes_any`].
pub(crate) fn widens(ann: &str, init: ExprId, ast: &Ast, types: &HashMap<ExprId, T>) -> bool {
    super::any_scalar_slot::takes_box(ann, init, ast, types) || takes_any(ann, init, ast, types)
}

/// Whether a binding annotated `ann` takes the face of `init`: the
/// annotation names a `Promise<T>` with a `T` other than `any`, and the
/// initializer types `Promise<any>` because it settles a value of any
/// kind. A `Promise.reject(r)` types `Promise<any>` too, as the `never`
/// a rejected promise fulfils with: the annotation names it truly.
fn takes_any(ann: &str, init: ExprId, ast: &Ast, types: &HashMap<ExprId, T>) -> bool {
    promise_inner(ann).is_some_and(|i| i != "any")
        && types.get(&init).is_some_and(untyped)
        && !is_reject_mint(ast, init)
}

/// A promise whose settled value is unknown.
fn untyped(t: &T) -> bool {
    matches!(t, T::Promise(inner) if **inner == T::Any)
}

/// The checker type of a binding annotated `ann` whose initializer is
/// in the widen set (`widened`): the initializer's `Promise<any>`. An
/// annotation whose `T` takes every value on the any lane (a union, a
/// nullable) reads the settled value as it is already, and keeps its
/// type.
pub(crate) fn checker_face(
    ann: &T,
    init: ExprId,
    types: &HashMap<ExprId, T>,
    widened: bool,
) -> Option<T> {
    (widened
        && matches!(ann, T::Promise(i) if typed_lane(i))
        && types.get(&init).is_some_and(untyped))
    .then(|| T::Promise(Box::new(T::Any)))
}
