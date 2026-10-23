//! The throw check after a suspension inside a `try` or a `for…of`.
//!
//! A frame dropped while it waits is unwound by re-entering it at its
//! suspension point with a throw pending (`torajs-promise`
//! `frame::body`). Outside every exit protocol the throw path is
//! exactly that unwinding: it releases what the body holds and leaves.
//! Inside one it is not: the throw would enter a `catch` or a
//! `finally` and close the for-ofs it crosses, all user code, which a
//! function that is never resumed must not run (§27.7.5.3: nothing
//! continues an abandoned await). So there the check first asks
//! whether the frame is being cancelled, and a cancelled frame takes
//! its own exit: it releases the owned operand, the parked temps, the
//! values the exit protocols keep (a pending return, the throws the
//! `finally` bodies around it set aside) and every owned local, with
//! no handler, no `finally` body and no IteratorClose, then propagates
//! the cancelling throw that `frame::body` discards.
//!
//! The question is a read of the frame's cancel word. The frame is
//! the resume function's parameter, which the body gets only once it
//! is installed (`async_setup::frame_body`), so the lowering asks
//! through a call of `__torajs_frame_cancelled` there turns into the
//! read; no call of it survives.

use crate::ssa::{IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_temp_scratch::ThrowTemp;

/// The placeholder `async_setup::frame_body` turns into the read of
/// the frame's cancel word.
pub(crate) const CANCELLED_PROBE: &str = "__torajs_frame_cancelled";

/// The throw check after a suspension, `owned` the value it hands the
/// body (dropped on every throw edge).
pub(super) fn throw_check(ctx: &mut LowerCtx<'_>, owned: Option<(Operand, Type)>) {
    if !in_exit_protocol(ctx) {
        match owned {
            Some((op, ty)) => ctx.emit_throw_check_owned(None, op, ty),
            None => ctx.emit_throw_check(None),
        }
        return;
    }
    let active = ctx.emit_throw_active_load();
    let raised = test(ctx, active);
    let normal = ctx.f.add_block();
    let thrown = ctx.f.add_block();
    branch(ctx, raised, thrown, normal);
    ctx.cur_block = thrown;
    let probe = *ctx
        .fn_table
        .get(CANCELLED_PROBE)
        .expect("the frame calls are declared with the suspending bodies");
    let word = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(probe, vec![]),
        Type::I64,
        None,
    );
    let cancelled = test(ctx, word);
    let cancel = ctx.f.add_block();
    let throw_blk = ctx.f.add_block();
    branch(ctx, cancelled, cancel, throw_blk);
    ctx.emit_throw_path(throw_blk, owned);
    ctx.cur_block = cancel;
    unwind(ctx, owned);
    ctx.cur_block = normal;
}

/// Is a handler, a `finally` body or a for-of between the suspension
/// and the function's exit?
fn in_exit_protocol(ctx: &LowerCtx<'_>) -> bool {
    !ctx.try_stack.is_empty()
        || !ctx.try_finally_stack.is_empty()
        || !ctx.for_of_teardown_stack.is_empty()
        || !ctx.pending.finally_throws.is_empty()
}

/// The cancelled frame's exit: release everything the body owns here
/// and propagate.
fn unwind(ctx: &mut LowerCtx<'_>, owned: Option<(Operand, Type)>) {
    if let Some((op, ty)) = owned {
        ctx.emit_drop_value(op, ty);
    }
    let live: Vec<ThrowTemp> = ctx.temps.throw_live.iter().flatten().cloned().collect();
    for t in live.into_iter().rev() {
        ctx.emit_drop_throw_temp(t);
    }
    if !ctx.pending.finally_throws.is_empty() {
        drop_pending_return(ctx);
    }
    for [active, tag, value] in ctx.pending.finally_throws.clone().into_iter().rev() {
        let set = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(Type::I64, Operand::Value(active), 0),
            Type::I64,
            None,
        );
        let set = test(ctx, set);
        let release = ctx.f.add_block();
        let next = ctx.f.add_block();
        branch(ctx, set, release, next);
        ctx.cur_block = release;
        let load = |ctx: &mut LowerCtx<'_>, slot| {
            let k = InstKind::Load(Type::I64, Operand::Value(slot), 0);
            Operand::Value(ctx.f.append_inst(ctx.cur_block, k, Type::I64, None))
        };
        let (tag, value) = (load(ctx, tag), load(ctx, value));
        let boxed = InstKind::Call(ctx.intrinsics.any_box, vec![tag, value]);
        let boxed = ctx.f.append_inst(ctx.cur_block, boxed, Type::Any, None);
        ctx.emit_drop_value(Operand::Value(boxed), Type::Any);
        let cb = ctx.cur_block;
        ctx.f.set_term(cb, Terminator::Br(next));
        ctx.cur_block = next;
    }
    ctx.emit_drops_for_owned_locals();
    ctx.emit_propagate_ret();
}

/// Inside a `finally` body a `return` may be pending: its value is the
/// slot's, owned, until the tail returns it.
fn drop_pending_return(ctx: &mut LowerCtx<'_>) {
    let (Some(flag), Some(slot)) = (ctx.pending.return_flag, ctx.pending.return_slot) else {
        return;
    };
    let ty = ctx.f.ret;
    if ty.is_copy() {
        return;
    }
    let set = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Bool, Operand::Value(flag), 0),
        Type::Bool,
        None,
    );
    let release = ctx.f.add_block();
    let next = ctx.f.add_block();
    branch(ctx, Operand::Value(set), release, next);
    ctx.cur_block = release;
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(ty, Operand::Value(slot), 0),
        ty,
        None,
    );
    ctx.emit_drop_value(Operand::Value(v), ty);
    let cb = ctx.cur_block;
    ctx.f.set_term(cb, Terminator::Br(next));
    ctx.cur_block = next;
}

fn test(ctx: &mut LowerCtx<'_>, word: crate::ssa::ValueId) -> Operand {
    let k = InstKind::ICmp(IPred::Ne, Operand::Value(word), Operand::ConstI64(0));
    Operand::Value(ctx.f.append_inst(ctx.cur_block, k, Type::Bool, None))
}

fn branch(
    ctx: &mut LowerCtx<'_>,
    cond: Operand,
    then_blk: crate::ssa::BlockId,
    else_blk: crate::ssa::BlockId,
) {
    let cb = ctx.cur_block;
    ctx.f.set_term(
        cb,
        Terminator::CondBr {
            cond,
            then_blk,
            else_blk,
        },
    );
}
