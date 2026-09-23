//! An object literal with accessor slots meeting a slot that was
//! ANNOTATED — `const o: { v: number } = { get v() { return 7 } }`.
//!
//! The literal's layout spells the accessor as `__getter_v`, a
//! closure; the annotation spells `v`, a number. Both the assignment
//! check and the three lowering lanes compare layouts field by field,
//! so every such declaration was refused — eleven of thirteen probe
//! shapes, against a `tsc --strict` that accepts all of them. That is
//! the "refuse a legal program" direction of design-principles §7.1,
//! not a missing feature.
//!
//! **The rule, from tsc**: an accessor slot projects onto the property
//! it implements, and the ordinary comparison runs against the
//! projection. A getter whose RETURN type does not fit is still a type
//! error (`{ get v() { return "s" } }` into `{ v: number }` is TS2322,
//! and stays refused here).
//!
//! The projection itself is not written here. It already exists as
//! `check_type_of_call_object_static::own_property_types`, which the
//! [[Get]]-side faces (`Object.values` / `Object.entries` /
//! `JSON.stringify`) read; this is its fourth reader, not a second
//! copy of it.
//!
//! **Admission is not adoption.** When the projection admits, the
//! binding keeps the LITERAL's layout, not the annotation's — the
//! annotation only acts as the gate. Adopting it instead is what
//! rotation 695 measured and reverted: the accessor read probe
//! (`check_type_of_member_accessor::try_accessor_read`) looks for a
//! `__getter_<name>` slot on the RESOLVED receiver, so a binding
//! retyped to `{ v: number }` stops taking the accessor lane and `o.v`
//! reads slot 0 — printing a closure pointer as a number. Refusing a
//! legal program is bad; answering a wrong number for it is worse, and
//! the conformance gate cannot see the difference.
//!
//! **This is why the positions whose slot layout IS the declaration
//! stay refused** — an argument, a return, a class field. There is no
//! literal layout for them to keep, so admitting them would be exactly
//! that trade. They are the B tier of the RFC (a struct layout that
//! can hold an accessor pair), and they are out of scope here.

use crate::check::GenericAliasMap;
use crate::check::Type;
use crate::check_assignable::is_assignable_to_resolved;
use crate::check_type_of_call_object_static::own_property_types;
use std::collections::HashMap;

/// Whether this type's layout spells an accessor anywhere a
/// declaration's own layout could carry one — the literal itself, a
/// literal nested in a field, or the element type of an array of
/// them.
///
/// The lowering lanes ask this of the checker's verdict for the
/// initializer, which is how they learn to keep the inferred layout
/// instead of the parsed annotation. Asking the TYPE rather than the
/// AST is deliberate: an alias (`const p = o`) and a literal spelled
/// inline have to answer the same, and only the type does.
pub(crate) fn layout_carries_accessor(ty: &Type) -> bool {
    match ty {
        Type::Struct(fields) => fields.iter().any(|(name, f)| {
            crate::check_type_of_object_lit::accessor_slot(name.as_wtf8()).is_some()
                || layout_carries_accessor(f)
        }),
        Type::Array(elem) => layout_carries_accessor(elem),
        Type::Nullable(inner) => layout_carries_accessor(inner),
        _ => false,
    }
}

/// Whether `init_ty` fits the written annotation `ann_ty` once every
/// accessor slot is projected onto the property it implements.
///
/// Answers false when the initializer carries no accessor at all, so
/// the caller keeps its ordinary path: this is an extra admission, not
/// a replacement for one.
///
/// The projection runs only where the TARGET is a written annotation.
/// A target carrying accessor slots of its own is some literal's
/// inferred layout, and two inferred layouts have to line up slot for
/// slot — projecting there would let `{ v: 1 }` into a slot whose
/// field 0 is a closure.
pub(crate) fn admits(
    ann_ty: &Type,
    init_ty: &Type,
    class_structs: &HashMap<String, Type>,
    aliases: &HashMap<String, Type>,
    generic_aliases: &GenericAliasMap,
) -> bool {
    if !layout_carries_accessor(init_ty) || layout_carries_accessor(ann_ty) {
        return false;
    }
    let projected = project(init_ty);
    is_assignable_to_resolved(ann_ty, &projected, class_structs, aliases, generic_aliases)
}

/// `init_ty` with every accessor slot replaced by the property it
/// implements, at every depth a declaration's layout reaches.
fn project(ty: &Type) -> Type {
    match ty {
        Type::Struct(fields) => Type::Struct(
            own_property_types(fields)
                .into_iter()
                .map(|(k, v)| {
                    let v = project(&v);
                    (k, v)
                })
                .collect(),
        ),
        Type::Array(elem) => Type::Array(Box::new(project(elem))),
        Type::Nullable(inner) => Type::Nullable(Box::new(project(inner))),
        other => other.clone(),
    }
}
