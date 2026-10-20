//! `Promise<T | null>.then(cb)` / `.catch(cb)` — the nullable-inner
//! lane of [`crate::check_type_of_call_promise_then`]'s chain.
//!
//! Every other inner shape got an arm there as it was unlocked (Any,
//! Undefined / Void, Array). A nullable inner never did, so the whole
//! shape was unreachable: `Promise.resolve(a).then(...)` with
//! `a: string | null` was refused outright with "no member `.then` on
//! type Promise(Nullable(String))" — not a wrong answer, but no
//! answer at all, for a value the runtime already carries correctly
//! (the Str-slot boxer reads NULL as null, sentinel as undefined).
//!
//! Lives in its own file because the chain's host sat at 495 lines.

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

/// `Some(_)` when this is a `.then` / `.catch` on a nullable-inner
/// promise; `None` to let the chain continue.
pub(crate) fn try_then_nullable(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &[ExprId],
) -> Option<Result<Type, String>> {
    let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
    else {
        return None;
    };
    if (m_name != "then" && m_name != "catch") || args.len() != 1 {
        return None;
    }
    let src_ty = match crate::check::receiver_type(checker, ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    let Type::Promise(inner) = &src_ty else {
        return None;
    };
    if !matches!(**inner, Type::Nullable(_)) {
        return None;
    }
    let inner_ty = (**inner).clone();
    let cb_ty = match checker.type_of(ast, args[0]) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    let Type::Function(params, ret) = &cb_ty else {
        return Some(Err(format!(
            "Promise.{m_name} on Promise<{inner_ty:?}>: cb must be `(v: {inner_ty:?}) => V`, got {cb_ty:?}"
        )));
    };
    // A handler that declares no parameter ignores the settled value
    // (§27.2.5.4 passes it as one argument), as on every other lane.
    if params.len() > 1 {
        return Some(Err(format!(
            "Promise.{m_name} on Promise<{inner_ty:?}>: cb takes at most one parameter, got {cb_ty:?}"
        )));
    }
    // An `any` parameter is admitted the same way the sibling lanes
    // admit it: the lowering marks PARAM_ANY off the SSA signature
    // and the kernel boxes off the source's own repr stamp, which is
    // where the null / undefined distinction lives.
    if let Some(p) = params.first()
        && !matches!(p, Type::Any)
        && *p != inner_ty
    {
        return Some(Err(format!(
            "Promise.{m_name} on Promise<{inner_ty:?}>: cb parameter must be `{inner_ty:?}` or `any`, got {p:?}"
        )));
    }
    // The nullable inner passes THROUGH a value-less cb: `.then(v =>
    // console.log(v))` keeps the chain's own T rather than collapsing
    // it, matching the other lanes' Void handling.
    let result_inner = match &**ret {
        Type::Void | Type::Undefined | Type::Never => Type::Undefined,
        other => other.clone(),
    };
    Some(Ok(Type::Promise(Box::new(result_inner))))
}
