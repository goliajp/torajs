//! `Expr::Assign { target: Expr::Ident(name), value }` typecheck
//! sub-arm pulled out of
//! [`crate::check::Checker::type_of_inner`]'s `Expr::Assign` arm
//! as chunk-93 of the type_of_inner decomp.
//!
//! Two resolution paths (mirrors ssa_lower's `Expr::Assign` shape):
//!
//! 1. **Phase K.3 top-level data global** — when `lookup(name)` is
//!    `None` AND `globals` has the name: read declared global type,
//!    typecheck the value against it (`is_assignable_to_resolved`),
//!    consume the value expr, return the global type. (Const-ness
//!    not tracked for globals separately from LetDecl's `mutable`
//!    flag — pre-pass registration order means any top-level binding
//!    is writable from named-fn bodies for now.)
//! 2. **Local binding** — `lookup(name)` returns `LocalInfo`; reject
//!    if `!mutable` (const); typecheck value vs slot type; consume
//!    the value expr; `mark_unmoved(name)` clears the target's
//!    transient `moved` flag if rhs was `target + ...` (e.g. string
//!    concat); return the target type.

use crate::ast::{Ast, ExprId};
use crate::check::{Checker, Type};
use crate::check_assignable::is_assignable_to_resolved;

pub(crate) fn check(
    checker: &mut Checker,
    ast: &Ast,
    name: String,
    value: ExprId,
) -> Result<Type, String> {
    if checker.lookup(&name).is_none()
        && let Some(global_ty) = checker.globals.get(&name).cloned()
    {
        let value_ty = checker.type_of(ast, value)?;
        if !is_assignable_to_resolved(
            &global_ty,
            &value_ty,
            &checker.aliases,
            &checker.generic_alias_decls,
        ) {
            return Err(format!(
                "type mismatch assigning to global `{name}`: declared {global_ty:?}, value is {value_ty:?}"
            ));
        }
        checker.consume(ast, value);
        return Ok(global_ty);
    }
    let info = match checker.lookup(&name) {
        Some(i) => i,
        None => {
            return Err(format!("assignment to undeclared `{name}`"));
        }
    };
    if !info.mutable {
        return Err(format!("cannot assign to const `{name}`"));
    }
    let target_ty = info.ty.clone();
    let value_ty = checker.type_of(ast, value)?;
    if !is_assignable_to_resolved(
        &target_ty,
        &value_ty,
        &checker.aliases,
        &checker.generic_alias_decls,
    ) {
        return Err(format!(
            "type mismatch assigning to `{name}`: declared {target_ty:?}, value is {value_ty:?}"
        ));
    }
    checker.consume(ast, value);
    checker.mark_unmoved(&name);
    Ok(target_ty)
}
