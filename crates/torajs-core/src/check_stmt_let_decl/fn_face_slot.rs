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
//! value's own face; so does a store into a mutable `for-of` head or a
//! fn-typed parameter (`Checker::face_mismatch_params` →
//! `Ast::argv_boxed_params`). A declaration that took another face is
//! a census fn slot, so a call through it types what the value returns
//! (`check_type_of_call::fn_slot_ret`).
//!
//! An `any` value may be a function of any face, so a binding that
//! takes one, by its initializer or a store, joins the boxed-entry
//! census unless its slot passes no argument and discards the return
//! (`check::face_slots::faces_differ`); the call then answers what the
//! stored value returns, not the annotation's type.
//!
//! Only a difference the calling convention sees counts
//! (`fn_face::face_ann`), and the census is keyed by the declaration
//! (its initializer), not the name: a same-named binding elsewhere
//! keeps its direct calls.

use crate::ast::{Ast, Expr, ExprId, Stmt};
use crate::check::face_slots::{self, faces_differ};
use crate::check::{Checker, FnSlot, Type};
use crate::check_type_of_call::fn_face::{Face, held_face, let_face_ann};

/// Record the binding (annotation `ann`, initializer `init`) whose
/// initializer type `init_ty` differs from the annotated `ann_ty`
/// (`widened`: the fn-slot admit took it). A binding initialized from a
/// census slot joins the census, and one initialized from a binding
/// that took its value's face takes that face
/// (`check_type_of_call::fn_face::held_face`). Returns whether the
/// binding became a census fn slot.
pub(super) fn record_let_face(
    checker: &mut Checker,
    ast: &Ast,
    (ann, init, mutable): (&str, ExprId, bool),
    (init_ty, ann_ty): (&Type, &Type),
    widened: bool,
) -> bool {
    if matches!(ann_ty, Type::Function(..)) && census_value(checker, ast, init) {
        checker.face_mismatch_lets.insert(init);
        return true;
    }
    let held = held_face(checker, ast, init);
    let init_ty = held.as_ref().unwrap_or(init_ty);
    if !(widened || held.is_some() || *init_ty == Type::Any) {
        return false;
    }
    if *init_ty == Type::Any {
        if !faces_differ(init_ty, ann_ty)
            || carrier_holds_own_face(checker, ast, (ann, ann_ty), init)
        {
            return false;
        }
        checker.face_mismatch_lets.insert(init);
        return true;
    }
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

/// Does the default-parameter carrier `init` reads (`let h: F =
/// __dflt_h`, the desugar of `(h: F = g) => …`) hold only functions of
/// the face `ann_ty` spells, by its default and the arguments of the
/// calls in sight? The carrier is typed `any` only to see an
/// `undefined` argument; the binding then holds what the parameter
/// would.
fn carrier_holds_own_face(
    checker: &mut Checker,
    ast: &Ast,
    (ann, ann_ty): (&str, &Type),
    init: ExprId,
) -> bool {
    let (Expr::Ident(n), Some(owner)) = (ast.get_expr(init), checker.ret_owner.clone()) else {
        return false;
    };
    let Some(vs) = crate::check_type_of_call::carrier_values(ast, &owner, n) else {
        return false;
    };
    vs.into_iter()
        .all(|v| match checker.expr_types.get(&v).cloned() {
            Some(t @ Type::Function(..)) => {
                !census_value(checker, ast, v)
                    && held_face(checker, ast, v).is_none()
                    && (t == *ann_ty || let_face_ann(ann, &t, ann_ty) == Face::Same)
            }
            _ => false,
        })
}

/// A store of `value_ty` into the mutable binding `name` declared by
/// `decl` (annotation `ann`, declared type `target_ty`): a value of
/// another face joins the binding to the boxed-entry census, a `let`
/// by its declaration and a parameter by its function.
pub(crate) fn record_store_face(
    checker: &mut Checker,
    name: &str,
    ann: Option<&str>,
    decl: Option<&FnSlot>,
    target_ty: &Type,
    value_ty: &Type,
) {
    let Some(decl) = decl else {
        return;
    };
    if target_ty == value_ty {
        return;
    }
    let face = match (value_ty, ann) {
        (Type::Any, _) if faces_differ(value_ty, target_ty) => Face::Unspelled,
        (Type::Function(..), Some(ann)) => let_face_ann(ann, value_ty, target_ty),
        (Type::Function(..), None) => Face::Unspelled,
        _ => return,
    };
    if face == Face::Same {
        return;
    }
    match decl {
        FnSlot::Let(init) => crate::check::face_slots::record_let_store(checker, *init),
        FnSlot::Param(owner) => crate::check::face_slots::record_param(checker, owner, name),
    }
}

/// A store of a census slot's value (`k = g`) joins the binding or
/// parameter the write reaches (`decl`) to the census: it may hold a
/// function of another face than both annotations spell.
pub(crate) fn record_census_store(
    checker: &mut Checker,
    ast: &Ast,
    name: &str,
    decl: Option<&FnSlot>,
    target_ty: &Type,
    value: ExprId,
) {
    if !matches!(target_ty, Type::Function(..)) || !census_value(checker, ast, value) {
        return;
    }
    match decl {
        Some(FnSlot::Let(init)) => face_slots::record_let_store(checker, *init),
        Some(FnSlot::Param(owner)) => face_slots::record_param(checker, owner, name),
        None => {}
    }
}

/// Is `e` a read of a binding or parameter in the census, or of an
/// element of the element census (`FaceSlots::elems`), whose calls
/// ride the boxed entry? A slot it is handed to may then hold a
/// function of another face than both annotations spell (`const k: T =
/// h`, `run2(g)`), so it joins the census too.
pub(crate) fn census_value(checker: &mut Checker, ast: &Ast, e: ExprId) -> bool {
    let n = match ast.get_expr(e) {
        Expr::Ident(n) => n,
        // an element of an array of the element census
        _ => return face_slots::elem_value(checker, ast, e).is_some(),
    };
    let Some(info) = checker.lookup(n) else {
        return false;
    };
    info.fn_slot.is_some()
        || match info.decl {
            Some(FnSlot::Let(init)) => face_slots::let_read(checker, init),
            Some(FnSlot::Param(owner)) => face_slots::param_read(checker, &owner, n),
            None => false,
        }
}

/// [`record_store_face`] for a store a function body makes into a
/// top-level binding, which it reaches as a module global.
pub(crate) fn record_global_store_face(
    checker: &mut Checker,
    ast: &Ast,
    name: &str,
    global_ty: &Type,
    value_ty: &Type,
) {
    if !matches!(
        (global_ty, value_ty),
        (Type::Function(..), Type::Function(..) | Type::Any)
    ) {
        return;
    }
    let decl = crate::ast::toplevel_stmts_flat(ast)
        .into_iter()
        .find_map(|s| match s {
            Stmt::LetDecl {
                name: n,
                type_ann,
                init,
                mutable: true,
                ..
            } if n == name => Some((type_ann.as_deref(), FnSlot::Let(*init))),
            _ => None,
        });
    if let Some((ann, decl)) = decl {
        record_store_face(checker, name, ann, Some(&decl), global_ty, value_ty);
    }
}
