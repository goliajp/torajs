//! Exit-time teardown of the top level, emitted only on request.
//!
//! A program leaves through `__torajs_process_exit`, which does not walk
//! the heap: once the last job checkpoint has run, nothing a program can
//! observe is left (ECMA-262 §9.13 does not promise that a
//! FinalizationRegistry callback ever runs), and the process takes its
//! memory with it. So by default the top level is not torn down at all:
//! no drops of the top-level locals, the globals or the class cell
//! registry, and no last cycle-collector drain.
//!
//! The leak and refcount instruments need that teardown, because a
//! top-level binding's share that was booked wrong only shows when it is
//! released. Compiling with `TORAJS_TEARDOWN=1` emits it as before:
//!
//! - the fall-through end of `main`: locals, globals, class cells, cycle
//!   drain, after the exit code was taken;
//! - the end of the module body that awaits at top level, and a throw
//!   leaving it: its locals;
//! - an uncaught throw in `main`: its locals.

use std::sync::OnceLock;

use crate::ssa::InstKind;
use crate::ssa_lower::LowerCtx;

/// Whether the compile was asked for the teardown. Only the literal `1`
/// counts — a mistyped switch reads as off (same protocol as
/// `TORAJS_CHECKED_CAST`). Read once per process: every exit site of a
/// program asks.
pub(crate) fn on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| matches!(std::env::var("TORAJS_TEARDOWN").as_deref(), Ok("1")))
}

/// The top-level frame's owned locals, at an exit of `main` or of the
/// module body.
pub(crate) fn emit_local_drops(ctx: &mut LowerCtx<'_>) {
    if on() {
        ctx.emit_drops_for_owned_locals();
    }
}

/// The whole teardown at the fall-through end of `main`: locals,
/// globals, the class cells the prologue handed to the registry, then
/// one last cycle-collector drain for the cycles that outlived them.
/// Runs after the exit code was taken, while the heap was whole.
pub(crate) fn emit_main_teardown(ctx: &mut LowerCtx<'_>) {
    if !on() {
        return;
    }
    ctx.emit_drops_for_owned_locals();
    ctx.emit_drops_for_globals();
    crate::ssa_lower_main_exit::emit_class_cell_registry_release(ctx);
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.cycle_at_exit_drain, vec![]),
    );
}
