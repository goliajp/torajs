//! `xs.concat(...)` Array-receiver multi-arg arm extracted
//! from [`crate::check_type_of_call::check`]'s top-level
//! cascade (chunk 251 — forty-fourth sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! V3-18 wedge — `Array.concat` accepts any number of array
//! args per JS spec §22.1.3.2:
//!   `xs.concat()`             → fresh shallow copy of xs
//!   `xs.concat(a, b, ..., z)` → fresh array of xs then a's
//!                                then b's ... then z's
//!
//! Pre-fix tora declared concat with a fixed 1-arg signature
//! so multi-arg calls failed at the unified arity check.
//! Subset constraint kept: every additional arg must be an
//! `Array<T>` with the same element type as the receiver, OR
//! a single `T` value (per ES §23.1.3.2 — spec "values are
//! added" path). Mixed shapes are valid:
//! `xs.concat([4,5], 6, [7,8])`. Heterogeneous-element
//! substrate isn't in tora yet so non-T non-Array<T> args
//! fall through to the strict table.
//!
//! Returns `Some(Ok(Type::Array(elem)))` when the receiver
//! is `Type::Array<T>` AND args.is_empty() OR every arg is
//! `Array<T>` or `T`. `Some(Err(_))` on recursive `type_of`
//! failure. `None` otherwise (non-concat callee, non-Array
//! receiver, or arg type mismatch — cascade falls through).

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn try_match(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Option<Result<Type, String>> {
    let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
    else {
        return None;
    };
    if m_name != "concat" {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    let Type::Array(elem) = &src_ty else {
        return None;
    };
    let expected = (**elem).clone();
    // 0-arg form: shallow copy of receiver. Skip arg-type
    // validation entirely.
    if args.is_empty() {
        return Some(Ok(Type::Array(Box::new(expected))));
    }
    // ES §23.1.3.2 — every arg is either an Array<T> (spread
    // into the result) or a single T value (appended as one
    // element). Mixed shapes are valid. An Any-elem receiver
    // accepts every value / array shape (the Any concat lane
    // NaN-boxes per slot), so no narrowing applies there.
    let recv_any = matches!(expected, Type::Any);
    let mut mixed = false;
    for a in args {
        let a_ty = match checker.type_of(ast, *a) {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        if recv_any {
            continue;
        }
        // Rotation 546 — an Any-typed argument makes the result
        // element set statically unknowable, so the answer is
        // Array<Any> and §23.1.3.1's spread-vs-append question moves
        // to runtime: the mixed lane routes it through the
        // is-array-testing kernel (`arr_concat_any_arg`).
        if matches!(a_ty, Type::Any) {
            mixed = true;
            continue;
        }
        let is_arr_t = a_ty == Type::Array(Box::new(expected.clone()));
        let is_scalar_t = a_ty == expected;
        if !is_arr_t && !is_scalar_t {
            mixed = true;
        }
    }
    if mixed {
        // Rotation 545 — §23.1.3.1: a statically-shaped argument
        // that diverges from the receiver's element type makes the
        // result element set heterogeneous, so the answer is
        // Array<Any>, not a refusal. The lowering reads this call
        // type back and routes the mixed lane (checker is the single
        // decision point — rotation 544's mixed-anchor lesson).
        return Some(Ok(Type::Array(Box::new(Type::Any))));
    }
    Some(Ok(Type::Array(Box::new(expected))))
}
