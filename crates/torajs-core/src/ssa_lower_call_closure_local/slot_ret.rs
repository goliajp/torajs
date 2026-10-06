//! The lowering half of `check_type_of_call::fn_slot_ret`: a call
//! through a census-recorded fn-typed slot that the checker typed
//! `any` (`Ast::fn_slot_ret_any`) answers the boxed value it got back,
//! unconverted.
//!
//! The boxed-entry lanes ask [`ret_face`] for the type to convert to.
//! A lane that is not the boxed entry (a `.call` / `.apply` replay, a
//! typed direct call) produces a typed value, and [`settle`] boxes it,
//! so the value always has the representation the checker recorded
//! for the call.

use crate::ast::ExprId;
use crate::ssa::{Operand, Type};
use crate::ssa_lower::LowerCtx;

/// The return type a boxed-entry call's answer converts to: the
/// slot's declared one, or `any` when the checker typed the call so.
pub(super) fn ret_face(ctx: &LowerCtx<'_>, eid: ExprId, user_sig_id: crate::ssa::SigId) -> Type {
    if ctx.ast.fn_slot_ret_any.contains(&eid) {
        Type::Any
    } else {
        ctx.fn_sigs[user_sig_id.0 as usize].1
    }
}

/// The call's result in the representation its checker type names.
pub(crate) fn settle(ctx: &mut LowerCtx<'_>, eid: ExprId, op: Operand) -> Operand {
    if !ctx.ast.fn_slot_ret_any.contains(&eid) || ctx.operand_ty(&op) == Type::Any {
        return op;
    }
    ctx.box_to_any(op)
}
