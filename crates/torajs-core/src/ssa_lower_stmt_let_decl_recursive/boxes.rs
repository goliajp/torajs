//! Opening a binding's capture box ahead of the closure that captures
//! it.

use crate::ssa::{Operand, Type};
use crate::ssa_lower::{LocalInfo, LowerCtx};

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
    if ctx.hoisted_closure_lets.contains(name) || ctx.forward_capture_boxes.contains_key(name) {
        return;
    }
    open_box(ctx, name, Type::Any);
    ctx.forward_capture_boxes
        .insert(name.to_string(), Vec::new());
}
