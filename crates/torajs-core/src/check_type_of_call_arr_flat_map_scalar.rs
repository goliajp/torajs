//! `Array<T>.flatMap(cb)` scalar callback return wedge — `.flatMap`
//! where cb returns a scalar `U` per ES §23.1.3.11 spec step 8.d
//! (a non-Array result of `Call(mapperFunction, ...)` gets pushed
//! as-if `[U]` into the accumulator).
//!
//! Sister to [`crate::check_type_of_call_arr_flat_map_hetero`]:
//! same shape-detection cascade, but the callback return is a
//! primitive `U` instead of `Array<U>`. Answers `Array<U>` — the
//! flat_map lowering's scalar arm allocates `Arr<U>` and pushes the
//! scalar result each outer iteration (no inner walk).
//!
//! Admits `(T) => U` for `U ∈ {Number, String, Boolean, Any}` — the
//! primitive lanes the scalar-arm dst_arr layout can hold. Object /
//! Struct scalar returns take the wider `(T) => any` path (a struct
//! upcast to any lands in the `U = Any` bucket, which is answered
//! `Array<Any>`).
//!
//! Homogeneous `(T) => T[]` (Array-return) stays with the
//! method-table arm; `(T) => Array<U>` heterogeneous lands with the
//! sibling hetero wedge. This scalar wedge sits AFTER the hetero
//! arm in the check_type_of_call cascade so `(T) => Array<U>`
//! decisions never accidentally fall through to scalar.

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
    // Full §23.1.3 spec arity — (elem, index, srcArray); the trailing
    // slots are seed/promote-shaped so only the elem slot gates
    // (pred_void_cb's stance).
    if ps.is_empty() || ps.len() > 3 {
        return None;
    }
    // Accept `(T) => U` (contravariant `(any) => U` and literal
    // `(elem) => U` both admitted — mirrors the sibling hetero
    // wedge's param cascade).
    if ps[0] != **elem && ps[0] != Type::Any && **elem != Type::Any {
        return None;
    }
    // Scalar return only — `Array<_>` returns route to the sibling
    // hetero wedge / method-table arm above.
    if matches!(ret.as_ref(), Type::Array(_)) {
        return None;
    }
    // A value-less callback answers `undefined` per §23.1.3.11 step
    // 8.d (a non-Array result is pushed as-if `[U]`), so the product
    // is an array of undefineds — typed `Array<Any>` because the
    // lowering's scalar arm pushes the boxed-undefined sentinel into
    // the Any dst flavor (there is no dedicated undefined-elem
    // layout).
    if matches!(ret.as_ref(), Type::Void | Type::Undefined) {
        return Some(Ok(Type::Array(Box::new(Type::Any))));
    }
    // Only primitive `U` lanes have a matching dst arr flavor today
    // (matches the sibling hetero's set — heap-inner scalar U needs
    // the same struct-registry path).
    if !matches!(
        ret.as_ref(),
        Type::Number | Type::String | Type::Boolean | Type::Any
    ) {
        return None;
    }
    Some(Ok(Type::Array(ret.clone())))
}
