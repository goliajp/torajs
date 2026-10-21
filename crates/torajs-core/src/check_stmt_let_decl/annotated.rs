//! The annotated arm of the let-declaration check, moved out of
//! [`super::check`] whole: the declared type a binding with a written
//! annotation takes, and whether its fn-typed face was recorded.

use super::{any_layout_slot, apply_contextual_array_ann, fn_face_slot, held_any};
use crate::ast::{Ast, ExprId};
use crate::check::{Checker, DiagPush, Type, face_slots};
use crate::check_assignable::is_assignable_to_resolved;
use crate::check_type_ann::{resolve_type_ann_decl, unknown_type};

/// `None` when the annotation is unknown or refuses the initializer;
/// the error is already pushed.
pub(super) fn declared_ty(
    checker: &mut Checker,
    ast: &Ast,
    (name, mutable): (&str, bool),
    (ann, init_ty): (&str, Type),
    init: ExprId,
) -> Option<(Type, bool)> {
    let Some(ann_ty) =
        resolve_type_ann_decl(ann, &checker.aliases, &[], &checker.generic_alias_decls)
    else {
        checker.errors.push_err(unknown_type(ann));
        return None;
    };
    let init_ty = crate::check_assignable::in_declared_order(checker, ast, &ann_ty, init, init_ty);
    if held_any::face::refuses_init(checker, ast, name, init, &ann_ty) {
        return None;
    }
    // 423-03 ④ — a fn-typed LET takes the same S2 call-face
    // admit a fn-typed call ARG takes (`const slot: () =>
    // void = gb` where gb declares a defaulted param): the
    // sig-thunk pass reabstracts the mismatched init exactly
    // like a call-arg position, so the checker face and the
    // lowering face agree. See `fn_slot_admits` on why this
    // is not widened inside `is_assignable_to` itself.
    let fn_slot_widened = crate::check_assignable::fn_slot_admits(
        &ann_ty,
        &init_ty,
        &checker.class_structs,
        &checker.aliases,
        &checker.generic_alias_decls,
    );
    // An initializer carrying accessor slots is compared
    // through the property each one implements, and KEEPS its
    // own layout — see `check_accessor_objlit_slot` for why
    // adopting the annotation instead turns a refusal into a
    // closure pointer printed as a number.
    let ordinary = fn_slot_widened
        || is_assignable_to_resolved(
            &ann_ty,
            &init_ty,
            &checker.class_structs,
            &checker.aliases,
            &checker.generic_alias_decls,
        );
    // Only where the ordinary comparison FAILS, which is what
    // keeps an `any` annotation winning: it admits the literal
    // outright, so the projection never runs and the binding
    // stays the box its dynamic member lane expects.
    let accessor_layout = !ordinary
        && crate::check_accessor_objlit_slot::admits(
            &ann_ty,
            &init_ty,
            &checker.class_structs,
            &checker.aliases,
            &checker.generic_alias_decls,
        );
    if !ordinary && !accessor_layout {
        checker.errors.push_err(format!(
            "type mismatch on `{name}`: declared {ann_ty:?}, init has {init_ty:?}"
        ));
        return None;
    }
    let tys = (&init_ty, &ann_ty);
    let face_slot =
        fn_face_slot::record_let_face(checker, ast, (ann, init, mutable), tys, fn_slot_widened);
    face_slots::record_objlit_stores(checker, ast, &ann_ty, init);
    let ty = if accessor_layout {
        // Pushing the annotation's element type onto the
        // literal is the same adoption by another route: it
        // retypes each element to the data-field shape, and
        // `xs[0].v` reads the getter closure as a number.
        init_ty
    } else {
        apply_contextual_array_ann(checker, ast, init, &ann_ty);
        any_layout_slot::declared_face(checker, ast, init, ann_ty)
    };
    Some((ty, face_slot))
}
