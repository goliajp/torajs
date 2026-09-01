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
//! (`emit_close_under_pending_throw`).
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

use crate::ssa::{IPred, InstKind, Operand, Terminator, Type, ValueId};
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
}

/// The `return()` call alone. An iterator with no `return` closes as
/// a no-op, and so does a loop that never derived one (an indexed
/// receiver leaves the slot at `undefined`), so this is safe for every
/// lane the kernel drives.
fn emit_close_call(ctx: &mut LowerCtx, td: &ForOfTeardown) {
    match td {
        ForOfTeardown::Any { src, iter_slot } => {
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.any_iter_close,
                    vec![src.clone(), Operand::Value(*iter_slot)],
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
    ctx.emit_throw_check(None);
}

/// §7.4.9 on a throw completion: the original throw wins. The pending
/// throw is suspended (taken out of the TLS slot, the way a `finally`
/// entry does) so `return()` runs throw-free — a generator's `return`
/// resumes its body into its `finally` blocks — then whatever
/// `return()` left pending is discarded and the original is put back.
/// No throw-check follows: this runs inside a throw path that already
/// decided where it is going, and a check here would open a second
/// branch and re-run that path's drops.
pub(crate) fn emit_close_under_pending_throw(ctx: &mut LowerCtx, td: &ForOfTeardown) {
    let saved_tag = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.throw_take_tag, vec![]),
        Type::I64,
        None,
    );
    let saved_val = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.throw_take, vec![]),
        Type::I64,
        None,
    );
    emit_close_call(ctx, td);
    // `return()`'s own throw, if any, is ours to drop (throw_take
    // hands the payload over). Only when one is ACTIVE: the tag /
    // value globals keep their last contents after a take, so a
    // clean close — or a kernel that already swallowed the close's
    // throw — would hand back the suspended original here, and
    // dropping that freed the Error the catch was about to read.
    let own_active = ctx.emit_throw_active_load();
    let own_cmp = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Ne, Operand::Value(own_active), Operand::ConstI64(0)),
        Type::Bool,
        None,
    );
    let discard_blk = ctx.f.add_block();
    let restore_blk = ctx.f.add_block();
    let cb = ctx.cur_block;
    ctx.f.set_term(
        cb,
        Terminator::CondBr {
            cond: Operand::Value(own_cmp),
            then_blk: discard_blk,
            else_blk: restore_blk,
        },
    );
    ctx.cur_block = discard_blk;
    let own_tag = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.throw_take_tag, vec![]),
        Type::I64,
        None,
    );
    let own_val = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.throw_take, vec![]),
        Type::I64,
        None,
    );
    let own_boxed = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.any_box,
            vec![Operand::Value(own_tag), Operand::Value(own_val)],
        ),
        Type::Any,
        None,
    );
    ctx.emit_drop_value(Operand::Value(own_boxed), Type::Any);
    let cb2 = ctx.cur_block;
    ctx.f.set_term(cb2, Terminator::Br(restore_blk));
    ctx.cur_block = restore_blk;
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.throw_set,
            vec![Operand::Value(saved_tag), Operand::Value(saved_val)],
        ),
    );
}

/// Every enclosing for-of, innermost first — the LIFO a `return` out
/// of nested loops has to unwind. Emitted into the current block, so
/// the caller owns the placement; the slots release with the fn's
/// owned locals afterwards.
pub(crate) fn emit_all_for_return(ctx: &mut LowerCtx) {
    let pending: Vec<ForOfTeardown> = ctx.for_of_teardown_stack.iter().rev().cloned().collect();
    for td in &pending {
        emit_close(ctx, td);
    }
}

/// The for-ofs a jump leaves — `for_of_teardown_stack[depth..]`,
/// innermost first. A throw completion closes under the suspended
/// pending throw; every other exit closes plainly.
pub(crate) fn emit_closes_from(ctx: &mut LowerCtx, depth: usize, under_throw: bool) {
    if depth >= ctx.for_of_teardown_stack.len() {
        return;
    }
    let pending: Vec<ForOfTeardown> = ctx.for_of_teardown_stack[depth..]
        .iter()
        .rev()
        .cloned()
        .collect();
    for td in &pending {
        if under_throw {
            emit_close_under_pending_throw(ctx, td);
        } else {
            emit_close(ctx, td);
        }
    }
}
