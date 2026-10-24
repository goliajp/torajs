//! A `for await` over an array in the module body, the one body whose
//! `for await` the statement lowering steps (a function's goes to the
//! state machine, `ast::desugar_async_ssa_gate`). The array's iterator
//! is a sync one, which the loop reaches through an Async-from-Sync
//! iterator (§27.1.6.1), so every step is two jobs: the step's promise
//! is fulfilled by a reaction on PromiseResolve of the element
//! (§27.1.6.4 AsyncFromSyncIteratorContinuation), and the loop's await
//! of that promise goes on one job after it (§14.7.5.6 step 6.d). The
//! step that finds the array exhausted takes the same two. A rejected
//! element rejects the step's promise from that reaction, raised a job
//! later; the reaction first closes the array's iterator
//! (closeOnRejection), which has no `return`, so nothing runs.
//! Leaving the loop early (`break`, a throw out of the body, a jump
//! past it) runs AsyncIteratorClose (§7.4.11): the wrapper's `return`
//! answers a promise fulfilled at once (an array iterator has no
//! `return` of its own), and its await is one job, under a throw
//! completion too.

use super::{await_any_in, await_cell, in_place};
use crate::ast::ExprId;
use crate::ssa::{BlockId, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_fn::async_main::ModuleCalls;
use crate::ssa_lower_member_promise_value as pv;

/// The step reading the element `obj` (an index read): its value, owned,
/// once the step's two jobs have run.
pub(crate) fn await_sync_step(ctx: &mut LowerCtx<'_>, obj: ExprId) -> Operand {
    let v = if matches!(
        ctx.expr_types.get(&obj),
        Some(crate::check::Type::Promise(_))
    ) {
        await_element(ctx, obj)
    } else {
        let op = ctx.lower_expr(obj);
        let ty = ctx.operand_ty(&op);
        ctx.emit_owned_result_inc(op, ty);
        if ty == Type::Any {
            await_any_in(ctx, op, Type::Any, true)
        } else {
            // PromiseResolve of a value that is no promise fulfills a
            // fresh cell, whose reaction is queued at once
            in_place::yield_direct(ctx);
            op
        }
    };
    in_place::yield_direct(ctx);
    v
}

/// The element the checker typed `Promise<T>`, awaited as the cell and
/// read in its inner lane, a rejection raised one job later.
fn await_element(ctx: &mut LowerCtx<'_>, obj: ExprId) -> Operand {
    let src = ctx.lower_expr(obj);
    let held_any = matches!(ctx.operand_ty(&src), Type::Any);
    let ty = if held_any { Type::Any } else { Type::Promise };
    ctx.emit_owned_result_inc(src, ty);
    let inner = pv::recover_inner_ssa_ty(ctx, obj);
    let inner = ctx.widen_promise_inner_ty(inner, obj);
    if held_any {
        return await_any_in(ctx, src, inner.unwrap_or(Type::Any), true);
    }
    let undef = pv::undef_cell_slot(ctx, obj, inner.as_ref());
    await_cell(ctx, src, Some(obj), inner, undef, true)
}

/// The loop's two exits into `after`, built up front: the exhausted
/// step's two jobs, and the close a `break` takes (one job). Answers
/// `(exhausted, closing)`.
pub(crate) fn sync_iter_exits(ctx: &mut LowerCtx<'_>, after: BlockId) -> (BlockId, BlockId) {
    let back = ctx.cur_block;
    let exhausted = ctx.f.add_block();
    ctx.cur_block = exhausted;
    in_place::yield_direct(ctx);
    in_place::yield_direct(ctx);
    ctx.f.set_term(exhausted, Terminator::Br(after));
    let closing = ctx.f.add_block();
    ctx.cur_block = closing;
    in_place::yield_direct(ctx);
    ctx.f.set_term(closing, Terminator::Br(after));
    ctx.cur_block = back;
    (exhausted, closing)
}

/// AsyncIteratorClose of the loop on a jump past it (one job), or on a
/// throw completion, which goes on after that job.
pub(crate) fn close_sync_iter(ctx: &mut LowerCtx<'_>, under_throw: bool) {
    let calls = ModuleCalls::of(ctx);
    let call = if under_throw {
        calls.yield_rethrow
    } else {
        calls.yield_direct
    };
    let cb = ctx.cur_block;
    ctx.f.append_void(cb, InstKind::Call(call, Vec::new()));
}
