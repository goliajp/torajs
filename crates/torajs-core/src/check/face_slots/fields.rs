//! The property slots of the census
//! ([`crate::ast::face_slots::FaceSlots::fields`]): a field store, or an
//! object literal's field against a declared struct type, that hands
//! the field a function value of another face than the field's type.

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

use super::{elems, faces_differ, remember};

/// The census owner of receiver `obj` when its field `name` holds such
/// a value ([`crate::ast::face_slots::FaceSlots::field_hit`]); a miss
/// is remembered.
pub(crate) fn field_read(
    checker: &mut Checker,
    ast: &Ast,
    obj: ExprId,
    name: &str,
) -> Option<String> {
    let recv = checker.expr_types.get(&obj)?;
    let census = &checker.face_slots;
    let hit = (census.fields.iter().any(|(_, n)| n == name))
        .then(|| owner_key(ast, recv))
        .flatten()
        .filter(|owner| census.field_hit(&ast.class_parents, owner, name));
    if hit.is_none() && matches!(recv, Type::ClassRef(_) | Type::Struct(_)) {
        remember(&mut checker.face_slot_reads.fields, name);
    }
    hit
}

/// The census owner of a receiver typed `ty`: a class instance's class,
/// or an inline struct type's structural spelling.
pub(crate) fn owner_key(ast: &Ast, ty: &Type) -> Option<String> {
    match ty {
        Type::ClassRef(n) if ast.class_parents.contains_key(n) => Some(n.clone()),
        Type::Struct(_) => Some(crate::check::type_to_ann(ty)),
        _ => None,
    }
}

/// A store of `value_ty` into field `field` of a receiver typed `recv`,
/// the field declared `slot_ty`.
pub(crate) fn record_field_store(
    checker: &mut Checker,
    ast: &Ast,
    recv: &Type,
    field: &str,
    slot_ty: &Type,
    value_ty: &Type,
) {
    if !faces_differ(value_ty, slot_ty) {
        return;
    }
    let Some(mut owner) = owner_key(ast, recv) else {
        return;
    };
    // the class that declares the field, which every receiver that can
    // hold it names among its ancestors
    while let Some(Some(parent)) = ast.class_parents.get(&owner) {
        let declares = matches!(
            checker.class_structs.get(parent),
            Some(Type::Struct(fs)) if fs.iter().any(|(n, _)| n == field)
        );
        if !declares {
            break;
        }
        owner = parent.clone();
    }
    let key = (owner, field.to_string());
    if !checker.face_slots.fields.contains(&key) {
        checker.face_slot_reads.stale |= checker.face_slot_reads.fields.contains(field);
        checker.face_slots.fields.insert(key);
    }
}

/// The fields of the object literal `value` admitted into a slot typed
/// `slot_ty`. The literal joins the census's literals only once every
/// member of the slot it holds is recorded: one whose value the checker
/// has not typed yet is not, and a reader of the census
/// (`hidden_any::method_lits::opaque`) takes the literal as seen. An
/// array literal's elements are recorded against a function element
/// type instead.
pub(crate) fn record_objlit_stores(
    checker: &mut Checker,
    ast: &Ast,
    slot_ty: &Type,
    value: ExprId,
) {
    let Expr::ObjectLit { fields } = ast.get_expr(value) else {
        elems::record_arrlit_stores(checker, ast, slot_ty, value);
        return;
    };
    let Type::Struct(slot_fields) = slot_ty else {
        return;
    };
    let mut whole = true;
    for (key, v) in fields {
        let Some((_, field_ty)) = slot_fields.iter().find(|(n, _)| n == key) else {
            continue;
        };
        let Some(value_ty) = checker.expr_types.get(v).cloned() else {
            whole = false;
            continue;
        };
        record_field_store(checker, ast, slot_ty, &key.lossy(), field_ty, &value_ty);
    }
    let seen = checker.face_slots.objlits.entry(value).or_default();
    if whole && !seen.contains(slot_ty) {
        seen.push(slot_ty.clone());
    }
}

/// `recv.field = value` (a class field's initializer included, which
/// the constructor stores through `this`): the store itself, and an
/// array or object literal's members against the field's type
/// (`b.fs = [tag]`).
pub(crate) fn record_field_assign(
    checker: &mut Checker,
    ast: &Ast,
    recv: &Type,
    (field, slot_ty): (&str, &Type),
    (value, value_ty): (ExprId, &Type),
) {
    record_field_store(checker, ast, recv, field, slot_ty, value_ty);
    record_objlit_stores(checker, ast, slot_ty, value);
}
