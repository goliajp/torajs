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

/// `__torajs_dstr_park_take(it)` — the rest element's OWNED copy of
/// the park before the slot clears. The park slot lives outside the
/// close wrap's try block and the copy inside it, so the copy would
/// otherwise be a cross-scope alias with no stake of its own and the
/// clear would free the iterator under the drain. A retain (cell +1,
/// immediates untouched) is exactly the stake the copy's own scope
/// drop settles.
pub(crate) fn try_lower_park_take(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Option<Operand> {
    let it = ctx.lower_expr(args[0]);
    let cur_block = ctx.cur_block;
    let out = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.anyv_retain, vec![it]),
        crate::ssa::Type::Any,
        None,
    );
    Some(Operand::Value(out))
}

/// `__torajs_dstr_step_one(it, raw)` — §13.15.5.5 one element's step
/// off the parked walk. Unlike its siblings this one WRITES the park
/// slot, so `args[0]` is read as a LOCATION rather than a value: the
/// current park loads out, the kernel takes that stake and reports
/// the next park through a scratch slot, and the result stores back.
///
/// The store lands BEFORE the throw check on purpose. A step that
/// throws sets §7.4.8's [[done]], and the kernel signals that by
/// writing `undefined`; checking first would unwind past the store
/// and leave a live iterator in the slot for the pattern's finally to
/// close — the `*-thrw-close-skip` regression.
pub(crate) fn try_lower_step_one(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Option<Operand> {
    let target = crate::ssa_lower_dstr_iter::park_target_of_expr(ctx, args[0])?;
    let recv = ctx.lower_expr(args[1]);
    let cur = crate::ssa_lower_dstr_iter::emit_park_load(ctx, &target);
    let out_slot = ctx.alloca(crate::ssa::Type::Any, Some("__dstr_step_park"));
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.dstr_step_one,
            vec![cur, recv, Operand::Value(out_slot)],
        ),
        crate::ssa::Type::Any,
        None,
    );
    let next = ctx.f.append_inst(
        cur_block,
        InstKind::Load(crate::ssa::Type::Any, Operand::Value(out_slot), 0),
        crate::ssa::Type::Any,
        None,
    );
    crate::ssa_lower_dstr_iter::emit_park_store(ctx, &target, Operand::Value(next));
    ctx.emit_throw_check(None);
    Some(Operand::Value(v))
}
