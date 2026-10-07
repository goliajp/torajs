//! The public type of a lifted closure VALUE — what a caller of the
//! function value sees, as opposed to the lifted fn's own params.

use crate::ast::{Ast, Param};
use crate::check::Type;

pub(super) fn closure_value_ty(
    ast: &Ast,
    fn_name: &str,
    real_params: &[Param],
    fn_ty: Type,
) -> Type {
    let Type::Function(param_tys, ret_ty) = &fn_ty else {
        return fn_ty;
    };
    // RFC 20260714-objlit-accessor blade 1 — an object-literal method's
    // receiver is a param of the LIFTED fn, not of the method the user
    // wrote: `o.m()` passes no receiver. It still has to be declared
    // into the body's scope (that is what makes `this.a` resolve), so
    // only the VALUE type sheds it.
    let takes_recv = real_params.first().is_some_and(|p| p.name == "__this");
    let value_ty = if takes_recv {
        Type::Function(param_tys.iter().skip(1).cloned().collect(), ret_ty.clone())
    } else {
        fn_ty.clone()
    };
    // RFC 20260708-closure-argv-face — a full-arguments closure's
    // PUBLIC type is `(...args: any[]) => R`: any call arity is
    // legal and every call must ride the boxed dual entry (whose
    // adapter feeds real argc + argv into the synthetic params).
    // The rest-tail type reuses the whole variadic track — call
    // admit, assignability, and the SSA boxed-lane registration.
    if ast.closure_argv_fns.contains(fn_name) {
        Type::Function(vec![Type::Rest(Box::new(Type::Any))], ret_ty.clone())
    } else {
        value_ty
    }
}
