//! `Stmt::ForOf { var_name, var_type_ann, src_ident, i_ident,
//! elem_expr, body }` typecheck pulled out of
//! [`crate::check::Checker::check_stmt`]'s `Stmt::ForOf` arm as
//! chunk-106 of the check_stmt decomp. 9th check_stmt sibling.
//!
//! P5.3 generic for-of. The parser hoists src to a fresh Ident and
//! pre-builds `elem_expr = src[i]`. Typing the var_name binding
//! goes through `Expr::Index` lowering on elem_expr — which already
//! infers the right element type per source shape (`Array<T>` value
//! = T, `String[i]` = String, dynobj-backed `Any[i]` = Any). We
//! also declare `i_ident` as a Number local so the synthetic
//! counter typechecks.
//!
//! - **P5.3 Phase B exception** — when src has `Type::Struct` (a
//!   class instance), the protocol path in ssa_lower bypasses
//!   elem_expr entirely; typing `src[i]` here would error ("can't
//!   index into Struct"). Probe src's type first; if it's a
//!   class-shape Struct, defer the element type to ssa_lower (mark
//!   as Any so var_name still typechecks downstream as opaque).
//! - **P6.4c** — same skip for `Type::Map` / `Type::Set` /
//!   `Type::MapIter` / `Type::ArrIter` (handled by
//!   `lower_for_of_map_like`). For Type::Map specifically the
//!   yielded value is `[k, v]` `Array<Any>` (enables destructuring
//!   `for (let [k, v] of m)`); for Set / MapIter / ArrIter the
//!   yield is type-erased Any.

use std::collections::HashMap;

use crate::ast::{Ast, Expr, ExprId, Stmt};
use crate::check::{Checker, DiagPush, LocalInfo, Type, resolve_type_ann};

pub(crate) fn check(
    checker: &mut Checker,
    ast: &Ast,
    var_name: &str,
    var_type_ann: &Option<String>,
    i_ident: &str,
    elem_expr: ExprId,
    body: &Stmt,
) {
    checker.scopes.push(HashMap::new());
    let _ = checker.declare(
        i_ident.to_string(),
        LocalInfo {
            ty: Type::Number,
            mutable: true,
            moved: false,
            borrowed: false,
            declared_class: None,
        },
    );
    let src_kind = if let Expr::Index { obj, .. } = ast.get_expr(elem_expr) {
        checker.type_of(ast, *obj).ok()
    } else {
        None
    };
    let src_is_iter_subset = matches!(
        src_kind,
        Some(Type::Struct(_))
            | Some(Type::Map)
            | Some(Type::Set)
            | Some(Type::MapIter)
            | Some(Type::ArrIter)
    );
    let elem_ty = if src_is_iter_subset {
        match src_kind {
            Some(Type::Map) => Type::Array(Box::new(Type::Any)),
            _ => Type::Any,
        }
    } else {
        match checker.type_of(ast, elem_expr) {
            Ok(t) => t,
            Err(e) => {
                checker.errors.push_err(e);
                Type::Any
            }
        }
    };
    let var_ty = if let Some(ann) = var_type_ann {
        resolve_type_ann(ann, &checker.aliases).unwrap_or(elem_ty)
    } else {
        elem_ty
    };
    let _ = checker.declare(
        var_name.to_string(),
        LocalInfo {
            ty: var_ty,
            mutable: false,
            moved: false,
            borrowed: false,
            declared_class: None,
        },
    );
    checker.check_stmt(ast, body);
    checker.scopes.pop();
}
