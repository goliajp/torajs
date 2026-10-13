//! A binding or parameter whose type is not a census fn slot, but whose
//! declaration a store joined to the census.

use super::{assign_stores, find_decl};
use crate::ast::{Ast, ExprId};
use crate::check::face_slots::{let_read, param_read};
use crate::check::{Checker, FnSlot, Type};

/// The census slot the binding `n` is: its type's (`fn_slot`), or the
/// declaration `decl` a store joined to the census: a `let` handed an
/// `any` value (`n = raw`), or a value of another face when an
/// annotation spells the face its calls expect (an unannotated binding
/// is typed by its initializer, whose face the boxed entry keeps); a
/// parameter of a body checked before the call that hands it such a
/// value (an object literal's method).
pub(super) fn joined_slot(
    checker: &mut Checker,
    ast: &Ast,
    n: &str,
    (fn_slot, decl): (Option<FnSlot>, Option<FnSlot>),
) -> Option<FnSlot> {
    if fn_slot.is_some() {
        return fn_slot;
    }
    match decl {
        Some(FnSlot::Let(init))
            if let_read(checker, init) && spelled_or_any(checker, ast, n, init) =>
        {
            Some(FnSlot::Let(init))
        }
        Some(FnSlot::Param(owner)) if param_read(checker, &owner, n) => Some(FnSlot::Param(owner)),
        _ => None,
    }
}

/// Is the binding `n` declared with `init` annotated, or handed an `any`
/// value by a store?
fn spelled_or_any(checker: &Checker, ast: &Ast, n: &str, init: ExprId) -> bool {
    find_decl(&ast.stmts, init).is_some()
        || assign_stores(ast, n).any(|v| checker.expr_types.get(&v) == Some(&Type::Any))
}
