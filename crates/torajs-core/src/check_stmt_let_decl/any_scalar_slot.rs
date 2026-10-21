//! A `number` / `string` / `boolean` binding initialized with a read of
//! an `any` value.
//!
//! `const n: number = x` with `x: any` holding `"str"` is a legal program
//! whose annotation has no runtime meaning: §14.3.1.2 / §13.15.2 bind
//! the value as it is (§9.1.1.1.5 SetMutableBinding converts nothing),
//! so `n` is the string and `typeof n` is `"string"`. A typed slot would
//! have to convert it (ToNumber answered NaN), so the binding holds the
//! box from its declaration: its reads type `any` and take the any
//! lanes, whose number arms stay inline. The annotation still checks
//! what is stored into the binding later ([`declared_store`]).
//!
//! The mark is the cross-type widen set (`crate::let_widen`), keyed by
//! the initializer, so the checker, the local slot and the global slot
//! read one answer. An initializer that reads a held value counts
//! (`any_layout_slot::any_read`), and so does an `as any` cast, which
//! converts nothing either; a call is left out, since the typed
//! `JSON.parse` lanes decode theirs into the declared type.

use std::collections::HashMap;

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, FnSlot, Type as T};
use crate::ssa::Type;

/// Whether `ann` spells one of the scalar types this applies to.
fn scalar_ann(ann: &str) -> bool {
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

/// A store into a binding declared `ann` that holds a box: the value
/// must still fit the annotation, as tsc requires of it.
pub(crate) fn declared_store(
    checker: &Checker,
    name: &str,
    (ann, decl): (Option<&str>, Option<&FnSlot>),
    value_ty: &T,
) -> Result<(), String> {
    let (Some(ann), Some(FnSlot::Let(init))) = (ann, decl) else {
        return Ok(());
    };
    if !scalar_ann(ann) || !checker.cross_type_widened.contains(init) {
        return Ok(());
    }
    let declared = match ann {
        "number" => T::Number,
        "string" => T::String,
        _ => T::Boolean,
    };
    if crate::check_assignable::is_assignable_to_resolved(
        &declared,
        value_ty,
        &checker.class_structs,
        &checker.aliases,
        &checker.generic_alias_decls,
    ) {
        Ok(())
    } else {
        Err(format!(
            "type mismatch assigning to `{name}`: declared {declared:?}, value is {value_ty:?}"
        ))
    }
}
