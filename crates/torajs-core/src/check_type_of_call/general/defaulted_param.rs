//! A nullable argument into a defaulted parameter, split out of the general call checker.

use super::*;

/// `T | undefined` into a parameter declared `T` WITH a default — legal
/// in TS, because a defaulted parameter's caller-side type is `T |
/// undefined` (§10.2.11: an `undefined` argument takes the default).
/// The one caller today is the async wrapper RFC 20260805 blade 1
/// mints: its `n?: number` forwards to the generator's `n: number = 5`,
/// and the or_default lane binds the literal default for the boxed
/// actual. Only a top-level FnDecl callee is consulted — the lookup is
/// by name, which is all the wrapper's thunk needs.
pub(super) fn nullable_into_defaulted_param(
    ast: &Ast,
    callee: &ExprId,
    i: usize,
    param_ty: &Type,
    arg_ty: &Type,
) -> bool {
    let Type::Nullable(inner) = arg_ty else {
        return false;
    };
    if **inner != *param_ty {
        return false;
    }
    let crate::ast::Expr::Ident(name) = ast.get_expr(*callee) else {
        return false;
    };
    ast.stmts.iter().any(|s| {
        matches!(s, crate::ast::Stmt::FnDecl { name: n, params, .. }
            if n == name
                && crate::ast::peel_hidden_params(params)
                    .get(i)
                    .is_some_and(|p| p.default.is_some()))
    })
}
