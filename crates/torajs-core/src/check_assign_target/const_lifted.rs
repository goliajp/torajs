//! Stores to a machine field a `const` / `using` local was lifted into.

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

/// RFC 20260915 #8 — is `<receiver>.<field>` a machine field that a
/// `const` / `using` local was lifted into? The receiver is the
/// machine's own `this` (`ClassRef(__Gen_<name>)`) or a nested
/// machine's frame handle typed the same; anything else is not a
/// lifted slot.
pub(crate) fn is_const_lifted_field(ast: &Ast, obj_ty: &Type, field: &str) -> bool {
    let Type::ClassRef(cls) = obj_ty else {
        return false;
    };
    ast.const_lifted_fields
        .get(cls)
        .is_some_and(|fields| fields.contains(field))
}

/// RFC 20260915 #8, update-expression clothing — `this.<f>++` on a
/// lifted `const` is a store like any other. The receiver's declared
/// type is read off the scope ledger (no `type_of`: the caller types
/// the whole target right after, and a read must not run twice).
pub(crate) fn reject_const_lifted_update(
    checker: &Checker,
    ast: &Ast,
    target: ExprId,
) -> Result<(), String> {
    let Expr::Member { obj, name } = ast.get_expr(target) else {
        return Ok(());
    };
    let Expr::Ident(recv) = ast.get_expr(*obj) else {
        return Ok(());
    };
    let Some(info) = checker.lookup(recv) else {
        return Ok(());
    };
    if is_const_lifted_field(ast, &info.ty, name) {
        return Err(format!("cannot assign to const `{name}`"));
    }
    Ok(())
}
