//! `Expr::Assign` typecheck pulled out of
//! [`crate::check::Checker::type_of_inner`]'s `Expr::Assign` arm
//! as chunk-313 of the type_of_inner decomp.
//!
//! Assignment target shape dispatch — all three legal target
//! forms delegate to their existing sibling:
//!
//! - `Ident(name)` → [`crate::check_assign_ident::check`] (K.3
//!   top-level data global vs local binding lookup).
//! - `Member { obj, name }` →
//!   [`crate::check_assign_target::check_member`] (M1.4 / P3.2 /
//!   T-27 / T-29 / P9.4 / P8.2 / V3-05 / V3-06 / M-OO.5 readonly).
//! - `Index { obj, index }` →
//!   [`crate::check_assign_target::check_index`] (M1.4
//!   `arr[i] = value` with Number-index + Array<T> elem type
//!   match).
//!
//! Any other target shape is a typecheck error (`invalid
//! assignment target`).

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn check(
    checker: &mut Checker,
    ast: &Ast,
    target: ExprId,
    value: ExprId,
) -> Result<Type, String> {
    match ast.get_expr(target).clone() {
        Expr::Ident(name) => {
            // RFC 20260710 C5 — rebinding the receiver invalidates
            // every member-path narrow rooted at it.
            checker.member_narrows.retain(|(recv, _), _| recv != &name);
            crate::check_assign_ident::check(checker, ast, name, value)
        }
        Expr::Member { obj, name: field } => {
            // RFC 20260710 C5 — writing the member invalidates its
            // own narrow entry (Ident receivers mint the keys).
            if let Expr::Ident(recv) = ast.get_expr(obj) {
                checker
                    .member_narrows
                    .remove(&(recv.clone(), field.clone()));
            }
            crate::check_assign_target::check_member(checker, ast, obj, field, value)
        }
        Expr::Index { obj, index } => {
            crate::check_assign_target::check_index(checker, ast, obj, index, value)
        }
        _ => Err("invalid assignment target".into()),
    }
}
