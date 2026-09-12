//! RFC 20260912-global-var-cell knife 5 — the synthetic calls a
//! Script evaluated inside a function lowers to (`desugar_eval::
//! script_fn_position`): `__torajs_script_preflight_{lex,fn,var}(name)`,
//! `__torajs_global_var_declare(name)`, `__torajs_global_fn_declare(name,
//! value)`. Every operand is a string literal or a function value the
//! kernel takes as `any`; `type_of` runs for its effects, and the
//! call is a statement (`Type::Undefined`). The kernels decide
//! everything (`globalthis_object_decls`), nothing is typed here.

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

/// Is `name` one of the synthetics, at its arity?
pub(crate) fn is_script_decl_synthetic(name: &str, arity: usize) -> bool {
    matches!(
        (name, arity),
        (
            "__torajs_script_preflight_lex"
                | "__torajs_script_preflight_fn"
                | "__torajs_script_preflight_var"
                | "__torajs_global_var_declare",
            1
        ) | ("__torajs_global_fn_declare", 2)
    )
}

pub(crate) fn try_match(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Option<Result<Type, String>> {
    let Expr::Ident(n) = ast.get_expr(*callee) else {
        return None;
    };
    if !is_script_decl_synthetic(n, args.len()) {
        return None;
    }
    for a in args {
        if let Err(e) = checker.type_of(ast, *a) {
            return Some(Err(e));
        }
    }
    Some(Ok(Type::Undefined))
}
