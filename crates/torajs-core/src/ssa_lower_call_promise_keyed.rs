//! `Promise.allKeyed` / `Promise.allSettledKeyed` lowering — the
//! await-dictionary proposal's keyed combinators. Sibling of
//! [`crate::ssa_lower_call_promise_static`]'s `lower_aggregate` (that
//! file sits at the 500-line watch): the argument is an OBJECT, not
//! an iterable, so there is no typed sync lane to pick — every shape
//! boxes to `any` and the runtime kernel validates (a non-object
//! REJECTS with TypeError per the proposal, never a compile reject —
//! the checker admits every argument type for the same reason).
//! Trailing args evaluate for side effects and drop, the S273
//! posture the sibling's combinators share.

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn lower_keyed(ctx: &mut LowerCtx<'_>, method: &str, args: &[ExprId]) -> Operand {
    let arg_op = ctx.lower_expr(args[0]);
    for &a in &args[1..] {
        let _ = ctx.lower_expr(a);
    }
    let arg_ty = ctx.operand_ty(&arg_op);
    let boxed = ctx.box_to_any_from_expr(args[0], arg_op.clone());
    let fid = match method {
        "allKeyed" => ctx.intrinsics.promise_all_keyed_dyn,
        "allSettledKeyed" => ctx.intrinsics.promise_allsettled_keyed_dyn,
        _ => unreachable!("keyed combinator arm gated on the two names"),
    };
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(fid, vec![boxed]),
        Type::Promise,
        None,
    );
    // The box shares; a fresh owned temp still owes its stake (the
    // sibling's dyn-combinator posture).
    if arg_ty.is_refcounted() && ctx.expr_is_fresh_owned(args[0]) {
        ctx.emit_drop_value(arg_op, arg_ty);
    }
    Operand::Value(v)
}
