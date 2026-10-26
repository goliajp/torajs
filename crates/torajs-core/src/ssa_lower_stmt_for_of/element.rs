//! The loop variable's value in the array for-of: the element read,
//! awaited under `for await`.

use crate::ssa::Operand;
use crate::ssa_lower::LowerCtx;

/// The loop variable's value for the element `elem_expr` (`src[i]`).
pub(super) fn element_value(
    ctx: &mut LowerCtx,
    elem_expr: crate::ast::ExprId,
    steps_await: bool,
) -> Operand {
    if steps_await {
        crate::ssa_lower_fn::suspend::await_sync_step(ctx, elem_expr)
    } else {
        ctx.lower_expr(elem_expr)
    }
}
