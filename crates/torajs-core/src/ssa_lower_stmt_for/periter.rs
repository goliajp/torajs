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
//! loop leaves by (a failed test or a `break`) releases it.

use crate::ast::{ExprId, Stmt};
use crate::ssa::{InstKind, Operand, Type, ValueId};
use crate::ssa_lower::{LocalInfo, LowerCtx};
use crate::ssa_lower_closure_captures::captures_any;

pub(super) struct PerIterVar {
    name: String,
    /// The head's own binding, a plain slot the first copy is seeded
    /// from; it keeps its value (and its stake) until the loop ends.
    outer: LocalInfo,
    track: ValueId,
}

/// The head names a closure in the loop captures, and those of them a
/// closure the initializer makes captures.
pub(super) fn per_iter_candidates(
    ctx: &LowerCtx,
    init: Option<&Stmt>,
) -> (Vec<String>, Vec<String>) {
    fn decls<'a>(s: &'a Stmt, out: &mut Vec<(bool, &'a String, ExprId)>) {
        match s {
            Stmt::LetDecl {
                mutable,
                is_var,
                name,
                init,
                ..
            } => out.push((*mutable && !*is_var, name, *init)),
            Stmt::Multi(v) => v.iter().for_each(|s| decls(s, out)),
            _ => {}
        }
    }
    let mut head = Vec::new();
    init.into_iter().for_each(|s| decls(s, &mut head));
    let captured: Vec<String> = head
        .iter()
        .filter(|d| ctx.escape_captured_lets.contains(d.1))
        .map(|d| d.1.clone())
        .collect();
    let captured_by_init = |name: &str| head.iter().any(|e| captures_any(ctx.ast, e.2, &[name]));
    let in_init = head
        .iter()
        .filter(|d| d.0 && captured_by_init(d.1))
        .map(|d| d.1.clone())
        .collect();
    (captured, in_init)
}

/// After the initializer ran: a binding the initializer's closures
/// captured lives in their box, which stays theirs. The loop starts
/// from a plain slot holding its value.
pub(super) fn detach_init_boxes(ctx: &mut LowerCtx, in_init: &[String]) {
    for name in in_init {
        let Some(info) = ctx.locals.get(name).copied() else {
            continue;
        };
        if !info.boxed {
            continue;
        }
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(info.ty, Operand::Value(info.slot), 0),
            info.ty,
            None,
        );
        if !info.ty.is_copy() {
            ctx.emit_owned_result_inc(Operand::Value(v), info.ty);
        }
        let slot = ctx.binding_slot_alloca(info.ty, name);
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(Operand::Value(v), Operand::Value(slot), 0),
        );
        let drop_fid = ctx.capture_box_drop_fid(info.ty);
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Call(drop_fid, vec![Operand::Value(info.slot)]),
        );
        if let Some(ann) = ctx.let_anns.get(&info.slot).cloned() {
            ctx.let_anns.insert(slot, ann);
        }
        ctx.boxed_noncopy_lets.remove(name);
        ctx.locals.insert(
            name.clone(),
            LocalInfo {
                slot,
                ty: info.ty,
                moved: false,
                borrowed: false,
                boxed: false,
                scope_depth: info.scope_depth,
            },
        );
    }
}

/// The tracking state for each candidate something writes: a binding
/// never written keeps the env snapshot, which no closure can tell
/// from a copy. Any / Substr stay recorded boundaries.
pub(super) fn setup_per_iter_vars(ctx: &mut LowerCtx, candidates: Vec<String>) -> Vec<PerIterVar> {
    let mut per_iter = Vec::new();
    for name in candidates {
        let Some(outer) = ctx.locals.get(&name).copied() else {
            continue;
        };
        if !(ctx.mutated_captured_lets.contains(&name) && outer.ty != Type::Substr) {
            continue;
        }
        let track = ctx.alloca(Type::Ptr, Some("__periter_box"));
        per_iter.push(PerIterVar { name, outer, track });
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
/// value takes the box's own stake.
fn mint_box(ctx: &mut LowerCtx, pv: &PerIterVar, v: ValueId) -> ValueId {
    if !pv.outer.ty.is_copy() {
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
/// binding.
pub(super) fn mint_first_boxes(ctx: &mut LowerCtx, per_iter: &[PerIterVar]) {
    for pv in per_iter {
        let cur = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(pv.outer.ty, Operand::Value(pv.outer.slot), 0),
            pv.outer.ty,
            None,
        );
        let boxed = mint_box(ctx, pv, cur);
        bind_box(ctx, pv, boxed);
    }
}

/// At the loop header, which the first copy and every update reach:
/// the current box is the one the track slot holds.
pub(super) fn reload_boxes(ctx: &mut LowerCtx, per_iter: &[PerIterVar]) {
    for pv in per_iter {
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
/// stake (a closure that captured it holds its own).
pub(super) fn renew_boxes(ctx: &mut LowerCtx, per_iter: &[PerIterVar]) {
    for pv in per_iter {
        let old = ctx.locals.get(&pv.name).expect("per-iter box bound").slot;
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(pv.outer.ty, Operand::Value(old), 0),
            pv.outer.ty,
            None,
        );
        let boxed = mint_box(ctx, pv, v);
        let drop_fid = ctx.capture_box_drop_fid(pv.outer.ty);
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Call(drop_fid, vec![Operand::Value(old)]),
        );
        bind_box(ctx, pv, boxed);
    }
}

/// The block the loop leaves by: release the box of the iteration the
/// test failed in or a `break` left, and point the name back at the
/// head's binding, which the for frame then releases.
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
        ctx.locals.insert(pv.name.clone(), pv.outer);
    }
}
