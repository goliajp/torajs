//! RFC 20260820-dstr-deferred-close — the destructuring-pattern
//! synthesis arms of [`crate::ssa_lower_call_class_synth`], split to
//! their own sibling at the parent's size cap (the dstr family is
//! its own vocabulary: park slot, pending close, rest drain).

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand};
use crate::ssa_lower::LowerCtx;

/// `__torajs_dstr_close_pending(it)` — finally-position close of the
/// parked iterator slot (undefined / parked-index = no-op). Borrow
/// shape: the slot keeps its stake, its own drop settles it.
pub(crate) fn try_lower_close_pending(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Option<Operand> {
    let it = ctx.lower_expr(args[0]);
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Call(ctx.intrinsics.dstr_close_pending, vec![it]),
    );
    ctx.emit_throw_check(None);
    Some(Operand::ConstI64(0))
}

/// `__torajs_dstr_drain_rest(park, raw)` — 刀 D: drain the parked
/// rest tail into a fresh `Array<Any>` after the rest target's
/// reference (and its yield) evaluated. Both arguments are borrows
/// (the park copy and the raw-source binding keep their stakes); the
/// answer is an OWNED boxed array the enclosing assignment takes.
pub(crate) fn try_lower_drain_rest(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Option<Operand> {
    let park = ctx.lower_expr(args[0]);
    let recv = ctx.lower_expr(args[1]);
    let cur_block = ctx.cur_block;
    let out = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.dstr_drain_rest, vec![park, recv]),
        crate::ssa::Type::Any,
        None,
    );
    ctx.emit_throw_check(None);
    Some(Operand::Value(out))
}
