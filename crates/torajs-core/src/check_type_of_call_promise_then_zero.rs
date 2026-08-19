//! `Promise<T>.then()` / `.catch()` with both handlers absent —
//! §27.2.5.4 defaults onFulfilled to Identity and onRejected to
//! Thrower when the slot is not callable, so the 0-arg spelling is
//! legal and the derived promise settles exactly as the source does
//! (one reaction tick later). Split from
//! `check_type_of_call_promise_then.rs` at its 500-line watch
//! (rotation 452).
//!
//! The result type is the source's own `Promise<T>` — Identity
//! forwards the value unchanged, so the pass-through is exact for
//! every inner T. The lowering pairs this with the
//! `__torajs_promise_then_passthrough` kernel (mint pending +
//! adopt the source).

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn try_then_zero_arg(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Option<Result<Type, String>> {
    if let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && (m_name == "then" || m_name == "catch")
        && args.is_empty()
    {
        let src_ty = match checker.type_of(ast, *src_id) {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        if let Type::Promise(inner) = &src_ty {
            return Some(Ok(Type::Promise(inner.clone())));
        }
    }
    None
}
