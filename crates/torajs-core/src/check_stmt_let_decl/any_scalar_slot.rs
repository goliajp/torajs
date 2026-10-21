//! A `number` / `string` / `boolean` binding initialized with a read of
//! an `any` value.
//!
//! `const n: number = x` with `x: any` holding `"str"` is a legal program
//! whose annotation has no runtime meaning: §14.3.1.2 / §13.15.2 bind
//! the value as it is (§9.1.1.1.5 SetMutableBinding converts nothing),
//! so `n` is the string and `typeof n` is `"string"`. A typed slot would
//! have to convert it (ToNumber answered NaN), so the binding holds the
//! box from its declaration: its reads type `any` and take the any
//! lanes, whose number arms stay inline. The annotation is still the
//! binding's type for the TypeScript rules ([`held_decl`], kept on
//! the binding): what is
//! stored into it and what its reads flow into are checked against it.
//!
//! The mark is the cross-type widen set (`crate::let_widen`), keyed by
//! the initializer, so the checker, the local slot and the global slot
//! read one answer. An initializer that reads a held value counts
//! (`any_layout_slot::any_read`), and so does an `as any` cast, which
//! converts nothing either; a call is left out, since the typed
//! `JSON.parse` lanes decode theirs into the declared type.

use std::collections::HashMap;

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type as T};
use crate::ssa::Type;

/// Whether `ann` spells one of the scalar types this applies to.
pub(crate) fn scalar_ann(ann: &str) -> bool {
    matches!(ann, "number" | "string" | "boolean")
}

/// Whether a binding annotated `ann` holds the `any` value `init` reads.
pub(crate) fn takes_box(ann: &str, init: ExprId, ast: &Ast, types: &HashMap<ExprId, T>) -> bool {
    scalar_ann(ann)
        && (super::any_layout_slot::any_read(init, ast, types)
            || matches!(ast.get_expr(init), Expr::As { .. }) && types.get(&init) == Some(&T::Any))
}

/// The checker type of a binding annotated with the scalar `ann` whose
/// initializer is in the widen set (`widened`): `any`.
pub(crate) fn checker_face(ann: &T, widened: bool) -> Option<T> {
    (widened && matches!(ann, T::Number | T::String | T::Boolean)).then_some(T::Any)
}

/// The lowering mirror of [`checker_face`]: the slot the annotation
/// parsed to becomes an `any` slot.
pub(crate) fn value_repr(slot: &Type, widened: bool) -> Option<Type> {
    (widened && matches!(slot, Type::I64 | Type::F64 | Type::Bool | Type::Str)).then_some(Type::Any)
}

/// The type the TypeScript rules read a binding declared `ann` with
/// `init` as, when it holds a box (`super::held_any::face`): a scalar
/// annotation whose initializer is in the widen set, or, unannotated,
/// the type tsc infers from an initializer typed `any` only because it
/// reads a held value (`const t = n` with `n: number` holding one).
pub(crate) fn held_decl(
    checker: &Checker,
    ast: &Ast,
    (ann, init): (Option<&str>, ExprId),
    ty: &T,
) -> Option<T> {
    let Some(ann) = ann else {
        return (*ty == T::Any)
            .then(|| super::held_any::face::face(checker, ast, init))
            .flatten();
    };
    let declared = match ann {
        "number" => T::Number,
        "string" => T::String,
        "boolean" => T::Boolean,
        _ => return None,
    };
    checker
        .cross_type_widened
        .contains(&init)
        .then_some(declared)
}

/// A binding `name` whose reads the TypeScript rules read as `held`
/// ([`held_decl`]): noted, and when it is top-level, function bodies
/// reach it as a global, by that type.
pub(crate) fn note_held(checker: &mut Checker, name: &str, held: Option<&T>) {
    let Some(t) = held else {
        return;
    };
    if matches!(t, T::Number | T::String | T::Boolean) {
        checker.held_any.scalars = true;
    }
    if checker.scopes.len() == 1 {
        (checker.held_any.scalar_globals).insert(name.to_string(), t.clone());
    }
}
