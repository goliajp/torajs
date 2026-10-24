//! §7.4.9 IteratorClose as a record every abrupt exit out of a for-of
//! body can emit.
//!
//! `break` reaches the close because it branches to the loop's exit
//! block, which is where the close lives (RFC 20260725 刀 6 / 6b).
//! `return` doesn't — it leaves for the function epilogue — so the
//! return site walks this stack (`emit_all_for_return`). A throw
//! routing to a catch and a labeled jump over the loop don't reach the
//! exit block either; they walk the stack from the depth their target
//! recorded (`ExitTarget::teardown_depth`, RFC 20260901-scope-exit-drops
//! 刀 2), the throw under a suspended pending throw
//! (`emit_close_under_pending_throw`, through the runtime's abrupt close).
//!
//! The iterator slot's own release is NOT here: the slot is a scoped
//! local of the loop's iter frame since 刀 2, so the frame's close —
//! fall-through, depth drop, fn exit — releases it on every path, and
//! this module only ever calls `return()`. Close reads the slot, so
//! every exit closes before it drops.
//!
//! The exit block guards the close on "did the loop stop early"; a
//! return / throw / labeled jump needs no guard, since leaving IS
//! stopping early.

use crate::ssa::{InstKind, Operand, Type, ValueId};
use crate::ssa_lower::LowerCtx;

/// One live for-of's close, by lane. Both carry the iterator slot;
/// they differ in how the iterator is reached — the any lane hands the
/// source alongside it (the kernel picks the protocol back up from
/// there), the typed lane knows the iterator's static type and boxes
/// the loaded value.
#[derive(Clone)]
pub(crate) enum ForOfTeardown {
    Any {
        src: Operand,
        iter_slot: ValueId,
    },
    Typed {
        iter_slot: ValueId,
        iter_ret_ty: Type,
    },
    /// The module body's `for await` over an array: the close is the
    /// await of what the Async-from-Sync wrapper's `return` answers
    /// (`ssa_lower_fn::suspend::sync_step`).
    AsyncFromSync,
}

/// The `return()` call alone. An iterator with no `return` closes as
/// a no-op, and so does a loop that never derived one (an indexed
/// receiver leaves the slot at `undefined`), so this is safe for every
/// lane the kernel drives.
fn emit_close_call(ctx: &mut LowerCtx, td: &ForOfTeardown) {
    match td {
        ForOfTeardown::AsyncFromSync => crate::ssa_lower_fn::suspend::close_sync_iter(ctx, false),
        ForOfTeardown::Any { src, iter_slot } => {
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.any_iter_close,
                    vec![*src, Operand::Value(*iter_slot)],
                ),
            );
        }
        ForOfTeardown::Typed {
            iter_slot,
            iter_ret_ty,
        } => {
            let iter_for_close = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(*iter_ret_ty, Operand::Value(*iter_slot), 0),
                *iter_ret_ty,
                None,
            );
            let boxed_iter = ctx.box_to_any(Operand::Value(iter_for_close));
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.iter_close_value, vec![boxed_iter]),
            );
        }
    }
}

/// §7.4.9 with the iterator still live, on a normal completion (`break`
/// / `return` / labeled jump): a throw out of `return()` propagates.
pub(crate) fn emit_close(ctx: &mut LowerCtx, td: &ForOfTeardown) {
    emit_close_call(ctx, td);
    // the await of a promise fulfilled at once raises nothing
    if !matches!(td, ForOfTeardown::AsyncFromSync) {
        ctx.emit_throw_check(None);
    }
}

/// §7.4.9 on a throw completion: the original throw wins. The runtime
/// takes the pending throw out of the TLS slot (the way a `finally`
/// entry does) so `return()` runs throw-free — a generator's `return`
/// resumes its body into its `finally` blocks — then discards whatever
/// `return()` left pending and puts the original back. No throw-check
/// follows: this runs inside a throw path that already decided where
/// it is going, and a check here would open a second branch and re-run
/// that path's drops.
pub(crate) fn emit_close_under_pending_throw(ctx: &mut LowerCtx, td: &ForOfTeardown) {
    let call = match td {
        ForOfTeardown::AsyncFromSync => {
            return crate::ssa_lower_fn::suspend::close_sync_iter(ctx, true);
        }
        ForOfTeardown::Any { src, iter_slot } => InstKind::Call(
            ctx.intrinsics.any_iter_close_abrupt,
            vec![*src, Operand::Value(*iter_slot)],
        ),
        ForOfTeardown::Typed {
            iter_slot,
            iter_ret_ty,
        } => {
            let iter = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(*iter_ret_ty, Operand::Value(*iter_slot), 0),
                *iter_ret_ty,
                None,
            );
            let boxed = ctx.box_to_any(Operand::Value(iter));
            InstKind::Call(ctx.intrinsics.iter_close_abrupt, vec![boxed])
        }
    };
    ctx.f.append_void(ctx.cur_block, call);
}

/// Every enclosing for-of, innermost first — the LIFO a `return` out
/// of nested loops has to unwind. Emitted into the current block, so
/// the caller owns the placement; the slots release with the fn's
/// owned locals afterwards.
pub(crate) fn emit_all_for_return(ctx: &mut LowerCtx) {
    emit_closes_from(ctx, 0, false);
}

/// The for-ofs a jump leaves — `for_of_teardown_stack[depth..]`,
/// innermost first. A throw completion closes under the suspended
/// pending throw; every other exit closes plainly.
pub(crate) fn emit_closes_from(ctx: &mut LowerCtx, depth: usize, under_throw: bool) {
    if depth >= ctx.for_of_teardown_stack.len() {
        return;
    }
    // Innermost first. While one loop's close is being emitted, that
    // loop and everything inside it are off the stack: the close's own
    // throw check routes to a handler and closes `[handler.depth..]`
    // on the way (`emit_throw_check`), and with the entry still there
    // the iterator whose `return()` just threw was closed a second
    // time (rotation 652: `try { for (x of it) { try { return } finally
    // {} } } catch {}` logged two closes). The outer loops the throw
    // crosses still close, as §7.4.9 wants; only the one that threw
    // is spent. Restored afterwards — a non-throwing close falls
    // through to the next one.
    let saved: Vec<ForOfTeardown> = ctx.for_of_teardown_stack.clone();
    for idx in (depth..saved.len()).rev() {
        ctx.for_of_teardown_stack.truncate(idx);
        if under_throw {
            emit_close_under_pending_throw(ctx, &saved[idx]);
        } else {
            emit_close(ctx, &saved[idx]);
        }
    }
    ctx.for_of_teardown_stack = saved;
}
