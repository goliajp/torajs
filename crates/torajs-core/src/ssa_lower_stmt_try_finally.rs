//! `finally` block lowering for `Stmt::Try` extracted from
//! [`crate::ssa_lower_stmt_try`] (chunk 170 — file-size debt
//! cleanup for chunk 155's sibling).
//!
//! Pre-extract this finally lowering block was 295 LOC inline
//! inside `ssa_lower_stmt_try::lower`. Body verbatim moves here as
//! `lower` free fn — caller passes the finally block, finally body
//! stmts, and the after_blk to branch to on fall-through.
//!
//! P7.5 O5 suspend pending throw at finally entry + 4-way tail
//! dispatch (active throw → propagate via try_stack or fn-ret-
//! sentinel; pending_return → outer-finally or fn-ret-load;
//! pending_break / pending_continue → outer-finally-in-same-loop
//! or loop break/continue target with flag-clear).
//!
//! Chunk 453 split the 292-line body into per-stage emit fns below
//! (suspend / restore / throw-propagate / pending-return / pending-
//! loop-flag); the near-identical break and continue tails deduped
//! into `emit_pending_loop_flag` picking the loop target by side.

use crate::ast::Stmt;
use crate::ssa::{BlockId, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::LowerCtx;

pub(crate) fn lower(ctx: &mut LowerCtx, fb: BlockId, fbody: &[Stmt], after_blk: BlockId) {
    ctx.try_finally_stack.pop();
    ctx.try_finally_loop_depth.pop();
    ctx.cur_block = fb;
    ctx.scope_stack.push(Vec::new());
    ctx.shadow_stack.push(Vec::new());

    let (saved_active_slot, saved_tag_slot, saved_value_slot) = emit_o5_suspend(ctx);
    ctx.pending
        .finally_throws
        .push([saved_active_slot, saved_tag_slot, saved_value_slot]);
    crate::ssa_lower_stmt_let_decl_recursive::hoist_forward_boxes(ctx, fbody.iter());
    for s in fbody {
        ctx.lower_stmt(s);
        if !ctx.cur_open() {
            break;
        }
    }
    ctx.pending.finally_throws.pop();
    // RFC 20260901-scope-exit-drops — the finally body's own frame
    // closes BEFORE the tail dispatch: its locals are dead on every
    // one of the four ways out, and the dispatch's depth-based drops
    // (chain to an outer finally / handler, jump to a loop or label
    // target) must see the scope stack as it stands outside this
    // statement. Pre-RFC the frame was popped without a drop walk.
    ctx.close_scope_frame();
    if ctx.cur_open() {
        emit_o5_restore(ctx, saved_active_slot, saved_tag_slot, saved_value_slot);
        emit_throw_propagate(ctx);
        emit_pending_return(ctx);
        if let Some(flag) = ctx.pending.break_flag {
            emit_pending_loop_flag(ctx, flag, true, after_blk);
        }
        if let Some(flag) = ctx.pending.continue_flag {
            emit_pending_loop_flag(ctx, flag, false, after_blk);
        }
        emit_pending_label_flags(ctx);
        let cb4 = ctx.cur_block;
        ctx.f.set_term(cb4, Terminator::Br(after_blk));
    }
}

/// O5 finally-entry suspend: snapshot the pending throw (active flag,
/// tag, value) into entry allocas, clearing the TLS pending slot so
/// the finally body runs throw-free
fn emit_o5_suspend(ctx: &mut LowerCtx) -> (ValueId, ValueId, ValueId) {
    let saved_active_slot = ctx.alloca_in_entry(Type::I64, Some("__o5_saved_active"));
    let saved_tag_slot = ctx.alloca_in_entry(Type::I64, Some("__o5_saved_tag"));
    let saved_value_slot = ctx.alloca_in_entry(Type::I64, Some("__o5_saved_value"));
    let snap_active = ctx.emit_throw_active_load();
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(
            Operand::Value(snap_active),
            Operand::Value(saved_active_slot),
            0,
        ),
    );
    let snap_tag = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.throw_take_tag, vec![]),
        Type::I64,
        None,
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(snap_tag), Operand::Value(saved_tag_slot), 0),
    );
    let snap_value = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.throw_take, vec![]),
        Type::I64,
        None,
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(
            Operand::Value(snap_value),
            Operand::Value(saved_value_slot),
            0,
        ),
    );
    (saved_active_slot, saved_tag_slot, saved_value_slot)
}

/// O5 finally-exit probe: keep a throw raised by the finally body
/// itself, else restore the suspended one via `throw_set`; falls
/// through with `cur_block` on a fresh dispatch block
fn emit_o5_restore(
    ctx: &mut LowerCtx,
    saved_active_slot: ValueId,
    saved_tag_slot: ValueId,
    saved_value_slot: ValueId,
) {
    let probe_active = ctx.emit_throw_active_load();
    let probe_cmp = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(
            IPred::Ne,
            Operand::Value(probe_active),
            Operand::ConstI64(0),
        ),
        Type::Bool,
        None,
    );
    let dispatch_blk = ctx.f.add_block();
    let check_saved_blk = ctx.f.add_block();
    let cbr = ctx.cur_block;
    ctx.f.set_term(
        cbr,
        Terminator::CondBr {
            cond: Operand::Value(probe_cmp),
            then_blk: dispatch_blk,
            else_blk: check_saved_blk,
        },
    );
    ctx.cur_block = check_saved_blk;
    let sa = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(saved_active_slot), 0),
        Type::I64,
        None,
    );
    let sa_cmp = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Ne, Operand::Value(sa), Operand::ConstI64(0)),
        Type::Bool,
        None,
    );
    let do_restore_blk = ctx.f.add_block();
    let cbr2 = ctx.cur_block;
    ctx.f.set_term(
        cbr2,
        Terminator::CondBr {
            cond: Operand::Value(sa_cmp),
            then_blk: do_restore_blk,
            else_blk: dispatch_blk,
        },
    );
    ctx.cur_block = do_restore_blk;
    let st = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(saved_tag_slot), 0),
        Type::I64,
        None,
    );
    let sv = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(saved_value_slot), 0),
        Type::I64,
        None,
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.throw_set,
            vec![Operand::Value(st), Operand::Value(sv)],
        ),
    );
    let cbr3 = ctx.cur_block;
    ctx.f.set_term(cbr3, Terminator::Br(dispatch_blk));
    ctx.cur_block = dispatch_blk;
}

/// 4-way tail #1 — active throw: branch to the enclosing handler,
/// or emit drops + a zero-sentinel Ret at fn boundary; leaves
/// `cur_block` on the no-throw continuation block
fn emit_throw_propagate(ctx: &mut LowerCtx) {
    let active = ctx.emit_throw_active_load();
    let throw_cmp = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Ne, Operand::Value(active), Operand::ConstI64(0)),
        Type::Bool,
        None,
    );
    let prop_blk = ctx.f.add_block();
    let no_throw_blk = ctx.f.add_block();
    let cb = ctx.cur_block;
    ctx.f.set_term(
        cb,
        Terminator::CondBr {
            cond: Operand::Value(throw_cmp),
            then_blk: prop_blk,
            else_blk: no_throw_blk,
        },
    );
    ctx.cur_block = prop_blk;
    if let Some(handler) = ctx.try_stack.last().copied() {
        crate::ssa_lower_for_of_teardown::emit_closes_from(ctx, handler.teardown_depth, true);
        ctx.emit_drops_for_scopes_from(handler.scope_depth);
        let cb2 = ctx.cur_block;
        ctx.f.set_term(cb2, Terminator::Br(handler.blk));
    } else {
        crate::ssa_lower_for_of_teardown::emit_closes_from(ctx, 0, true);
        ctx.emit_drops_for_owned_locals();
        ctx.emit_propagate_ret();
    }
    ctx.cur_block = no_throw_blk;
}

/// 4-way tail #2 — pending return: branch to the outer finally when
/// nested, else load the fn-ret slot, drop owned locals and Ret. A
/// Void function has a flag and no slot (`ssa_lower_stmt_return`):
/// its pending return is `Ret(None)`.
fn emit_pending_return(ctx: &mut LowerCtx) {
    let Some(flag) = ctx.pending.return_flag else {
        return;
    };
    let slot = ctx.pending.return_slot;
    let f = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Bool, Operand::Value(flag), 0),
        Type::Bool,
        None,
    );
    let ret_blk = ctx.f.add_block();
    let no_ret_blk = ctx.f.add_block();
    let cb3 = ctx.cur_block;
    ctx.f.set_term(
        cb3,
        Terminator::CondBr {
            cond: Operand::Value(f),
            then_blk: ret_blk,
            else_blk: no_ret_blk,
        },
    );
    ctx.cur_block = ret_blk;
    if let Some(outer_fb) = ctx.try_finally_stack.last().copied() {
        // Chaining to the outer finally leaves the outer try body's
        // frames (this statement sat inside them) — and closes the
        // for-ofs between the two tries, which the return site left
        // open for this finally to run first (rotation 652).
        crate::ssa_lower_for_of_teardown::emit_closes_from(ctx, outer_fb.teardown_depth, false);
        ctx.emit_drops_for_scopes_from(outer_fb.scope_depth);
        let cb4 = ctx.cur_block;
        ctx.f.set_term(cb4, Terminator::Br(outer_fb.blk));
    } else {
        // Every for-of still open around this try closes now, after
        // the finally body and outside the try's own handlers.
        crate::ssa_lower_for_of_teardown::emit_all_for_return(ctx);
        let fn_ret_ty = ctx.f.ret;
        let ret = slot.map(|slot| {
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(fn_ret_ty, Operand::Value(slot), 0),
                fn_ret_ty,
                None,
            );
            Operand::Value(v)
        });
        ctx.emit_drops_for_owned_locals();
        let cb4 = ctx.cur_block;
        ctx.f.set_term(cb4, Terminator::Ret(ret));
    }
    ctx.cur_block = no_ret_blk;
}

/// Labeled-jump tails — one dispatch per live pending flag on the
/// label stack (a labeled break/continue that crossed an intervening
/// try-finally set it and branched here). While the popped finally
/// depth still exceeds the label frame's `finally_depth_at_push`,
/// another finally sits between this one and the label — chain to it
/// with the flag kept; otherwise clear the flag and branch to the
/// label's real target.
fn emit_pending_label_flags(ctx: &mut LowerCtx) {
    let frames: Vec<crate::ssa_lower_stmt_break_continue::LabelFrame> =
        ctx.label_stack.iter().map(|(_, f)| *f).collect();
    for frame in frames {
        for (slot, flag) in frame.pending_flags.into_iter().enumerate() {
            let Some(flag) = flag else { continue };
            let take_break = slot == 1;
            let f = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(Type::Bool, Operand::Value(flag), 0),
                Type::Bool,
                None,
            );
            let hit_blk = ctx.f.add_block();
            let no_hit_blk = ctx.f.add_block();
            let cb = ctx.cur_block;
            ctx.f.set_term(
                cb,
                Terminator::CondBr {
                    cond: Operand::Value(f),
                    then_blk: hit_blk,
                    else_blk: no_hit_blk,
                },
            );
            ctx.cur_block = hit_blk;
            let still_intervening = ctx.try_finally_stack.len() > frame.finally_depth_at_push;
            if still_intervening {
                let outer_fb = *ctx.try_finally_stack.last().expect("len checked");
                crate::ssa_lower_for_of_teardown::emit_closes_from(
                    ctx,
                    outer_fb.teardown_depth,
                    false,
                );
                ctx.emit_drops_for_scopes_from(outer_fb.scope_depth);
                let cb2 = ctx.cur_block;
                ctx.f.set_term(cb2, Terminator::Br(outer_fb.blk));
            } else {
                use crate::ssa_lower_stmt_break_continue::LabelTarget;
                let (target, depth, td) = match frame.target {
                    LabelTarget::Loop(idx) => {
                        let lt = ctx.loop_stack[idx];
                        (lt.target(take_break), lt.scope_depth, lt.teardown_depth)
                    }
                    LabelTarget::Block(blk) => (blk, frame.scope_depth, frame.teardown_depth),
                };
                ctx.f.append_void(
                    ctx.cur_block,
                    InstKind::Store(Operand::ConstBool(false), Operand::Value(flag), 0),
                );
                crate::ssa_lower_for_of_teardown::emit_closes_from(ctx, td, false);
                ctx.emit_drops_for_scopes_from(depth);
                let cb2 = ctx.cur_block;
                ctx.f.set_term(cb2, Terminator::Br(target));
            }
            ctx.cur_block = no_hit_blk;
        }
    }
}

/// 4-way tails #3/#4 — pending break (`take_break`) / continue: to the
/// outer finally when it sits in the same loop, else to the loop's
/// break/continue target with the flag cleared, else `after_blk`
/// (loop-less break inside switch-lowered code)
fn emit_pending_loop_flag(ctx: &mut LowerCtx, flag: ValueId, take_break: bool, after_blk: BlockId) {
    let f = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Bool, Operand::Value(flag), 0),
        Type::Bool,
        None,
    );
    let hit_blk = ctx.f.add_block();
    let no_hit_blk = ctx.f.add_block();
    let cb3 = ctx.cur_block;
    ctx.f.set_term(
        cb3,
        Terminator::CondBr {
            cond: Operand::Value(f),
            then_blk: hit_blk,
            else_blk: no_hit_blk,
        },
    );
    ctx.cur_block = hit_blk;
    let target_idx = ctx.jump_loop(take_break);
    let outer_in_same_loop = target_idx.is_some_and(|i| ctx.finally_inside_loop(i));
    if outer_in_same_loop && let Some(outer_fb) = ctx.try_finally_stack.last().copied() {
        crate::ssa_lower_for_of_teardown::emit_closes_from(ctx, outer_fb.teardown_depth, false);
        ctx.emit_drops_for_scopes_from(outer_fb.scope_depth);
        let cb4 = ctx.cur_block;
        ctx.f.set_term(cb4, Terminator::Br(outer_fb.blk));
    } else if let Some(lt) = target_idx.map(|i| ctx.loop_stack[i]) {
        let target = lt.target(take_break);
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(Operand::ConstBool(false), Operand::Value(flag), 0),
        );
        // The for-ofs and frames between the loop and the try die
        // here — the break site only released the ones inside the
        // try body.
        crate::ssa_lower_for_of_teardown::emit_closes_from(ctx, lt.teardown_depth, false);
        ctx.emit_drops_for_scopes_from(lt.scope_depth);
        let cb4 = ctx.cur_block;
        ctx.f.set_term(cb4, Terminator::Br(target));
    } else {
        let cb4 = ctx.cur_block;
        ctx.f.set_term(cb4, Terminator::Br(after_blk));
    }
    ctx.cur_block = no_hit_blk;
}
