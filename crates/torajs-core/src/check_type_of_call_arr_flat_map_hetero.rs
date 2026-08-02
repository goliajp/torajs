//! `Array<T>.flatMap(cb)` heterogeneous callback return wedge —
//! `.flatMap` on a typed array where cb's return `Array<U>` differs
//! from the receiver's element type `T[]` (e.g.
//! `numbers.flatMap(n => [n.toString()])` — bun answers a `string[]`,
//! pre-fix the checker's method-table entry declared flatMap's shape
//! as `((T) => T[]) => T[]` and rejected the shape outright).
//!
//! Accepts `(T) => Array<U>` for `U ∈ {Number, String, Boolean, Any}`
//! (the primitive lanes flat_map's dst-array-from-cb-sig lowering can
//! hold) and answers `Array<U>`. Homogeneous `(T) => T[]` stays with
//! the method-table arm.
//!
//! Sister to [`crate::check_type_of_call_arr_map_hetero`] — same
//! shape, just `Array<U>` return instead of scalar `U`. No lowering
//! change: ssa_lower's flat_map reads `dst_arr_ty` off the cb's
//! FnSig return (arr_flat_map.rs:64), which is already `Array<U>`
//! once the wedge admits the shape.
//!
//! Scalar `(T) => U` return (bun spec: single value acts like [U])
//! is not covered — the lowering assumes `inner_arr_ty = Array<_>`.
//! Deferred as L3b follow-up.

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
    if m_name != "flatMap" || args.len() != 1 {
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
    // Accept `(T) => Array<U>`, the `(any) => ...` widening
    // (contravariance), and the `(elem-typed) => ...` literal shape.
    // Anything else falls through to the strict method-table arm.
    if ps[0] != **elem && ps[0] != Type::Any && **elem != Type::Any {
        return None;
    }
    // Require the cb to return an Array; scalar returns aren't
    // covered by the current flat_map lowering (see file doc).
    let Type::Array(inner) = ret.as_ref() else {
        return None;
    };
    // Same-`T` inner keeps the method-table arm (its type spelling
    // stays `(T) => T[]`, which the checker's regular shape check
    // accepts more precisely).
    if **inner == **elem {
        return None;
    }
    // Primitive `U` lanes plus nested-array inners — an `Array<V>`
    // element is a pointer slot, which the flat_map walk already
    // handles exactly like Str (LoadDyn pointer + rc_inc + push).
    // Struct inners still need the struct-registry path map hetero
    // is missing.
    if !matches!(
        **inner,
        Type::Number | Type::String | Type::Boolean | Type::Any | Type::Array(_)
    ) {
        return None;
    }
    Some(Ok(Type::Array(inner.clone())))
}
