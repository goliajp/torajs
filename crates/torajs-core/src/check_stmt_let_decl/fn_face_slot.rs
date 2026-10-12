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
//! joins the boxed-entry census (`Checker::face_mismatch_bindings` →
//! `Ast::fnsig_mismatch_bindings`): the adapter unboxes per the stored
//! value's own face. Either way the binding is a census fn slot, so a
//! call through it types what the value returns
//! (`check_type_of_call::fn_slot_ret`).

use crate::ast::ExprId;
use crate::check::{Checker, Type};

/// Record the binding `name` (annotation `ann`, initializer `init`)
/// whose initializer type `init_ty` differs from the annotated
/// `ann_ty`. Returns whether the binding became a census fn slot.
pub(super) fn record_let_face(
    checker: &mut Checker,
    ann: &str,
    init: ExprId,
    mutable: bool,
    name: &str,
    init_ty: &Type,
    ann_ty: &Type,
) -> bool {
    if init_ty == ann_ty || !matches!((init_ty, ann_ty), (Type::Function(..), Type::Function(..))) {
        return false;
    }
    if mutable {
        checker.face_mismatch_bindings.insert(name.to_string());
        return true;
    }
    match crate::check_type_of_call::fn_face::let_face_ann(ann, init_ty, ann_ty) {
        Some(face) => {
            checker.let_face_anns.insert(init, face);
            true
        }
        // a value with a narrower slot than the annotation, or a face
        // this lane does not spell: the boxed entry, whose adapter
        // tests each argument against the value's own slot
        None => {
            checker.face_mismatch_bindings.insert(name.to_string());
            true
        }
    }
}
