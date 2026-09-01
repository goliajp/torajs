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

use crate::ast::Stmt;
use crate::ssa::{BlockId, InstKind, Operand, Terminator, ValueId};
use crate::ssa_lower::LowerCtx;

/// Target of a labeled statement, kept on `LowerCtx::label_stack`
/// while lowering the statement's body (ES §13.13).
#[derive(Clone, Copy)]
pub(crate) enum LabelTarget {
    /// A labeled iteration statement or `switch`. Its
    /// `(continue_target, break_target)` live in `loop_stack[idx]` for
    /// the duration of body lowering — the label references the slot
    /// rather than the blocks so the loop lowering stays oblivious to
    /// labels. `break label` → `loop_stack[idx].1`; `continue label`
    /// → `loop_stack[idx].0`.
    Loop(usize),
    /// A labeled non-iteration statement (block / if / etc.).
    /// `break label` branches to this after-block; `continue label`
    /// is a syntax error for such a target.
    Block(BlockId),
}

/// One entry of `LowerCtx::label_stack`.
#[derive(Clone, Copy)]
pub(crate) struct LabelFrame {
    pub(crate) target: LabelTarget,
    /// `try_finally_stack.len()` captured when this label was pushed.
    /// A `break`/`continue` whose current finally depth exceeds this
    /// crossed a `try...finally` opened *inside* the labeled statement,
    /// so branching straight to the target would skip that finally —
    /// the jump routes through the innermost finally instead, via a
    /// lazily-allocated pending flag below.
    pub(crate) finally_depth_at_push: usize,
    /// `scope_stack.len()` when the label was pushed — the first frame
    /// a `break label` leaves behind (RFC 20260901-scope-exit-drops).
    /// A loop-like target reads the loop's own depth instead.
    pub(crate) scope_depth: usize,
    /// Pending-jump flags `[continue, break]` for jumps to this label
    /// that cross an intervening try-finally: the jump site sets the
    /// flag and branches to the innermost finally; each finally tail
    /// dispatches it onward (to the next outer finally while depth
    /// still exceeds `finally_depth_at_push`, else — flag cleared —
    /// to the label's target). Allocated on first such jump.
    pub(crate) pending_flags: [Option<ValueId>; 2],
}

/// `Stmt::Labeled { label, body }` — ES §13.13. The label is registered
/// on `label_stack` for the duration of body lowering so a `break label`
/// / `continue label` inside can resolve its target.
///
/// A labeled *iteration statement* (or `switch`) registers its label as
/// [`LabelTarget::Loop`] pointing at the `loop_stack` slot the loop
/// lowering is about to fill — so the ~10 loop lowerings stay oblivious
/// to labels. Any other labeled statement registers a
/// [`LabelTarget::Block`] whose after-block a `break label` exits to.
pub(crate) fn lower_labeled(ctx: &mut LowerCtx, label: &str, body: &Stmt) {
    let finally_depth = ctx.try_finally_stack.len();
    let scope_depth = ctx.scope_stack.len();
    if stmt_is_loop_like(body) {
        // The loop lowering pushes onto loop_stack next; reference the
        // slot it will occupy during body lowering.
        let idx = ctx.loop_stack.len();
        ctx.label_stack.push((
            label.to_string(),
            LabelFrame {
                target: LabelTarget::Loop(idx),
                finally_depth_at_push: finally_depth,
                scope_depth,
                pending_flags: [None, None],
            },
        ));
        ctx.lower_stmt(body);
        ctx.label_stack.pop();
    } else {
        let after = ctx.f.add_block();
        ctx.label_stack.push((
            label.to_string(),
            LabelFrame {
                target: LabelTarget::Block(after),
                finally_depth_at_push: finally_depth,
                scope_depth,
                pending_flags: [None, None],
            },
        ));
        ctx.lower_stmt(body);
        ctx.label_stack.pop();
        // Fall through past the labeled statement into `after`.
        if ctx.cur_open() {
            let cb = ctx.cur_block;
            ctx.f.set_term(cb, Terminator::Br(after));
        }
        ctx.cur_block = after;
    }
}

/// `break label` / `continue label` target through nested `Labeled`
/// wrappers is the ultimate iteration statement, so a label on a loop
/// still counts as loop-like even when stacked (`a: b: for …`).
fn stmt_is_loop_like(s: &Stmt) -> bool {
    match s {
        Stmt::For { .. }
        | Stmt::While { .. }
        | Stmt::DoWhile { .. }
        | Stmt::ForOf { .. }
        | Stmt::ForOfSplitIter { .. }
        | Stmt::Switch { .. } => true,
        Stmt::Labeled { body, .. } => stmt_is_loop_like(body),
        _ => false,
    }
}

/// Resolve a `break`/`continue` label to its stack index, panicking
/// on an unknown label (a valid program never reaches this — the
/// same loud convention as `break` outside any loop).
fn resolve_label(ctx: &LowerCtx, kw: &str, label: &str) -> usize {
    ctx.label_stack
        .iter()
        .rposition(|(name, _)| name == label)
        .unwrap_or_else(|| panic!("ssa-lower: `{kw} {label}` to unknown label"))
}

/// A labeled jump that crossed a `try...finally` opened inside the
/// labeled statement can't branch straight to its target — that
/// would skip the finally. Instead it sets the frame's per-kind
/// pending flag (lazily allocated) and branches to the innermost
/// finally; each finally tail dispatches the flag onward (see
/// `ssa_lower_stmt_try_finally::emit_pending_label_flags`). Returns
/// true when it emitted the routed form.
fn route_labeled_through_finally(ctx: &mut LowerCtx, frame_idx: usize, want_break: bool) -> bool {
    let frame = ctx.label_stack[frame_idx].1;
    if ctx.try_finally_stack.len() <= frame.finally_depth_at_push {
        return false;
    }
    let slot = want_break as usize;
    let flag = match frame.pending_flags[slot] {
        Some(f) => f,
        None => {
            let kw = if want_break { "brk" } else { "cont" };
            let name = format!("__pending_{kw}_{}", ctx.label_stack[frame_idx].0);
            let f = ctx.alloca_bool_flag_in_entry(Some(&name));
            ctx.label_stack[frame_idx].1.pending_flags[slot] = Some(f);
            f
        }
    };
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::ConstBool(true), Operand::Value(flag), 0),
    );
    let fb = *ctx.try_finally_stack.last().expect("depth checked above");
    // RFC 20260901-scope-exit-drops — the frames inside the try body
    // die now; the finally tail releases the rest when it dispatches
    // the pending jump onward.
    ctx.emit_drops_for_scopes_from(fb.scope_depth);
    let cb = ctx.cur_block;
    ctx.f.set_term(cb, Terminator::Br(fb.blk));
    true
}

/// Resolve a labeled jump's direct target and the scope depth it
/// leaves (RFC 20260901-scope-exit-drops): a loop-like label defers
/// to the loop's own entry, a plain labeled block to the depth the
/// label was pushed at.
fn labeled_target(ctx: &LowerCtx, idx: usize, want_break: bool) -> (BlockId, usize) {
    match ctx.label_stack[idx].1.target {
        LabelTarget::Loop(li) => {
            let lt = ctx.loop_stack[li];
            (if want_break { lt.brk } else { lt.cont }, lt.scope_depth)
        }
        LabelTarget::Block(blk) => (blk, ctx.label_stack[idx].1.scope_depth),
    }
}

pub(crate) fn lower_break(ctx: &mut LowerCtx, label: Option<&str>) {
    if let Some(label) = label {
        let idx = resolve_label(ctx, "break", label);
        if route_labeled_through_finally(ctx, idx, true) {
            return;
        }
        let (after, depth) = labeled_target(ctx, idx, true);
        ctx.emit_drops_for_scopes_from(depth);
        let cb = ctx.cur_block;
        ctx.f.set_term(cb, Terminator::Br(after));
        return;
    }
    let lt = *ctx
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
        // RFC 20260901-scope-exit-drops — frames inside the try body
        // die here; the finally tail releases the ones between the
        // try and the loop.
        ctx.emit_drops_for_scopes_from(fb.scope_depth);
        let cb = ctx.cur_block;
        ctx.f.set_term(cb, Terminator::Br(fb.blk));
    } else {
        // RFC 20260901-scope-exit-drops — a direct `break` leaves
        // every frame from the loop body up, skipping their closes.
        ctx.emit_drops_for_scopes_from(lt.scope_depth);
        ctx.f.set_term(ctx.cur_block, Terminator::Br(lt.brk));
    }
}

pub(crate) fn lower_continue(ctx: &mut LowerCtx, label: Option<&str>) {
    if let Some(label) = label {
        let idx = resolve_label(ctx, "continue", label);
        if route_labeled_through_finally(ctx, idx, false) {
            return;
        }
        // `continue` requires an iteration statement (ES §14.8); a
        // labeled non-loop target is a syntax error upstream.
        if matches!(ctx.label_stack[idx].1.target, LabelTarget::Block(_)) {
            panic!("ssa-lower: `continue {label}` targets a non-loop label")
        }
        let (cont, depth) = labeled_target(ctx, idx, false);
        ctx.emit_drops_for_scopes_from(depth);
        let cb = ctx.cur_block;
        ctx.f.set_term(cb, Terminator::Br(cont));
        return;
    }
    let lt = *ctx
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
        ctx.emit_drops_for_scopes_from(fb.scope_depth);
        let cb = ctx.cur_block;
        ctx.f.set_term(cb, Terminator::Br(fb.blk));
        return;
    }
    ctx.emit_drops_for_scopes_from(lt.scope_depth);
    ctx.f.set_term(ctx.cur_block, Terminator::Br(lt.cont));
}
