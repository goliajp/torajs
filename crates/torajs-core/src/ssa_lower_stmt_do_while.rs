//! `Stmt::DoWhile` arm of `LowerCtx::lower_stmt` extracted from
//! [`crate::ssa_lower`] (chunk 153).
//!
//! Pre-extract this arm was 33 LOC inline inside `lower_stmt`.
//! Body verbatim moved here as a free fn; lower_stmt's match arm
//! delegates with one line.
//!
//! `do { body } while (cond)` — body runs at least once, then
//! `cond` decides whether to repeat:
//!
//! ```text
//!   br body_blk
//! body_blk:
//!   <body>
//!   br cond_blk        (if cur_open after body)
//! cond_blk:
//!   c = lower_expr(cond) → coerce_to_bool
//!   cond_br c, body_blk, after
//! after:
//! ```
//!
//! `break` inside body jumps to `after`; `continue` jumps to
//! `cond_blk` so the loop condition still re-evaluates (matches
//! standard JS semantics).

use crate::ast::Stmt;
use crate::ssa::Terminator;
use crate::ssa_lower::LowerCtx;

pub(crate) fn lower(ctx: &mut LowerCtx, body: &Stmt, cond: crate::ast::ExprId) {
    let body_blk = ctx.f.add_block();
    let cond_blk = ctx.f.add_block();
    let after = ctx.f.add_block();

    ctx.f.set_term(ctx.cur_block, Terminator::Br(body_blk));

    // RFC 20260901-scope-exit-drops — no loop-owned frame; a jump
    // owes the body's frames pushed by `lower_stmt(body)`.
    ctx.loop_stack.push(crate::ssa_lower_scope_exit::LoopTargets {
        cont: cond_blk,
        brk: after,
        scope_depth: ctx.scope_stack.len(),
    });
    ctx.cur_block = body_blk;
    ctx.lower_stmt(body);
    if ctx.cur_open() {
        ctx.f.set_term(ctx.cur_block, Terminator::Br(cond_blk));
    }
    ctx.loop_stack.pop();

    ctx.cur_block = cond_blk;
    let raw = ctx.lower_expr(cond);
    let c = ctx.coerce_to_bool(raw.clone());
    // Chunk 636 — release an owned condition temp after the
    // truthiness test (see ssa_lower_stmt_if.rs); re-evaluated
    // per iteration, so the release runs per iteration too.
    ctx.release_owned_temp(cond, &raw);
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: c,
            then_blk: body_blk,
            else_blk: after,
        },
    );

    ctx.cur_block = after;
}
