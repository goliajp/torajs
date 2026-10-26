//! A `for await` over an `any` value in the body of an async function
//! that suspends over a frame: the module body's loop (`any_step`) with
//! the runtime's steps answering what to await instead of running jobs
//! in place (`torajs-anyvalue` `iter_any_frame`).
//!
//! The close (§7.4.11) is driven the same way. Under a throw completion
//! the original throw is set aside first, held across the close's
//! suspensions as a parked temp (the frame's trace reports it, a
//! cancelled exit releases it), every throw the close raises is
//! discarded, and the original is pending again at the end (step 6).

use super::frame_step::{
    ANY_CLOSE_CHECK, ANY_CLOSE_FRAME, ANY_NEXT_FRAME, call, restore_throw, take_throw, yield_one,
    yield_raise,
};
use super::{read::read_settled, suspend_on};
use crate::ssa::{BlockId, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_promise_repr_mark::REPR_ANY;

/// `torajs-anyvalue` `STEP_RAISE_LATER`.
const STEP_RAISE_LATER: i64 = 4;

/// `torajs-anyvalue` `CLOSE_*`.
const CLOSE_YIELD: i64 = 1;
const CLOSE_YIELD_RAISE: i64 = 2;
const CLOSE_AWAIT_RESULT: i64 = 3;
const CLOSE_AWAIT_VALUE: i64 = 4;

/// The step call over the owned `src` with the loop's slots, its sync
/// lane's throw raised a job later; answers the step's code, with any
/// other throw checked.
pub(super) fn next_step(ctx: &mut LowerCtx<'_>, args: Vec<Operand>) -> ValueId {
    let next = call(ctx, ANY_NEXT_FRAME);
    let cb = ctx.cur_block;
    let code = ctx
        .f
        .append_inst(cb, InstKind::Call(next, args), Type::I64, None);
    let later = ctx.f.add_block();
    let join = ctx.f.add_block();
    branch_eq(ctx, code, STEP_RAISE_LATER, later, join);
    ctx.cur_block = later;
    yield_raise(ctx, false);
    br(ctx, join);
    ctx.cur_block = join;
    ctx.emit_throw_check(None);
    code
}

/// The loop's close; `abrupt`: under the throw the loop is left by.
pub(super) fn close(
    ctx: &mut LowerCtx<'_>,
    src: Operand,
    idx_slot: ValueId,
    iter_slot: ValueId,
    abrupt: bool,
) {
    let parked = abrupt.then(|| std::mem::take(&mut ctx.temps.throw_live));
    let original = abrupt.then(|| {
        let boxed = take_throw(ctx);
        (boxed, ctx.push_throw_temp(boxed, Type::Any))
    });
    let out = ctx.alloca_in_entry(Type::Any, Some("__forawait_close"));
    let args = vec![
        src,
        Operand::Value(idx_slot),
        Operand::Value(iter_slot),
        Operand::Value(out),
    ];
    let close = call(ctx, ANY_CLOSE_FRAME);
    let cb = ctx.cur_block;
    let code = ctx
        .f
        .append_inst(cb, InstKind::Call(close, args), Type::I64, None);
    let join = ctx.f.add_block();
    let arms = [
        CLOSE_YIELD,
        CLOSE_YIELD_RAISE,
        CLOSE_AWAIT_RESULT,
        CLOSE_AWAIT_VALUE,
    ]
    .map(|k| (k, ctx.f.add_block()));
    for (k, blk) in arms {
        let next = ctx.f.add_block();
        branch_eq(ctx, code, k, blk, next);
        ctx.cur_block = next;
    }
    // CLOSE_DONE: what the close raised, if anything
    settle(ctx, abrupt);
    br(ctx, join);
    for (k, blk) in arms {
        ctx.cur_block = blk;
        match k {
            CLOSE_YIELD => yield_one(ctx, None),
            CLOSE_YIELD_RAISE if abrupt => {
                discard(ctx);
                yield_one(ctx, None);
            }
            CLOSE_YIELD_RAISE => {
                yield_raise(ctx, false);
                ctx.emit_throw_check(None);
            }
            CLOSE_AWAIT_RESULT => await_result(ctx, out, abrupt),
            _ => await_value(ctx, out, abrupt),
        }
        br(ctx, join);
    }
    ctx.cur_block = join;
    if let Some((boxed, token)) = original {
        ctx.pop_throw_temp(token);
        restore_throw(ctx, boxed);
    }
    if let Some(parked) = parked {
        ctx.temps.throw_live = parked;
    }
}

/// An async iterator's `return()` answer in `out`, awaited: its
/// rejection raises at once, a fulfillment that is no object is a
/// TypeError (§7.4.11 steps 6-9).
fn await_result(ctx: &mut LowerCtx<'_>, out: ValueId, abrupt: bool) {
    let settled = resolve_and_suspend(ctx, out, abrupt);
    if abrupt {
        read_discarding(ctx, settled);
    } else {
        let raw = read_settled(ctx, settled, REPR_ANY);
        let check = call(ctx, ANY_CLOSE_CHECK);
        let cb = ctx.cur_block;
        ctx.f
            .append_void(cb, InstKind::Call(check, vec![Operand::Value(raw)]));
        ctx.emit_throw_check_owned(None, settled, Type::Promise);
    }
    ctx.emit_drop_value(settled, Type::Promise);
}

/// The Async-from-Sync wrapper's `value` in `out`, awaited (§27.1.6.4):
/// the close goes on one job after it settles, a rejection raised then.
fn await_value(ctx: &mut LowerCtx<'_>, out: ValueId, abrupt: bool) {
    let settled = resolve_and_suspend(ctx, out, abrupt);
    if abrupt {
        read_discarding(ctx, settled);
    } else {
        super::frame_step::delay_rejection(ctx, settled);
        read_settled(ctx, settled, REPR_ANY);
    }
    ctx.emit_drop_value(settled, Type::Promise);
    yield_one(ctx, None);
}

/// PromiseResolve of the owned value in `out`, then the suspension on
/// it: answers the settled cell, owned. Under a throw completion a
/// throwing `then` read is discarded (the cell it answers is rejected).
fn resolve_and_suspend(ctx: &mut LowerCtx<'_>, out: ValueId, abrupt: bool) -> Operand {
    let cb = ctx.cur_block;
    let v = InstKind::Load(Type::Any, Operand::Value(out), 0);
    let v = Operand::Value(ctx.f.append_inst(cb, v, Type::Any, None));
    let resolve = InstKind::Call(ctx.intrinsics.promise_resolve_any, vec![v]);
    let cell = Operand::Value(ctx.f.append_inst(cb, resolve, Type::Promise, None));
    settle(ctx, abrupt);
    suspend_on(ctx, cell, Type::Any)
}

/// The settled cell's read with what it raises discarded.
fn read_discarding(ctx: &mut LowerCtx<'_>, settled: Operand) {
    let cb = ctx.cur_block;
    let read = InstKind::Call(
        ctx.intrinsics.promise_get_value_as,
        vec![settled, Operand::ConstI64(REPR_ANY)],
    );
    ctx.f.append_inst(cb, read, Type::I64, None);
    discard(ctx);
}

/// Drop the pending throw, if there is one.
fn discard(ctx: &mut LowerCtx<'_>) {
    let active = ctx.emit_throw_active_load();
    let cb = ctx.cur_block;
    let raised = InstKind::ICmp(IPred::Ne, Operand::Value(active), Operand::ConstI64(0));
    let raised = ctx.f.append_inst(cb, raised, Type::Bool, None);
    let drop_blk = ctx.f.add_block();
    let join = ctx.f.add_block();
    let term = Terminator::CondBr {
        cond: Operand::Value(raised),
        then_blk: drop_blk,
        else_blk: join,
    };
    ctx.f.set_term(cb, term);
    ctx.cur_block = drop_blk;
    let boxed = take_throw(ctx);
    ctx.emit_drop_value(boxed, Type::Any);
    br(ctx, join);
    ctx.cur_block = join;
}

/// What the close raised: discarded under a throw completion, else
/// checked.
fn settle(ctx: &mut LowerCtx<'_>, abrupt: bool) {
    if abrupt {
        discard(ctx);
    } else {
        ctx.emit_throw_check(None);
    }
}

fn branch_eq(ctx: &mut LowerCtx<'_>, code: ValueId, k: i64, then_blk: BlockId, else_blk: BlockId) {
    let cb = ctx.cur_block;
    let eq = InstKind::ICmp(IPred::Eq, Operand::Value(code), Operand::ConstI64(k));
    let eq = ctx.f.append_inst(cb, eq, Type::Bool, None);
    let term = Terminator::CondBr {
        cond: Operand::Value(eq),
        then_blk,
        else_blk,
    };
    ctx.f.set_term(cb, term);
}

fn br(ctx: &mut LowerCtx<'_>, to: BlockId) {
    let cb = ctx.cur_block;
    ctx.f.set_term(cb, Terminator::Br(to));
}
