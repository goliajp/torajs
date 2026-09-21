//! The promise reaction arms of cascade segment 1 — `.then` /
//! `.catch` / `.finally`. Split out of `route_early` when the
//! §27.2.5.4 empty-slot arm pushed that dispatcher past the
//! 200-line function limit (file-size HARD RULE, "move before you
//! add"). The group still runs at exactly its old position in the
//! cascade and the arms keep their order relative to each other;
//! nothing else moved.

use crate::ast::{Ast, ExprId};
use crate::check::{Checker, Type};

pub(super) fn try_route(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Option<Result<Type, String>> {
    // Promise<T>.then / .catch early-route arms (T-19.l 2-arg
    // shape, T-19.o heterogeneous T→U + P10.7 Promise<Any>,
    // P10.2-A1.1 Promise<Undefined>, P10.2-A4 Promise<Array<U>>)
    // — see [`crate::check_type_of_call_promise_then`] (chunk
    // 207 — first sub-batch of check_type_of_call.rs per-shape
    // decomposition). All 4 patterns must run BEFORE the
    // regular method-table dispatch because the table's
    // static signature fixes arg count and inner-T constraint.
    // `.finally` is not handled there — its `() => void` shape is
    // the table arm's, and every OTHER return shape is the arm
    // below's (§27.2.5.3 declares `onFinally` as `() => any`).
    // §27.2.5.4 step 3/4 first: a handler that is not statically a
    // function is an EMPTY slot per spec, and the typed arms below
    // can only express a matching signature. See
    // [`crate::check_type_of_call_promise_any_cb`].
    if let Some(r) = crate::check_type_of_call_promise_any_cb::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    if let Some(r) = crate::check_type_of_call_promise_then::try_match(checker, ast, callee, args) {
        return Some(r);
    }
    if let Some(r) =
        crate::check_type_of_call_promise_finally::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    None
}
