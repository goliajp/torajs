//! `Array<T>.map(cb)` heterogeneous callback return wedge —
//! `.map` on a typed array where cb's return `U` differs from
//! the receiver's element type `T` (e.g.
//! `numbers.map(n => n.toString())` — bun answers a `string[]`,
//! pre-fix the checker's method-table entry declared map's shape
//! as `((T) => T) => T[]` and rejected the shape outright).
//!
//! Accepts `(T) => U` for `U ∈ {Number, String, Boolean, Any}`
//! (the primitive lanes emit_map's dst array can hold plus the
//! Any-lane box path) and answers `Array<U>`. `Void` is not
//! matched here — the sister
//! [`crate::check_type_of_call_arr_pred_void_cb`] arm already
//! covers a Void-ret map by boxing `undefined` into an `Any[]`.
//! Homogeneous `(T) => T` stays with the method-table arm.
//!
//! ssa_lower's `emit_map` already reads `dst_arr_ty` off the
//! call result type (§20260706 M6.2 loop) and pushes through
//! `raw_slot_arg` — an F64 return bit-casts across, an owned
//! heap value passes through, so no lowering change is needed
//! for the primitive `U` variants covered here.

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
    if m_name != "map" || args.len() != 1 {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(_) => return None,
    };
    let Type::Array(elem) = &src_ty else {
        return None;
    };
    let cb_ty = match checker.type_of(ast, args[0]) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    let Type::Function(ps, ret) = &cb_ty else {
        return None;
    };
    if ps.len() != 1 {
        return None;
    }
    // Accept `(T) => U` (the elem match), the `(any) => U`
    // widening (contravariance), and the `(elem-typed) => U`
    // literal shape. Anything else falls through to the
    // strict method-table arm.
    if ps[0] != **elem && ps[0] != Type::Any && **elem != Type::Any {
        return None;
    }
    // Same-`T` return keeps the method-table arm (its type
    // spelling stays `(T) => T`, which the checker's regular
    // shape check accepts more precisely).
    if **ret == **elem {
        return None;
    }
    // Void ret is the sister wedge's territory (boxes `undefined`
    // into `Any[]`); stay out of its lane.
    if matches!(**ret, Type::Void) {
        return None;
    }
    // Only primitive `U` lanes have a matching `emit_map` dst
    // shape today (a heap-inner map — e.g. `numbers.map(x =>
    // ({v: x}))` returning `Struct[]` — needs a struct-registry
    // path the M6.2 lowering hasn't been widened to).
    if !matches!(
        **ret,
        Type::Number | Type::String | Type::Boolean | Type::Any
    ) {
        return None;
    }
    Some(Ok(Type::Array(ret.clone())))
}
