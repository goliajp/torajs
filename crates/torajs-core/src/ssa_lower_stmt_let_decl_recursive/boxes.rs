//! Opening a binding's capture box ahead of the closure that captures
//! it.

use std::collections::HashMap;

use crate::ssa::{Operand, Type};
use crate::ssa_lower::{LocalInfo, LowerCtx};

/// The ordinary bindings whose box is up ahead of their declaration,
/// keyed by name and the scope depth the box is in, each with the
/// `(lifted closure fn, capture index)` of every env that took it. The
/// depth tells this binding from a nested block's own binding of the
/// name, which shadows it inside that block and never fills its box.
pub(crate) type ForwardBoxes = HashMap<(String, usize), Vec<(String, usize)>>;

/// The key of a box waiting for a declaration of `name` at the current
/// depth.
pub(crate) fn here(ctx: &LowerCtx, name: &str) -> (String, usize) {
    (name.to_string(), ctx.scope_stack.len() - 1)
}

/// Note that a closure's env took the box `name` resolves to, when that
/// is a box still waiting for its declaration.
pub(crate) fn note_capture_site(ctx: &mut LowerCtx, name: &str, site: (String, usize)) {
    let key = (name.to_string(), ctx.locals[name].scope_depth);
    if let Some(sites) = ctx.forward_capture_boxes.get_mut(&key) {
        sites.push(site);
    }
}

/// Give a binding a capture box holding null, register it, and answer
/// the box's value-slot pointer. Shared by both entry shapes so the
/// ownership bookkeeping is written once.
pub(super) fn open_box(ctx: &mut LowerCtx, name: &str, ty: Type) -> crate::ssa::ValueId {
    let cur_depth = ctx.scope_stack.len() - 1;
    let slot = ctx.emit_capture_boxed(ty, Operand::ConstPtrNull);
    ctx.boxed_noncopy_lets.insert(name.to_string());
    if let Some(prev) = ctx.locals.get(name).copied()
        && prev.scope_depth < cur_depth
    {
        let top_shadow = ctx.shadow_stack.last_mut().expect("shadow frame");
        top_shadow.push((name.to_string(), prev));
    }
    ctx.locals.insert(
        name.to_string(),
        LocalInfo {
            slot,
            ty,
            // The box owns the closure's stake and this frame owns the
            // box, so the scope-close walk has to reach it (the
            // `boxed_noncopy_lets` arm of the drop walk releases it).
            moved: true,
            borrowed: false,
            boxed: true,
            scope_depth: cur_depth,
        },
    );
    ctx.scope_stack
        .last_mut()
        .expect("scope frame")
        .push(name.to_string());
    slot
}

/// Open `name`'s capture box HERE, in the ordinary-binding shape
/// [`hoist_forward_boxes`] gives a forward-referenced one: a
/// provisional `Any` box the declaration patches when it runs.
///
/// The switch lowerer needs the same thing for a different reason.
/// §14.12.4 gives the whole CaseBlock one declarative environment, so
/// a closure in one clause may capture a binding another clause
/// declares — and the clause bodies are sibling basic blocks. A box
/// minted where the declaration sits is defined in a block that does
/// not dominate the capture, and a constant scrutinee
/// (`switch (1) { case 0: let x; case 1: (function () { x; })(); }`)
/// deletes that block outright, leaving the capture reading a value
/// with no definition ("ValueId not allocated" at regalloc). Minting
/// it before the compare chain puts it where every clause can see it.
pub(crate) fn open_case_block_box(ctx: &mut LowerCtx, name: &str) {
    if ctx.is_main_fn && ctx.globals.contains_key(name) {
        return;
    }
    let key = here(ctx, name);
    if ctx.hoisted_closure_lets.contains(&key) || ctx.forward_capture_boxes.contains_key(&key) {
        return;
    }
    open_box(ctx, name, Type::Any);
    ctx.forward_capture_boxes.insert(key, Vec::new());
}
