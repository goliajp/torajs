//! `Stmt::Block` arm of `LowerCtx::lower_stmt` extracted from
//! [`crate::ssa_lower`] (chunk 151).
//!
//! Pre-extract this arm was 62 LOC inline inside `lower_stmt`.
//! Body verbatim moved here as a free fn; lower_stmt's match arm
//! delegates with one line.
//!
//! M1.3 scope semantics:
//!
//! - Push a fresh scope frame (mirrored by shadow frame so bindings
//!   shadowed inside the block can be restored on close).
//! - Lower each stmt; on a closed block (e.g. early `return` /
//!   `throw` / `break`), stop iteration — the close step skips the
//!   per-binding drops because the exit that closed the block
//!   already released them (fn-wide via `emit_drops_for_owned_locals`,
//!   or by depth via `emit_drops_for_scopes_from`).
//! - On a fall-through close, drop owners declared at this depth in
//!   declaration order (skip moved, Copy, stack-alloca'd locals) —
//!   `LowerCtx::close_scope_frame` (RFC 20260901-scope-exit-drops).
//! - Remove the block's bindings from `locals` so outer code can't
//!   reference them + end-of-fn drop emission doesn't double-drop.
//! - Restore shadowed outer-scope bindings (without this,
//!   `let x = 10; { let x = 99 } x` would crash since the inner
//!   close removed `x` along with the outer entry).
//!
//! `try_lower_while_fast` peeks at consecutive stmt pairs to spot
//! the `let i = 0; while (i < N) { …; i++ }` desugar pattern and
//! emit a tighter loop — preserved verbatim.

use crate::ast::Stmt;
use crate::ssa_lower::LowerCtx;

pub(crate) fn lower(ctx: &mut LowerCtx, stmts: &[Stmt]) {
    ctx.scope_stack.push(Vec::new());
    ctx.shadow_stack.push(Vec::new());
    crate::ssa_lower_stmt_let_decl_recursive::hoist_forward_boxes(ctx, stmts.iter());
    let mut prev: Option<&Stmt> = None;
    for s in stmts {
        if !ctx.try_lower_while_fast(prev, s) {
            ctx.lower_stmt(s);
        }
        if !ctx.cur_open() {
            break;
        }
        prev = Some(s);
    }
    // RFC 20260901-scope-exit-drops — the close protocol (drop on a
    // still-open block, remove, restore shadows) lives on the ctx so
    // the jump sites that leave this frame early release the same
    // set the same way.
    ctx.close_scope_frame();
}
