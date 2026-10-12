//! A fn-typed binding holding a function value of another face than
//! its annotation: `const g: (y?: number) => number = mk(1)` where `mk`
//! returns `(y?: number) => any`.
//!
//! The let-position admit takes the value, as TS does, but a call
//! through the binding is shaped by the ANNOTATION's face while the
//! value was compiled in its own — the body reads a NaN-boxed return as
//! an f64 (`g()` printed NaN, bun 6). An immutable binding can only
//! ever hold that one value, so its slot takes the value's face
//! (`Checker::let_face_anns`, applied to the declaration by the
//! monomorph pass) and its calls cost nothing more than before. A
//! mutable binding may hold a different face after each store, so it
//! joins the boxed-entry census (`Checker::face_mismatch_lets` →
//! `Ast::face_mismatch_lets`): the adapter unboxes per the stored
//! value's own face. Either way the binding is a census fn slot, so a
//! call through it types what the value returns
//! (`check_type_of_call::fn_slot_ret`).
//!
//! Only a difference the calling convention sees counts
//! (`fn_face::face_ann`), and the census is keyed by the declaration
//! (its initializer), not the name: a same-named binding elsewhere
//! keeps its direct calls.

use crate::ast::ExprId;
use crate::check::{Checker, Type};
use crate::check_type_of_call::fn_face::{Face, let_face_ann};

/// Record the binding (annotation `ann`, initializer `init`) whose
/// initializer type `init_ty` differs from the annotated `ann_ty`.
/// Returns whether the binding became a census fn slot.
pub(super) fn record_let_face(
    checker: &mut Checker,
    ann: &str,
    init: ExprId,
    mutable: bool,
    init_ty: &Type,
    ann_ty: &Type,
) -> bool {
    if init_ty == ann_ty || !matches!((init_ty, ann_ty), (Type::Function(..), Type::Function(..))) {
        return false;
    }
    match let_face_ann(ann, init_ty, ann_ty) {
        Face::Same => false,
        Face::Spelled(face) if !mutable => {
            checker.let_face_anns.insert(init, face);
            true
        }
        // a mutable slot, a value with a narrower slot than the
        // annotation, or a face this lane does not spell: the boxed
        // entry, whose adapter tests each argument against the value's
        // own slot
        _ => {
            checker.face_mismatch_lets.insert(init);
            true
        }
    }
}

/// A store of `value_ty` into the mutable binding declared by `decl`
/// (annotation `ann`, declared type `target_ty`): a value of another
/// face joins the binding to the boxed-entry census.
pub(crate) fn record_store_face(
    checker: &mut Checker,
    ann: Option<&str>,
    decl: Option<ExprId>,
    target_ty: &Type,
    value_ty: &Type,
) {
    let Some(decl) = decl else {
        return;
    };
    if target_ty == value_ty || !matches!(value_ty, Type::Function(..)) {
        return;
    }
    let face = match ann {
        Some(ann) => let_face_ann(ann, value_ty, target_ty),
        None => Face::Unspelled,
    };
    if face != Face::Same {
        checker.face_mismatch_lets.insert(decl);
    }
}
