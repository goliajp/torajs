//! The job-count beats of a `for await` (`sync_step`, `any_step`): the
//! module body runs the job in place (`in_place`); the body of an async
//! function suspends its frame over it, a direct suspension that the
//! queue resumes one job later.

use std::collections::HashMap;

use crate::ast::Ast;
use crate::ssa::{
    FuncId, IPred, InstKind, Module, Operand, SuspendKind, SuspendLane, Terminator, Type,
};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_synthesize_main::declare_intrinsic;

/// `torajs-anyvalue` `iter_any_frame`, and the step read the module
/// body shares.
pub(super) const ANY_NEXT_FRAME: &str = "__torajs_any_iter_next_frame";
pub(super) const ANY_RESULT_STEP: &str = "__torajs_any_iter_result_step";
pub(super) const ANY_CLOSE_FRAME: &str = "__torajs_any_iter_close_frame";
pub(super) const ANY_CLOSE_CHECK: &str = "__torajs_any_iter_close_result_check";

/// Declare the calls of a frame body's `for await` over an `any` value,
/// for a program with a function that suspends over a frame and has a
/// `for await` (`AsyncSsa::frame_for_await`).
pub(crate) fn declare(ast: &Ast, module: &mut Module, fn_table: &mut HashMap<String, FuncId>) {
    if !ast.async_ssa.frame_for_await {
        return;
    }
    let slots = [Type::Any, Type::Ptr, Type::Ptr, Type::Ptr];
    let calls: [(&str, &[Type], Type); 4] = [
        (ANY_NEXT_FRAME, &slots, Type::I64),
        (ANY_RESULT_STEP, &[Type::Any, Type::Ptr], Type::I64),
        (ANY_CLOSE_FRAME, &slots, Type::I64),
        (ANY_CLOSE_CHECK, &[Type::I64], Type::Void),
    ];
    for (name, params, ret) in calls {
        // the module body declares the step read too
        if !fn_table.contains_key(name) {
            declare_intrinsic(module, fn_table, name, params, ret);
        }
    }
}

/// The call `name` [`declare`] put in the function table.
pub(super) fn call(ctx: &LowerCtx<'_>, name: &str) -> FuncId {
    *ctx.fn_table
        .get(name)
        .expect("the frame for-await calls are declared with the bodies that have one")
}

/// One job, queued at once. `kept`: a value the body owns across it,
/// which the frame's trace reports and its cancelled exit releases.
pub(super) fn yield_one(ctx: &mut LowerCtx<'_>, kept: Option<(Operand, Type)>) {
    if super::in_place::in_module_body(ctx) {
        super::in_place::yield_direct(ctx);
        return;
    }
    let aside = super::held::aside(ctx);
    let held = super::held::held(ctx, kept, &aside);
    let cb = ctx.cur_block;
    let kind = InstKind::Suspend(
        SuspendKind::Await,
        SuspendLane::Direct,
        Operand::ConstI64(0),
        held,
    );
    ctx.f.append_void(cb, kind);
    super::cancel::throw_check(ctx, kept, &aside);
}

/// The pending throw, raised again one job later. `abrupt`: inside a
/// throw path, which has already released the temps parked for it.
pub(super) fn yield_raise(ctx: &mut LowerCtx<'_>, abrupt: bool) {
    if super::in_place::in_module_body(ctx) {
        let calls = crate::ssa_lower_fn::async_main::ModuleCalls::of(ctx);
        let cb = ctx.cur_block;
        ctx.f
            .append_void(cb, InstKind::Call(calls.yield_rethrow, Vec::new()));
        return;
    }
    let parked = abrupt.then(|| std::mem::take(&mut ctx.temps.throw_live));
    let boxed = take_throw(ctx);
    let token = ctx.push_throw_temp(boxed, Type::Any);
    yield_one(ctx, None);
    ctx.pop_throw_temp(token);
    restore_throw(ctx, boxed);
    if let Some(parked) = parked {
        ctx.temps.throw_live = parked;
    }
}

/// The pending throw taken out of the slot as an owned `any` box.
pub(super) fn take_throw(ctx: &mut LowerCtx<'_>) -> Operand {
    let cb = ctx.cur_block;
    let i = ctx.intrinsics;
    let tag = ctx.f.append_inst(
        cb,
        InstKind::Call(i.throw_take_tag, vec![]),
        Type::I64,
        None,
    );
    let value = ctx
        .f
        .append_inst(cb, InstKind::Call(i.throw_take, vec![]), Type::I64, None);
    let pair = vec![Operand::Value(tag), Operand::Value(value)];
    Operand::Value(
        ctx.f
            .append_inst(cb, InstKind::Call(i.any_box, pair), Type::Any, None),
    )
}

/// The throw [`take_throw`] boxed, pending again; the box is released.
pub(super) fn restore_throw(ctx: &mut LowerCtx<'_>, boxed: Operand) {
    let cb = ctx.cur_block;
    let i = ctx.intrinsics;
    let tag = InstKind::Call(i.any_unbox_tag, vec![boxed]);
    let tag = ctx.f.append_inst(cb, tag, Type::I64, None);
    let value = InstKind::Call(i.any_unbox_value_owned, vec![boxed]);
    let value = ctx.f.append_inst(cb, value, Type::I64, None);
    ctx.emit_drop_value(boxed, Type::Any);
    let cb = ctx.cur_block;
    let set = vec![Operand::Value(tag), Operand::Value(value)];
    ctx.f.append_void(cb, InstKind::Call(i.throw_set, set));
}

/// `torajs-promise` `STATE_REJECTED`.
const STATE_REJECTED: i32 = 2;

/// A frame body's await whose rejection is raised one job after the
/// continuation (`sync_step`: the Async-from-Sync step's promise is
/// rejected from a reaction): on the owned settled cell `settled`, a
/// rejected one lets one job run before its read raises.
pub(super) fn delay_rejection(ctx: &mut LowerCtx<'_>, settled: Operand) {
    let cb = ctx.cur_block;
    let word = InstKind::Load(Type::I32, settled, super::read::PROMISE_STATE_WORD_OFF);
    let word = ctx.f.append_inst(cb, word, Type::I32, None);
    let state = InstKind::BinOp(
        crate::ssa::BinOp::And,
        Operand::Value(word),
        Operand::ConstI32(0xFF),
    );
    let state = ctx.f.append_inst(cb, state, Type::I32, None);
    let rejected = InstKind::ICmp(
        IPred::Eq,
        Operand::Value(state),
        Operand::ConstI32(STATE_REJECTED),
    );
    let rejected = ctx.f.append_inst(cb, rejected, Type::Bool, None);
    let delay = ctx.f.add_block();
    let join = ctx.f.add_block();
    let term = Terminator::CondBr {
        cond: Operand::Value(rejected),
        then_blk: delay,
        else_blk: join,
    };
    ctx.f.set_term(cb, term);
    ctx.cur_block = delay;
    yield_one(ctx, Some((settled, Type::Promise)));
    let end = ctx.cur_block;
    ctx.f.set_term(end, Terminator::Br(join));
    ctx.cur_block = join;
}
