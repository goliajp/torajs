//! The element slots of the census
//! ([`crate::ast::face_slots::FaceSlots::elems`]): an array of a
//! function type some store handed a function value of another face.
//!
//! `hs[0] = f` with `f: any` into `((x: number) => number)[]` leaves a
//! function compiled in its own face (`(x: any) => any`) where the
//! element type promises the typed one, so a direct call through the
//! element passed an f64 the body read as a box. A call through such an
//! element rides the boxed entry instead, whose adapter unboxes per the
//! stored value's own face, and the checker types it `any`: what the
//! stored function returns is not known (`check_type_of_call::fn_slot_ret`).
//!
//! The key is the element type's spelling, not the binding: an array
//! value moves between bindings of its type (a parameter, a field, a
//! returned value), and each of them reads the elements it holds.

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

/// Is `elem` a function element type, `undefined` or `null` allowed
/// (`(F | undefined)[]`)? A store compares faces past the nullable
/// wrapper (`faces_differ`), so the census key does too.
pub(crate) fn is_fn_elem(elem: &Type) -> bool {
    matches!(strip_nullable(elem), Type::Function(..))
}

fn strip_nullable(t: &Type) -> &Type {
    match t {
        Type::Nullable(inner) => inner.as_ref(),
        other => other,
    }
}

/// The census key of a function element type `elem`: the function
/// type's spelling, so `F[]` and `(F | undefined)[]` share it.
fn key_of(elem: &Type) -> Option<String> {
    is_fn_elem(elem).then(|| crate::check::type_to_ann(strip_nullable(elem)))
}

/// Does an array typed `arr` hold such a value in its elements? The
/// lowering's question, asked of the census the checker left.
pub(crate) fn elem_hit(census: &crate::ast::face_slots::FaceSlots, arr: &Type) -> bool {
    !census.elems.is_empty()
        && matches!(arr, Type::Array(elem) if key_of(elem).is_some_and(|k| census.elems.contains(&k)))
}

/// The census key of an array typed `arr` when its elements hold such
/// a value; a miss is remembered.
pub(crate) fn elem_read(checker: &mut Checker, arr: &Type) -> Option<String> {
    let Type::Array(elem) = arr else {
        return None;
    };
    let key = key_of(elem)?;
    if checker.face_slots.elems.contains(&key) {
        return Some(key);
    }
    super::remember(&mut checker.face_slot_reads.elems, &key);
    None
}

/// The array expression whose element `e` hands over: an index
/// (`hs[i]`), a method that returns an element (`hs.at(i)`, `pop`,
/// `shift`, `find`, `findLast`) or may return one its accumulator held
/// (`reduce`, `reduceRight`), each also behind a non-null assertion.
pub(crate) fn elem_source(ast: &Ast, e: ExprId) -> Option<ExprId> {
    match ast.get_expr(e) {
        Expr::Index { obj, .. } => Some(*obj),
        Expr::As { expr, ty_ann } if ty_ann == "__nonnull__" => elem_source(ast, *expr),
        Expr::Call { callee, .. } => match ast.get_expr(*callee) {
            Expr::Member { obj, name }
                if matches!(
                    name.as_str(),
                    "at" | "pop" | "shift" | "find" | "findLast" | "reduce" | "reduceRight"
                ) =>
            {
                Some(*obj)
            }
            _ => None,
        },
        _ => None,
    }
}

/// The census key of the array a function value `e` was read out of
/// ([`elem_source`]), when its elements hold such a value.
pub(crate) fn elem_value(checker: &mut Checker, ast: &Ast, e: ExprId) -> Option<String> {
    let obj = elem_source(ast, e)?;
    if !checker.expr_types.get(&e).is_some_and(is_fn_elem) {
        return None;
    }
    let arr = checker.expr_types.get(&obj)?.clone();
    elem_read(checker, &arr)
}

/// A store of `value_ty` into an element of an array of `elem_ty`.
pub(crate) fn record_elem_store(checker: &mut Checker, elem_ty: &Type, value_ty: &Type) {
    if !super::faces_differ(value_ty, elem_ty) {
        return;
    }
    let Some(key) = key_of(elem_ty) else {
        return;
    };
    if !checker.face_slots.elems.contains(&key) {
        checker.face_slot_reads.stale |= checker.face_slot_reads.elems.contains(&key);
        checker.face_slots.elems.insert(key);
    }
}

/// The elements of the array literal `value` admitted into a slot typed
/// `slot_ty`; a spread element hands over values already in an array.
pub(crate) fn record_arrlit_stores(
    checker: &mut Checker,
    ast: &Ast,
    slot_ty: &Type,
    value: ExprId,
) {
    let (Type::Array(elem), Expr::Array(els)) = (slot_ty, ast.get_expr(value)) else {
        return;
    };
    if key_of(elem).is_none() {
        return;
    }
    for &el in els {
        // a spread hands over the elements of its source
        let (src, spread) = match ast.get_expr(el) {
            Expr::Spread { expr } => (*expr, true),
            _ => (el, false),
        };
        let value_ty = match checker.expr_types.get(&src).cloned() {
            Some(Type::Array(t)) if spread => *t,
            Some(t) if !spread || t == Type::Any => t,
            _ => continue,
        };
        record_elem_store(checker, elem, &value_ty);
    }
}
