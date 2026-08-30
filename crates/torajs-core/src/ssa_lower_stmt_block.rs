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
//!   `throw`), flag `early_exit` and stop iteration — the close
//!   step skips inner per-binding drops because the early-exit
//!   path already walked the full locals map via
//!   `emit_drops_for_owned_locals`.
//! - On a fall-through close, drop owners declared at this depth in
//!   declaration order (skip moved, Copy, stack-alloca'd locals).
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
use crate::ssa::{InstKind, Operand};
use crate::ssa_lower::LowerCtx;

pub(crate) fn lower(ctx: &mut LowerCtx, stmts: &[Stmt]) {
    ctx.scope_stack.push(Vec::new());
    ctx.shadow_stack.push(Vec::new());
    crate::ssa_lower_stmt_let_decl_recursive::hoist_forward_boxes(ctx, stmts.iter());
    let mut early_exit = false;
    let mut prev: Option<&Stmt> = None;
    for s in stmts {
        if !ctx.try_lower_while_fast(prev, s) {
            ctx.lower_stmt(s);
        }
        if !ctx.cur_open() {
            early_exit = true;
            break;
        }
        prev = Some(s);
    }
    let frame = ctx.scope_stack.pop().expect("scope frame");
    let shadows = ctx.shadow_stack.pop().expect("shadow frame");
    if !early_exit {
        for name in &frame {
            let info = match ctx.locals.get(name) {
                Some(i) => *i,
                None => continue,
            };
            if info.ty.is_copy() {
                // RFC 20260705 chunk 550 fix-up — escape-promoted
                // Copy locals hold a capture-box stake (outer-stake
                // protocol); release it at scope close.
                if ctx.escape_captured_lets.contains(name) {
                    ctx.f.append_void(
                        ctx.cur_block,
                        InstKind::Call(
                            ctx.intrinsics.capture_box_drop,
                            vec![Operand::Value(info.slot)],
                        ),
                    );
                }
                continue;
            }
            // RFC 20260710 — a promoted mutable non-Copy capture
            // releases its box stake (content drops with the box on
            // the last release).
            if !info.borrowed && ctx.boxed_noncopy_lets.contains(name) {
                let fid = ctx.capture_box_drop_fid(info.ty);
                ctx.f.append_void(
                    ctx.cur_block,
                    InstKind::Call(fid, vec![Operand::Value(info.slot)]),
                );
                continue;
            }
            if info.moved || ctx.stack_alloced_locals.contains(name) {
                continue;
            }
            let val = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(info.ty, Operand::Value(info.slot), 0),
                info.ty,
                None,
            );
            ctx.emit_drop_value(Operand::Value(val), info.ty);
        }
    }
    for name in frame {
        ctx.boxed_noncopy_lets.remove(&name);
        ctx.locals.remove(&name);
    }
    for (name, prev) in shadows {
        if ctx.binding_is_boxed_noncopy(&name, &prev) {
            ctx.boxed_noncopy_lets.insert(name.clone());
        }
        ctx.locals.insert(name, prev);
    }
}
