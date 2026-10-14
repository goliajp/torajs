//! A class declared inside a generator body: what of it reads the
//! generator's lifted locals, and through which base.

use super::{ParamMap, rewrite_params_in_expr};
use crate::ast::{Ast, Stmt};

/// Rewrite the lifted-local reads of the class `s`.
///
/// Its computed member keys (and computed static-field inits) live in
/// the `class_computed_keys` / `class_computed_static_fields` side
/// tables, not on the stmt tree — a lifted-local read inside the
/// brackets (`[k]`, a hoisted `__yx_N` yield temp most of all) kept its
/// old name while the binding became a state-machine field (the
/// cpn-class-*-yield t262 family). They evaluate in the ENCLOSING scope
/// per §15.7.14, so the lifted-field mapping applies verbatim.
///
/// Method / ctor bodies stay untouched: their `this` is the class
/// instance, and the capturing (ES5) lane owns their semantics.
pub(super) fn rewrite_class(
    ast: &mut Ast,
    s: &Stmt,
    pset: &ParamMap,
    visited: &mut std::collections::HashSet<u32>,
) {
    let Stmt::ClassDecl { name, .. } = s else {
        return;
    };
    let keys: Vec<crate::ast::ExprId> = ast
        .class_computed_keys
        .iter()
        .filter(|((c, _), _)| c == name)
        .map(|(_, &e)| e)
        .chain(
            ast.class_computed_static_fields
                .iter()
                .filter(|(c, _, _)| c == name)
                .map(|(_, _, init)| *init),
        )
        .collect();
    for k in keys {
        rewrite_params_in_expr(ast, k, pset, visited);
    }
}
