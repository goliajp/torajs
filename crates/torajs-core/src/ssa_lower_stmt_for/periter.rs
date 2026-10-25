//! The per-iteration copies of a `for (let …)` head's bindings
//! (§14.7.4.4 CreatePerIterationEnvironment) for a binding a closure
//! captures and something writes.
//!
//! The copy the first test runs in is made after the initializer, and
//! each later one at the end of an iteration, before the update: the
//! test, the body and the update that follows the copy all see the same
//! box, which is what a closure made in any of them captures. A closure
//! the initializer makes sees the head's own binding, which no copy
//! changes afterwards.
//!
//! The current box rides a `track` slot across the loop's edges: the
//! header reloads it, the update block replaces it, and the block the
//! loop leaves by (a failed test or a `break`) releases it. A Copy
//! binding no closure in the test or the update captures keeps the
//! cheaper shape the copies cannot be told from: the test and the
//! update run on the plain slot and only the body sees a box.

mod init;

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type, ValueId};
use crate::ssa_lower::{LocalInfo, LowerCtx};
use crate::ssa_lower_closure_captures::captures_any;
pub(super) use init::{detach_init_boxes, per_iter_candidates};

pub(super) struct PerIterVar {
    name: String,
    /// The head's own binding, a plain slot the first copy is seeded
    /// from. A non-Copy value's reference moves into that copy, so a
    /// return, a throw or a labelled jump out of the loop owes only the
    /// box the name is bound to.
    outer: LocalInfo,
    track: ValueId,
    /// No closure the test or the update makes captures the binding,
    /// so nothing tells which binding they run on: they run on the
    /// plain slot, and the iteration's box is seeded at the body's
    /// start and copied back at its end. Copy values only: a non-Copy
    /// copy-back costs a reference count pair per iteration, more than
    /// the box reads it saves.
    body_copy: bool,
}

/// The tracking state for each candidate something writes: a binding
/// never written keeps the env snapshot, which no closure can tell
/// from a copy. Any / Substr stay recorded boundaries.
pub(super) fn setup_per_iter_vars(
    ctx: &mut LowerCtx,
    candidates: Vec<String>,
    cond: Option<ExprId>,
    step: Option<ExprId>,
) -> Vec<PerIterVar> {
    let mut per_iter = Vec::new();
    for name in candidates {
        let Some(outer) = ctx.locals.get(&name).copied() else {
            continue;
        };
        // a closure dead-zone flag only ever turns true, before the
        // first test: every iteration may share its one box
        let tdz_flag = name.starts_with("__tdzc");
        if tdz_flag || !(ctx.mutated_captured_lets.contains(&name) && outer.ty != Type::Substr) {
            continue;
        }
        let in_head = cond
            .into_iter()
            .chain(step)
            .any(|e| captures_any(ctx.ast, e, &[name.as_str()]));
        let body_copy = outer.ty.is_copy() && !in_head;
        let track = ctx.alloca(Type::Ptr, Some("__periter_box"));
        per_iter.push(PerIterVar {
            name,
            outer,
            track,
            body_copy,
        });
    }
    per_iter
}

/// Point the name at a box: the body, the test, the update and every
/// closure they make resolve it there.
fn bind_box(ctx: &mut LowerCtx, pv: &PerIterVar, boxed: ValueId) {
    if !pv.outer.ty.is_copy() {
        // closures capture a non-Copy box byref (write_captures /
        // assign-ident boxed lanes)
        ctx.boxed_noncopy_lets.insert(pv.name.clone());
    }
    ctx.locals.insert(
        pv.name.clone(),
        LocalInfo {
            slot: boxed,
            ty: pv.outer.ty,
            moved: true,
            borrowed: false,
            boxed: true,
            scope_depth: pv.outer.scope_depth,
        },
    );
}

/// A fresh box holding `v`, recorded in the track slot. A non-Copy
/// value takes a stake of its own for the box unless `moved` hands it
/// the one its slot held.
fn mint_box(ctx: &mut LowerCtx, pv: &PerIterVar, v: ValueId, moved: bool) -> ValueId {
    if !moved && !pv.outer.ty.is_copy() {
        ctx.emit_owned_result_inc(Operand::Value(v), pv.outer.ty);
    }
    let boxed = ctx.emit_capture_boxed(pv.outer.ty, Operand::Value(v));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(boxed), Operand::Value(pv.track), 0),
    );
    if let Some(ann) = ctx.let_anns.get(&pv.outer.slot).cloned() {
        ctx.let_anns.insert(boxed, ann);
    }
    boxed
}

/// The first copy, before the first test, seeded from the head's
/// binding, which is not read again: its reference moves into the box
/// (the fast and the slow loop are exclusive, so it moves once).
/// A `body_copy` binding has no box yet: its track slot starts empty.
pub(super) fn mint_first_boxes(ctx: &mut LowerCtx, per_iter: &[PerIterVar]) {
    for pv in per_iter {
        if pv.body_copy {
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Store(Operand::ConstPtrNull, Operand::Value(pv.track), 0),
            );
            continue;
        }
        let cur = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(pv.outer.ty, Operand::Value(pv.outer.slot), 0),
            pv.outer.ty,
            None,
        );
        let boxed = mint_box(ctx, pv, cur, true);
        if !pv.outer.ty.is_copy() {
            // the slot owes nothing now, which a binding slot says with null
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Store(Operand::ConstPtrNull, Operand::Value(pv.outer.slot), 0),
            );
        }
        bind_box(ctx, pv, boxed);
    }
}

/// The start of the body: a `body_copy` binding's box for this
/// iteration, seeded from the plain slot.
pub(super) fn mint_body_boxes(ctx: &mut LowerCtx, per_iter: &[PerIterVar]) {
    for pv in per_iter.iter().filter(|pv| pv.body_copy) {
        let cur = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(pv.outer.ty, Operand::Value(pv.outer.slot), 0),
            pv.outer.ty,
            None,
        );
        let boxed = mint_box(ctx, pv, cur, false);
        bind_box(ctx, pv, boxed);
    }
}

/// At the loop header, which the first copy and every update reach:
/// the current box is the one the track slot holds.
pub(super) fn reload_boxes(ctx: &mut LowerCtx, per_iter: &[PerIterVar]) {
    for pv in per_iter.iter().filter(|pv| !pv.body_copy) {
        let boxed = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(Type::Ptr, Operand::Value(pv.track), 0),
            Type::Ptr,
            None,
        );
        if let Some(ann) = ctx.let_anns.get(&pv.outer.slot).cloned() {
            ctx.let_anns.insert(boxed, ann);
        }
        bind_box(ctx, pv, boxed);
    }
}

/// The end of an iteration (its fallthrough and `continue` both arrive
/// here), before the update: the next iteration's copy takes the
/// current value, and the iteration's own box loses this frame's
/// stake (a closure that captured it holds its own). A `body_copy`
/// binding's value goes back to the plain slot instead, which the
/// update and the next test run on.
pub(super) fn renew_boxes(ctx: &mut LowerCtx, per_iter: &[PerIterVar]) {
    for pv in per_iter {
        let old = ctx.locals.get(&pv.name).expect("per-iter box bound").slot;
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(pv.outer.ty, Operand::Value(old), 0),
            pv.outer.ty,
            None,
        );
        let boxed = if pv.body_copy {
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Store(Operand::Value(v), Operand::Value(pv.outer.slot), 0),
            );
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Store(Operand::ConstPtrNull, Operand::Value(pv.track), 0),
            );
            None
        } else {
            Some(mint_box(ctx, pv, v, false))
        };
        let drop_fid = ctx.capture_box_drop_fid(pv.outer.ty);
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Call(drop_fid, vec![Operand::Value(old)]),
        );
        match boxed {
            Some(b) => bind_box(ctx, pv, b),
            None => {
                ctx.locals.insert(pv.name.clone(), pv.outer);
            }
        }
    }
}

/// The block the loop leaves by: release the box of the iteration the
/// test failed in or a `break` left (a `body_copy` binding's track slot
/// is empty after a failed test), and point the name back at the
/// head's binding, which holds nothing the for frame owes.
pub(super) fn release_boxes(ctx: &mut LowerCtx, per_iter: &[PerIterVar]) {
    for pv in per_iter {
        let t = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(Type::Ptr, Operand::Value(pv.track), 0),
            Type::Ptr,
            None,
        );
        let drop_fid = ctx.capture_box_drop_fid(pv.outer.ty);
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Call(drop_fid, vec![Operand::Value(t)]),
        );
        ctx.boxed_noncopy_lets.remove(&pv.name);
        let outer = LocalInfo {
            moved: true,
            ..pv.outer
        };
        ctx.locals.insert(pv.name.clone(), outer);
    }
}
