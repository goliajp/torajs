//! Whether the program spells a way to take a declared field away from
//! a plain slot store other than redefining it: `freeze`, which makes
//! every field read-only (§20.1.2.6), or a property delete (`delete
//! o.f`, `Reflect.deleteProperty`), after which a store re-creates the
//! property (§10.1.9.2 step 2.c). `freeze` and `deleteProperty` count
//! as members read or called and as strings a key can be built from;
//! every other way to either function goes through `Object` / `Reflect`
//! held as a value or picked by a computed key, which [`super`] already
//! answers with every name.

use super::super::super::{Ast, Expr};

pub(super) const FREEZE: &str = "freeze";
pub(super) const DELETE_PROPERTY: &str = "deleteProperty";

pub(super) fn named(ast: &Ast) -> bool {
    let spelled = |s: &str| s == FREEZE || s == DELETE_PROPERTY;
    ast.exprs.iter().any(|e| match e {
        Expr::Member { name, .. } | Expr::OptChain { name, .. } => spelled(name),
        Expr::String(s) => s == FREEZE || s == DELETE_PROPERTY,
        // a binding's delete answers false and touches no property
        Expr::Delete { expr } => !matches!(ast.get_expr(*expr), Expr::Ident(_)),
        _ => false,
    })
}
