//! The sub-sibling prologue of [`crate::ssa_lower_stmt_let_decl::lower`]
//! — the ordered try-chain each specialized `let` lane gets its shot at
//! before the general path runs.
//!
//! Moved out of the parent rather than extracted inside it: the parent
//! sat at 498 prod LOC and its `lower` at 198, two lines under each
//! limit, so an in-file helper's signature and doc would have carried
//! the FILE over 500 while relieving the function (the rotation-147
//! lesson, in its exact form). A sibling relieves both.
//!
//! Order is load-bearing. The narrower a lane's shape, the earlier it
//! sits, so a lane never claims a declaration a more specific one
//! wanted; and the two global lanes come before the closure lane
//! because a top-level binding is promoted before it is ever a local.

use crate::ast::ExprId;
use crate::ssa_lower::LowerCtx;

/// Run the specialized lanes in order. `true` means one of them claimed
/// the declaration and fully lowered it — the caller returns without
/// touching the general path.
pub(crate) fn try_sub_siblings(
    ctx: &mut LowerCtx,
    name: &str,
    type_ann: Option<&String>,
    init: ExprId,
    mutable: bool,
) -> bool {
    // A capture box for this name is already up and already inside some
    // env (a closure earlier in this list captured it). The general
    // path is the one that knows to fill that box, so no specialized
    // lane may claim the declaration and put the value in a slot of its
    // own choosing — the closure would read the box forever.
    if ctx.forward_capture_boxes.contains_key(name) {
        return false;
    }
    // T-19.d — `let X: T = await Bun.file(p).json()`.
    if crate::ssa_lower_stmt_let_decl_bun_json::try_lower(ctx, name, type_ann, init) {
        return true;
    }
    // T-02 — `let v: T = JSON.parse(text)`.
    if crate::ssa_lower_stmt_let_decl_json_parse::try_lower(ctx, name, type_ann, init) {
        return true;
    }
    // T-09.c — `let o: Pair = Object.fromEntries(es)`.
    if crate::ssa_lower_stmt_let_decl_fromentries::try_lower(ctx, name, type_ann, init) {
        return true;
    }
    // K.3 / K.4 — top-level data global.
    if crate::ssa_lower_stmt_let_decl_global::try_lower_global_let(ctx, name, type_ann, init) {
        return true;
    }
    // M2 Phase B Stage 4 — `let f = global_fn`.
    if crate::ssa_lower_stmt_let_decl_global::try_lower_fn_addr_let(ctx, name, init) {
        return true;
    }
    // RFC 20260714-dstr-residual blade 3 — an array binding pattern
    // whose source has to be STEPPED, not indexed (a generator, a
    // Map / Set, a class iterable, anything behind `any`).
    if crate::ssa_lower_dstr_iter::try_lower_group(ctx, name, init) {
        return true;
    }
    // A closure needing its binding to exist before the mint that fills
    // it — self-referential, or one half of a mutually recursive pair.
    if crate::ssa_lower_stmt_let_decl_recursive::try_lower(ctx, name, type_ann, init, mutable) {
        return true;
    }
    false
}
