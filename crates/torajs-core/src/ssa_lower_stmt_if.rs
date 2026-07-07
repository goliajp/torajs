//! `Stmt::If` arm of `LowerCtx::lower_stmt` extracted from
//! [`crate::ssa_lower`] (chunk 152).
//!
//! Pre-extract this arm was 43 LOC inline inside `lower_stmt`.
//! Body verbatim moved here as a free fn; lower_stmt's match arm
//! delegates with one line.
//!
//! Standard if-else lowering:
//!
//! ```text
//!   c = lower_expr(cond) → coerce_to_bool
//!   cond_br c, then_blk, (else_blk | after_blk)
//! then_blk:
//!   <then>
//!   br after_blk         (if open)
//! [else_blk:
//!   <else>
//!   br after_blk         (if open)]
//! after_blk:
//! ```
//!
//! No-else case: `cond_br false → after_blk` directly. Skipping the
//! empty pass-through block matches the `demo_fib40()` reference
//! layout exactly + saves one BB / phi-edge LLVM has to fold.

use crate::ast::Stmt;
use crate::ssa::Terminator;
use crate::ssa_lower::LowerCtx;

pub(crate) fn lower(
    ctx: &mut LowerCtx,
    cond: crate::ast::ExprId,
    then_branch: &Stmt,
    else_branch: Option<&Stmt>,
) {
    let raw = ctx.lower_expr(cond);
    let c = ctx.coerce_to_bool(raw.clone());
    // Chunk 636 — the condition value is consumed by the truthiness
    // test alone; an owned temp (`if (wr.deref())`, `if (f())`) has
    // no other release site (probe l16d: the deref target stayed
    // alive after its only strong ref died). coerce_to_bool only
    // reads the operand, so releasing after it is safe.
    ctx.release_owned_temp(cond, &raw);
    let then_blk = ctx.f.add_block();
    let after_blk = ctx.f.add_block();

    let else_blk = if else_branch.is_some() {
        ctx.f.add_block()
    } else {
        after_blk
    };

    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: c,
            then_blk,
            else_blk,
        },
    );

    ctx.cur_block = then_blk;
    ctx.lower_stmt(then_branch);
    if ctx.cur_open() {
        ctx.f.set_term(ctx.cur_block, Terminator::Br(after_blk));
    }

    if let Some(eb) = else_branch {
        ctx.cur_block = else_blk;
        ctx.lower_stmt(eb);
        if ctx.cur_open() {
            ctx.f.set_term(ctx.cur_block, Terminator::Br(after_blk));
        }
    }

    ctx.cur_block = after_blk;
}
