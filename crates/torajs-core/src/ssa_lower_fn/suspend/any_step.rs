//! A `for await` over an `any` value in the module body (an async
//! function's body suspends at the same points, `any_frame`): the step runs
//! the runtime's iteration cascade with every await left to the body
//! (`torajs-anyvalue` `__torajs_any_iter_next_defer`), which awaits in
//! place what the step hands back:
//!
//! - an async iterator's `next()` result (§14.7.5.6 step 6.d): awaited,
//!   one job after it settles, then read as the IteratorResult
//!   (`__torajs_any_iter_result_step`);
//! - a sync lane's value, under the Async-from-Sync iterator
//!   (§27.1.6.4): awaited, and the loop goes on one job after the
//!   reaction that fulfills the step's promise, as `sync_step` does for
//!   an array; a rejection rejects that promise from the reaction, which
//!   first closes the sync iterator (closeOnRejection), and is raised a
//!   job later;
//! - a sync lane run out: the exhausted step's two jobs.
//!
//! Leaving the loop awaits its close in place, under a throw completion
//! too (`torajs-anyvalue` `__torajs_any_iter_close_defer`): an async
//! iterator's `return()` answer, or the Async-from-Sync wrapper's
//! `return`.

use super::frame_step::{ANY_RESULT_STEP, call, yield_one};
use super::{any_frame, await_any_in, in_place};
use crate::ssa::{BlockId, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_fn::async_main::ModuleCalls;
use crate::ssa_lower_for_of_teardown::ForOfTeardown;

/// `torajs-anyvalue` `STEP_AWAIT_VALUE` / `STEP_AWAIT_RESULT`.
const STEP_AWAIT_VALUE: i64 = 2;
const STEP_AWAIT_RESULT: i64 = 3;

/// The loop's close over the same slots as the step; `abrupt`: under
/// the throw the loop is left by, which no check follows
/// (`ssa_lower_for_of_teardown::emit_close_under_pending_throw`).
pub(crate) fn any_close(
    ctx: &mut LowerCtx<'_>,
    src: Operand,
    idx_slot: ValueId,
    iter_slot: ValueId,
    abrupt: bool,
) {
    if !in_place::in_module_body(ctx) {
        return any_frame::close(ctx, src, idx_slot, iter_slot, abrupt);
    }
    let calls = ModuleCalls::of(ctx);
    let args = vec![
        src,
        Operand::Value(idx_slot),
        Operand::Value(iter_slot),
        Operand::ConstI64(abrupt as i64),
    ];
    let cb = ctx.cur_block;
    ctx.f
        .append_void(cb, InstKind::Call(calls.any_close_defer, args));
    if !abrupt {
        // the cancel throw of an await in it takes its own exit
        let aside = super::held::aside(ctx);
        super::cancel::throw_check(ctx, None, &aside);
    }
}

/// A sync lane's rejected value under the step (`ForOfTeardown::
/// AnyStepRejected`): the sync iterator closes, and the throw goes on a
/// job later, as the step's promise rejects then.
pub(crate) fn close_rejected_step(ctx: &mut LowerCtx<'_>, src: Operand, iter_slot: ValueId) {
    let close = ctx.intrinsics.any_iter_close_abrupt;
    let cb = ctx.cur_block;
    let args = vec![src, Operand::Value(iter_slot)];
    ctx.f.append_void(cb, InstKind::Call(close, args));
    super::close_sync_iter(ctx, true);
}

/// One step over the owned `src` with the loop's cursor, iterator and
/// out slots: answers 1 with the value owned in `out`, 0 when the loop
/// is done (a throw in flight has already left through its check).
pub(crate) fn any_await_step(
    ctx: &mut LowerCtx<'_>,
    src: Operand,
    idx_slot: ValueId,
    iter_slot: ValueId,
    out_slot: ValueId,
) -> ValueId {
    let args = vec![
        src,
        Operand::Value(idx_slot),
        Operand::Value(iter_slot),
        Operand::Value(out_slot),
    ];
    let code = if in_place::in_module_body(ctx) {
        let calls = ModuleCalls::of(ctx);
        let cb = ctx.cur_block;
        let code = InstKind::Call(calls.any_next_defer, args);
        let code = ctx.f.append_inst(cb, code, Type::I64, None);
        ctx.emit_throw_check(None);
        code
    } else {
        any_frame::next_step(ctx, args)
    };
    let live = ctx.alloca_in_entry(Type::I64, Some("__forawait_live"));
    let join = ctx.f.add_block();
    let [exhausted, value, result, ready] = dispatch(ctx, code);

    ctx.cur_block = exhausted;
    yield_one(ctx, None);
    yield_one(ctx, None);
    store_and_join(ctx, Operand::ConstI64(0), live, join);

    ctx.cur_block = value;
    await_value(ctx, src, iter_slot, out_slot);
    store_and_join(ctx, Operand::ConstI64(1), live, join);

    ctx.cur_block = result;
    let got = await_result(ctx, out_slot);
    store_and_join(ctx, got, live, join);

    ctx.cur_block = ready;
    store_and_join(ctx, Operand::ConstI64(1), live, join);

    ctx.cur_block = join;
    let load = InstKind::Load(Type::I64, Operand::Value(live), 0);
    ctx.f.append_inst(join, load, Type::I64, None)
}

/// A sync lane's value in `out_slot`, awaited there: one job after it
/// settles, two in all; a rejection closes the sync iterator.
fn await_value(ctx: &mut LowerCtx<'_>, src: Operand, iter_slot: ValueId, out_slot: ValueId) {
    let v = load_any(ctx, out_slot);
    let rejected = ForOfTeardown::AnyStepRejected { src, iter_slot };
    ctx.for_of_teardown_stack.push(rejected);
    let v = await_any_in(ctx, v, Type::Any, false);
    ctx.for_of_teardown_stack.pop();
    let cb = ctx.cur_block;
    ctx.f
        .append_void(cb, InstKind::Store(v, Operand::Value(out_slot), 0));
    yield_one(ctx, Some((v, Type::Any)));
}

/// An async iterator's `next()` answer in `out_slot`, awaited and read
/// as the IteratorResult: 1 with its value in `out_slot`, else 0.
fn await_result(ctx: &mut LowerCtx<'_>, out_slot: ValueId) -> Operand {
    let result_step = call(ctx, ANY_RESULT_STEP);
    let p = load_any(ctx, out_slot);
    let step = await_any_in(ctx, p, Type::Any, false);
    let cb = ctx.cur_block;
    let read = InstKind::Call(result_step, vec![step, Operand::Value(out_slot)]);
    let got = ctx.f.append_inst(cb, read, Type::I64, None);
    ctx.emit_throw_check(None);
    Operand::Value(got)
}

/// The step's answer `code` branched to its four blocks: done, a sync
/// lane's value to await, an async iterator's answer to await, ready.
fn dispatch(ctx: &mut LowerCtx<'_>, code: ValueId) -> [BlockId; 4] {
    let exhausted = ctx.f.add_block();
    let value = ctx.f.add_block();
    let result = ctx.f.add_block();
    let ready = ctx.f.add_block();
    let not_done = ctx.f.add_block();
    let not_value = ctx.f.add_block();
    branch_eq(ctx, code, 0, exhausted, not_done);
    ctx.cur_block = not_done;
    branch_eq(ctx, code, STEP_AWAIT_VALUE, value, not_value);
    ctx.cur_block = not_value;
    branch_eq(ctx, code, STEP_AWAIT_RESULT, result, ready);
    [exhausted, value, result, ready]
}

fn load_any(ctx: &mut LowerCtx<'_>, slot: ValueId) -> Operand {
    let cb = ctx.cur_block;
    let load = InstKind::Load(Type::Any, Operand::Value(slot), 0);
    Operand::Value(ctx.f.append_inst(cb, load, Type::Any, None))
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

fn store_and_join(ctx: &mut LowerCtx<'_>, v: Operand, slot: ValueId, join: BlockId) {
    let cb = ctx.cur_block;
    ctx.f
        .append_void(cb, InstKind::Store(v, Operand::Value(slot), 0));
    ctx.f.set_term(cb, Terminator::Br(join));
}
