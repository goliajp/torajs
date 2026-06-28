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
//! or loop break/continue target with flag-clear). Body unchanged.

use crate::ast::Stmt;
use crate::ssa::{BlockId, IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn lower(ctx: &mut LowerCtx, fb: BlockId, fbody: &[Stmt], after_blk: BlockId) {
    ctx.try_finally_stack.pop();
    ctx.try_finally_loop_depth.pop();
    ctx.cur_block = fb;
    ctx.scope_stack.push(Vec::new());
    ctx.shadow_stack.push(Vec::new());

    let saved_active_slot = ctx.alloca_in_entry(Type::I64, Some("__o5_saved_active"));
    let saved_tag_slot = ctx.alloca_in_entry(Type::I64, Some("__o5_saved_tag"));
    let saved_value_slot = ctx.alloca_in_entry(Type::I64, Some("__o5_saved_value"));
    let snap_active = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.throw_check, vec![]),
        Type::I64,
        None,
    );
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
    for s in fbody {
        ctx.lower_stmt(s);
        if !ctx.cur_open() {
            break;
        }
    }
    if ctx.cur_open() {
        let probe_active = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.throw_check, vec![]),
            Type::I64,
            None,
        );
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

        let active = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.throw_check, vec![]),
            Type::I64,
            None,
        );
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
            let cb2 = ctx.cur_block;
            ctx.f.set_term(cb2, Terminator::Br(handler));
        } else {
            ctx.emit_drops_for_owned_locals();
            let cb2 = ctx.cur_block;
            let ret_ty = ctx.f.ret;
            let prop_term = match ret_ty {
                Type::Void => Terminator::Ret(None),
                Type::F64 => Terminator::Ret(Some(Operand::ConstF64(0.0))),
                Type::I32 => Terminator::Ret(Some(Operand::ConstI32(0))),
                Type::Bool => Terminator::Ret(Some(Operand::ConstBool(false))),
                _ => Terminator::Ret(Some(Operand::ConstI64(0))),
            };
            ctx.f.set_term(cb2, prop_term);
        }

        ctx.cur_block = no_throw_blk;
        if let (Some(slot), Some(flag)) = (ctx.pending_return_slot, ctx.pending_return_flag) {
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
                let cb4 = ctx.cur_block;
                ctx.f.set_term(cb4, Terminator::Br(outer_fb));
            } else {
                let fn_ret_ty = ctx.f.ret;
                let v = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Load(fn_ret_ty, Operand::Value(slot), 0),
                    fn_ret_ty,
                    None,
                );
                ctx.emit_drops_for_owned_locals();
                let cb4 = ctx.cur_block;
                ctx.f
                    .set_term(cb4, Terminator::Ret(Some(Operand::Value(v))));
            }
            ctx.cur_block = no_ret_blk;
        }
        if let Some(flag) = ctx.pending_break_flag {
            let f = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(Type::Bool, Operand::Value(flag), 0),
                Type::Bool,
                None,
            );
            let brk_blk = ctx.f.add_block();
            let no_brk_blk = ctx.f.add_block();
            let cb3 = ctx.cur_block;
            ctx.f.set_term(
                cb3,
                Terminator::CondBr {
                    cond: Operand::Value(f),
                    then_blk: brk_blk,
                    else_blk: no_brk_blk,
                },
            );
            ctx.cur_block = brk_blk;
            let cur_loop_len = ctx.loop_stack.len();
            let outer_in_same_loop =
                ctx.try_finally_loop_depth.last().copied() == Some(cur_loop_len);
            if outer_in_same_loop && let Some(outer_fb) = ctx.try_finally_stack.last().copied() {
                let cb4 = ctx.cur_block;
                ctx.f.set_term(cb4, Terminator::Br(outer_fb));
            } else if let Some((_, brk_target)) = ctx.loop_stack.last().copied() {
                ctx.f.append_void(
                    ctx.cur_block,
                    InstKind::Store(Operand::ConstBool(false), Operand::Value(flag), 0),
                );
                let cb4 = ctx.cur_block;
                ctx.f.set_term(cb4, Terminator::Br(brk_target));
            } else {
                let cb4 = ctx.cur_block;
                ctx.f.set_term(cb4, Terminator::Br(after_blk));
            }
            ctx.cur_block = no_brk_blk;
        }
        if let Some(flag) = ctx.pending_continue_flag {
            let f = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(Type::Bool, Operand::Value(flag), 0),
                Type::Bool,
                None,
            );
            let cont_blk = ctx.f.add_block();
            let no_cont_blk = ctx.f.add_block();
            let cb3 = ctx.cur_block;
            ctx.f.set_term(
                cb3,
                Terminator::CondBr {
                    cond: Operand::Value(f),
                    then_blk: cont_blk,
                    else_blk: no_cont_blk,
                },
            );
            ctx.cur_block = cont_blk;
            let cur_loop_len = ctx.loop_stack.len();
            let outer_in_same_loop =
                ctx.try_finally_loop_depth.last().copied() == Some(cur_loop_len);
            if outer_in_same_loop && let Some(outer_fb) = ctx.try_finally_stack.last().copied() {
                let cb4 = ctx.cur_block;
                ctx.f.set_term(cb4, Terminator::Br(outer_fb));
            } else if let Some((cont_target, _)) = ctx.loop_stack.last().copied() {
                ctx.f.append_void(
                    ctx.cur_block,
                    InstKind::Store(Operand::ConstBool(false), Operand::Value(flag), 0),
                );
                let cb4 = ctx.cur_block;
                ctx.f.set_term(cb4, Terminator::Br(cont_target));
            } else {
                let cb4 = ctx.cur_block;
                ctx.f.set_term(cb4, Terminator::Br(after_blk));
            }
            ctx.cur_block = no_cont_blk;
        }
        let cb4 = ctx.cur_block;
        ctx.f.set_term(cb4, Terminator::Br(after_blk));
    }
    ctx.scope_stack.pop();
    let finally_shadows = ctx.shadow_stack.pop().unwrap_or_default();
    for (name, prev) in finally_shadows {
        ctx.locals.insert(name, prev);
    }
}
