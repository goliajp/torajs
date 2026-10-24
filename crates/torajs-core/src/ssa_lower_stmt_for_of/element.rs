//! The loop variable's value in the array for-of: the element read,
//! awaited under `for await` by the checker's verdict on it.

use crate::ssa::{Operand, Type};
use crate::ssa_lower::LowerCtx;

/// The loop variable's value for the element `elem_expr` (`src[i]`).
pub(super) fn element_value(
    ctx: &mut LowerCtx,
    elem_expr: crate::ast::ExprId,
    is_await: bool,
    steps_await: bool,
) -> Operand {
    // Hole Z — for-await element await by the checker's verdict: a
    // Promise-typed element loads through promise_get_value (microtask
    // drain + value cast + rc_inc, same route the `await e` Member arm
    // takes); every other element awaits to itself per §27.2 and the
    // plain Index load stands.
    if steps_await {
        crate::ssa_lower_fn::suspend::await_sync_step(ctx, elem_expr)
    } else if is_await
        && matches!(
            ctx.expr_types.get(&elem_expr),
            Some(crate::check::Type::Promise(_))
        )
    {
        crate::ssa_lower_member_promise_value::lower_promise_get_value(ctx, elem_expr)
    } else {
        let v = ctx.lower_expr(elem_expr);
        // rotation 233 — an `any` element only knows its form at
        // runtime: the `__torajs_anyv_await` hook unwraps a heap
        // Promise cell (promise-in-any used to bind verbatim) and
        // passes everything else through identity.
        if is_await && matches!(ctx.operand_ty(&v), Type::Any) {
            crate::ssa_lower_member_promise_value::lower_any_await(ctx, v, elem_expr)
        } else {
            v
        }
    }
}
