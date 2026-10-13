//! What a fn-typed slot takes besides a closure of its own type,
//! split out of `ssa_lower_assign_slot_fit`.

use crate::ssa::Type;

/// An `any` value stored into a fn-typed binding (`n = raw`): the
/// checked cast takes the closure cell out of the box, as the let
/// initializer's lane does. The checker joins the binding to the
/// boxed-entry census wherever the value's face can differ from the
/// slot's, so its calls unbox per the stored value's own face.
pub(super) fn any_into_closure_slot(slot_ty: Type, v_ty: Type) -> bool {
    matches!(slot_ty, Type::Closure(_)) && v_ty == Type::Any
}

/// A top-level function's bare pointer assigned into a binding that
/// holds closures of the same signature (`let k = mk(); k = f`): the
/// slot takes the function's canonical closure cell, as a closure
/// parameter does. The stores the AST can see arrive wrapped already
/// (`ast::fn_assign_rhs_wrap`); a signature that differs (number
/// widths settled apart) has no cell to hand over.
pub(super) fn fn_pointer_into_closure_slot(slot_ty: Type, v_ty: Type) -> bool {
    matches!((slot_ty, v_ty), (Type::Closure(a), Type::FnSig(b)) if a == b)
}
