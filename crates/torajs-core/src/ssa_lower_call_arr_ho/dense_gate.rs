//! The gate a typed source's dense loop copy runs on every step.

use crate::ast::ExprId;
use crate::ssa::Type;
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_arr_hole_gate::TypedGate;

/// The per-step gate a typed source's dense loop copy needs, from what
/// the program may do to the source while the callback runs.
pub(crate) fn typed_dense_gate(
    ctx: &LowerCtx<'_>,
    elem_ty: Type,
    recv: ExprId,
    cb: ExprId,
    o_param: usize,
) -> TypedGate {
    use crate::ast::hof_len_shrink::{hof_len_may_shrink, hof_may_punch_hole};
    if elem_ty == Type::Any {
        TypedGate::None
    } else if hof_may_punch_hole(ctx.ast, recv, cb, o_param) {
        TypedGate::Present
    } else if hof_len_may_shrink(ctx.ast, recv, cb, o_param) {
        TypedGate::Len
    } else {
        TypedGate::None
    }
}
