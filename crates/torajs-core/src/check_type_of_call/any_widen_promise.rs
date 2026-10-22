//! The promise member of the any-widen wedge family
//! ([`super::any_widen`]), split out of that file.

use super::any_widen::{clonable_callee, record_widen_site};
use crate::ast::{Ast, ExprId};
use crate::check::{Checker, Type, WidenTarget};

/// A promise of `any` into a `Promise<T>` parameter, the promise twin
/// of [`super::any_widen::arr_any_into_typed_arr_param`]: TS admits it, and the promise
/// settles a value of any kind (§27.2.4.7 settles it as it is). Code
/// after an `await` of the parameter has no `any` version to fall back
/// to at run time, so the call takes a callee clone whose parameter is
/// `Promise<any>`.
#[allow(clippy::too_many_arguments)]
pub(super) fn any_promise_into_promise_param(
    checker: &mut Checker,
    ast: &Ast,
    eid: ExprId,
    callee: &ExprId,
    arg_ty: &Type,
    param_ty: &Type,
    i: usize,
) -> bool {
    if !matches!(param_ty, Type::Promise(t) if crate::check_type_of_call_promise_then::typed_lane(t))
        || !matches!(arg_ty, Type::Promise(t) if matches!(**t, Type::Any))
    {
        return false;
    }
    let crate::ast::Expr::Ident(n) = ast.get_expr(*callee) else {
        return false;
    };
    let Some(n) = clonable_callee(checker, ast, n, i) else {
        return false;
    };
    record_widen_site(checker, eid, &n, i, WidenTarget::Promise);
    true
}
