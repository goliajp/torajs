//! `Promise<T>.finally(onFinally)` where the handler RETURNS
//! something — the early-route arm the method table cannot express.
//!
//! §27.2.5.3 declares `onFinally` as `() => any` and does two things
//! with what comes back: if it is a thenable, the settlement waits for
//! it; otherwise it is discarded. tr's method-table signature says
//! `() => void`, so every other shape was a COMPILE REJECT —
//! `Promise.resolve(3).finally(() => 99)` did not build, and neither
//! did the shape the wait exists for,
//! `.finally(() => cleanupAsync())`.
//!
//! This is the mirror of [`crate::check_type_of_call_arr_pred_void_cb`]:
//! there a formal `(T) => boolean` had to admit a `Void` actual, here
//! a formal `() => void` has to admit any actual. The method-table arm
//! stays for the `() => void` shape it already covers; this one runs
//! ahead of it for the rest.
//!
//! The result type is the receiver's own `Promise<T>` whatever the
//! handler returns — `finally` forwards the source's settlement and
//! never the handler's value. Only a REJECTION of a returned promise
//! can displace it, and that is a rejection reason, which the type
//! does not name.
//!
//! Returns `Some(Ok(Promise<T>))` on match, `Some(Err(_))` when the
//! receiver's own `type_of` failed, and `None` for any other shape.

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn try_match(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Option<Result<Type, String>> {
    let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
    else {
        return None;
    };
    if m_name != "finally" || args.len() != 1 {
        return None;
    }
    let src_ty = match checker.type_of(ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    let Type::Promise(inner) = &src_ty else {
        return None;
    };
    let cb_ty = match checker.type_of(ast, args[0]) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    // Zero-parameter only: §27.2.5.3 calls `onFinally` with no
    // arguments, so a handler that declares one is a real mistake and
    // stays a reject. A `Void` return falls through to the method
    // table, which already answers it.
    let Type::Function(params, ret) = &cb_ty else {
        return None;
    };
    if !params.is_empty() || matches!(**ret, Type::Void) {
        return None;
    }
    Some(Ok(Type::Promise(inner.clone())))
}
