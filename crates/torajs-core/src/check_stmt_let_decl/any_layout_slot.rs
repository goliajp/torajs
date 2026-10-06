//! An `any` element in a struct-array literal.
//!
//! `const xs: A[] = [{ … }, v]` with `v: any` is a legal program whose
//! annotation has no runtime meaning: `v` is whatever it holds — a
//! dynamic object, a struct of another layout, a number. A struct
//! array reads every element at the declared field offsets, so the box
//! stored there read a dynamic object's header as fields (garbage, or
//! exit 139). Copying the fields out would cut the element off from
//! `v`, and the two must stay one object, as they are in JS.
//!
//! So a literal with an element whose type is unknown keeps the `any[]`
//! type its inference gave it — in the checker and in the lowering
//! alike — and so does a binding declared with it, or a mutable one a
//! later assignment hands such a literal (`crate::let_widen`). Element
//! reads go through the dynamic lane, which answers per the value's
//! real shape; a call passing an element to an `A` parameter takes the
//! any lane into the callee. The decision is per element: a literal
//! whose elements are all typed — even of different struct shapes
//! (optional fields, another field order, a subclass first) — keeps
//! the typed layout its annotation gives it.

use crate::ast::{Ast, Expr, ExprId};
use crate::check::Type as T;
use crate::ssa::Type;
use std::collections::HashMap;

/// Whether `lit` is an array literal with an element of unknown type:
/// one typed `any` (an `any` value, or an `as` cast of one), or a
/// spread of an `any` / `any[]` source.
pub(crate) fn has_dynamic_elem(lit: ExprId, ast: &Ast, types: &HashMap<ExprId, T>) -> bool {
    let Expr::Array(els) = ast.get_expr(lit) else {
        return false;
    };
    let any_arr = T::Array(Box::new(T::Any));
    els.iter().any(|&el| match ast.get_expr(el) {
        Expr::Spread { expr } => {
            matches!(types.get(expr), Some(t) if *t == T::Any || *t == any_arr)
        }
        _ => types.get(&el) == Some(&T::Any),
    })
}

/// Whether `ann` is an array of a struct layout.
pub(crate) fn struct_elem_array(ann: &T) -> bool {
    matches!(ann, T::Array(el) if matches!(**el, T::Struct(_) | T::ClassRef(_)))
}

/// The checker type of a binding declared `ann`: [`checker_face`]'s
/// answer, or the annotation itself.
pub(crate) fn declared_face(checker: &crate::check::Checker, ast: &Ast, init: ExprId, ann: T) -> T {
    let widened = checker.cross_type_widened.contains(&init);
    checker_face(&ann, init, ast, &checker.expr_types, widened).unwrap_or(ann)
}

/// The checker type of a binding annotated `ann` — an array of a
/// struct layout — whose initializer has an `any` element, or which
/// a later assignment widened (`widened`); `None` keeps the annotation.
fn checker_face(
    ann: &T,
    init: ExprId,
    ast: &Ast,
    types: &HashMap<ExprId, T>,
    widened: bool,
) -> Option<T> {
    (struct_elem_array(ann) && (widened || has_dynamic_elem(init, ast, types)))
        .then(|| T::Array(Box::new(T::Any)))
}

/// Whether `init` is `v as T` with `v: any`.
fn any_cast(init: ExprId, ast: &Ast, types: &HashMap<ExprId, T>) -> bool {
    matches!(ast.get_expr(init), Expr::As { expr, .. } if types.get(expr) == Some(&T::Any))
}

/// The checker type of a binding of struct type `ty` whose initializer
/// is `v as A` with `v: any`: the cast keeps the box (an `as` converts
/// nothing), so the binding is `any`, with or without an annotation,
/// as the lowering binds it — `None` keeps `ty`.
pub(crate) fn cast_face(ty: &T, init: ExprId, ast: &Ast, types: &HashMap<ExprId, T>) -> Option<T> {
    (matches!(ty, T::Struct(_) | T::ClassRef(_)) && any_cast(init, ast, types)).then_some(T::Any)
}

/// The lowering mirror of [`checker_face`]: the SSA type a binding
/// whose annotation parsed to `slot` takes for `init` — `None` keeps
/// the annotation's layout.
pub(crate) fn value_repr(
    slot: &Type,
    init: ExprId,
    ast: &Ast,
    types: &HashMap<ExprId, T>,
    widened: bool,
    arr_layouts: &mut Vec<Type>,
) -> Option<Type> {
    if matches!(slot, Type::Obj(_)) && any_cast(init, ast, types) {
        return Some(Type::Any);
    }
    let Type::Arr(id) = slot else { return None };
    (matches!(arr_layouts[id.0 as usize], Type::Obj(_))
        && (widened || has_dynamic_elem(init, ast, types)))
    .then(|| Type::Arr(crate::ssa_lower::intern_arr_layout(arr_layouts, Type::Any)))
}
