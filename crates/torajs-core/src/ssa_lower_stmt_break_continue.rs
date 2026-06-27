//! `Stmt::Break` / `Stmt::Continue` arms of `LowerCtx::lower_stmt`
//! extracted from [`crate::ssa_lower`] (chunk 154).
//!
//! Pre-extract these arms were 30 + 26 LOC inline inside
//! `lower_stmt` (combined 56 LOC); shared finally-routing
//! structure makes them natural siblings in the same module.
//!
//! M1.7 break/continue semantics:
//!
//! - **`break`** branches to the enclosing loop's break target
//!   (`loop_stack.last().1`). Errors if outside any loop.
//! - **`continue`** branches to the enclosing loop's continue
//!   target (`loop_stack.last().0`).
//!
//! Finally-routing: when a `try-with-finally` sits between the
//! `break` / `continue` and the loop (detected via
//! `try_finally_loop_depth.last() == loop_stack.len()` — i.e. the
//! finally was registered at the same loop depth we'd be exiting),
//! we don't branch directly to the loop target — we route through
//! the finally with a `__pending_break` / `__pending_continue`
//! flag set first. The finally tail dispatches back to the
//! original target once finally-body completes. Without this,
//! `break` would skip finally execution.

use crate::ssa::{InstKind, Operand, Terminator};
use crate::ssa_lower::LowerCtx;

pub(crate) fn lower_break(ctx: &mut LowerCtx) {
    let (_, after) = *ctx
        .loop_stack
        .last()
        .expect("ssa-lower: `break` outside of any loop");
    if let Some(&fb) = ctx.try_finally_stack.last()
        && ctx.try_finally_loop_depth.last().copied() == Some(ctx.loop_stack.len())
    {
        let flag = match ctx.pending_break_flag {
            Some(f) => f,
            None => {
                let f = ctx.alloca_bool_flag_in_entry(Some("__pending_break"));
                ctx.pending_break_flag = Some(f);
                f
            }
        };
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(Operand::ConstBool(true), Operand::Value(flag), 0),
        );
        let cb = ctx.cur_block;
        ctx.f.set_term(cb, Terminator::Br(fb));
    } else {
        ctx.f.set_term(ctx.cur_block, Terminator::Br(after));
    }
}

pub(crate) fn lower_continue(ctx: &mut LowerCtx) {
    let (cont_target, _) = *ctx
        .loop_stack
        .last()
        .expect("ssa-lower: `continue` outside of any loop");
    if let Some(&fb) = ctx.try_finally_stack.last()
        && ctx.try_finally_loop_depth.last().copied() == Some(ctx.loop_stack.len())
    {
        let flag = match ctx.pending_continue_flag {
            Some(f) => f,
            None => {
                let f = ctx.alloca_bool_flag_in_entry(Some("__pending_continue"));
                ctx.pending_continue_flag = Some(f);
                f
            }
        };
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(Operand::ConstBool(true), Operand::Value(flag), 0),
        );
        let cb = ctx.cur_block;
        ctx.f.set_term(cb, Terminator::Br(fb));
        return;
    }
    ctx.f.set_term(ctx.cur_block, Terminator::Br(cont_target));
}
