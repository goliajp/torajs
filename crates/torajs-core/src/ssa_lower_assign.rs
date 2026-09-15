//! `Expr::Assign` lowering — the target-shape dispatch moved out of
//! [`crate::ssa_lower_expr_inner`] (rotation 655, file-size: that
//! dispatch table sits at its cap), plus the per-expression operand
//! pins the assignment forms share with their embedded reads.
//!
//! RFC 20260915-compound-assign-single-eval. The parser desugars
//! `t op= v` to `t = (t' op v)` and `t ??= v` to `t' ?? (t = v)`
//! (`||=` / `&&=` alike), where `t'` is a fresh top-level node whose
//! `obj` / `index` children are the SAME ExprIds as `t`'s
//! (`clone_expr_for_compound`). Lowering used to `lower_expr` those
//! children once per site, so `a[i++] += 5` stepped `i` twice,
//! `f()[1] *= 2` called `f` twice and `o[key()] ??= v` called `key`
//! twice. §13.15.2 evaluates the LeftHandSideExpression's Reference
//! once: the three assignment entry points pin the shared children's
//! operands in [`SharedPins::operands`] before either site runs, and
//! `lower_expr` hands a pinned operand back instead of re-emitting.
//!
//! Ownership while pinned is the receiver's, not the sites': every
//! ownership probe answers "borrow" for a pinned eid (so the read
//! does not release the receiver temp and the store does not hand it
//! to a consuming kernel), and the entry point that pinned it
//! releases what was owned once the whole expression is lowered —
//! the same single-owner contract `pin_logical_assign_key` /
//! `unpin_logical_assign_key` use for the coerced key.

use crate::ast::{BinOp, Expr, ExprId};
use crate::ssa::Operand;
use crate::ssa_lower::LowerCtx;

/// Operands one assignment expression parks for reuse by a second
/// site inside the same expression (see the field docs; every entry
/// is set and cleared by the lowering that owns the expression).
#[derive(Default)]
pub(crate) struct SharedPins {
    /// Cluster #4 T4 — compound `o[k] op= v` coerced-key pin: the keyed
    /// write lane runs ToPropertyKey once and parks (key eid, coerced key)
    /// here so the embedded read reuses it (§6.2.5 GetValue writes the
    /// coerced key back into the Reference Record — one coerce total).
    pub(crate) compound_key: Option<(ExprId, Operand)>,
    /// The lowered `obj` / `index` children an assignment form shares
    /// between its target and its embedded read (module doc). Consulted
    /// by `lower_expr` (hit = hand it back) and by the ownership probes
    /// (hit = borrow). Distinct from `compound_key`, which holds the
    /// key AFTER ToPropertyKey; this holds it as lowered.
    pub(crate) operands: std::collections::HashMap<ExprId, Operand>,
}

/// What one entry point pinned: each shared child's eid, its lowered
/// operand and whether the operand was an owned temp before the pin
/// hid that (the release the entry point owes at [`unpin`]).
pub(crate) struct SharedTargetPin {
    entries: Vec<(ExprId, Operand, bool)>,
}

/// The shared children of a target and its clone: both `Member` on one
/// `obj`, or both `Index` on one `obj` and one `index`. Anything else
/// (an Ident target, or two independently spelled reads) shares nothing.
fn shared_children(
    ctx: &LowerCtx<'_>,
    read: ExprId,
    target: ExprId,
) -> Option<(ExprId, Option<ExprId>)> {
    match (ctx.ast.get_expr(read), ctx.ast.get_expr(target)) {
        (Expr::Member { obj: o1, .. }, Expr::Member { obj: o2, .. }) if o1 == o2 => {
            Some((*o1, None))
        }
        (Expr::Index { obj: o1, index: i1 }, Expr::Index { obj: o2, index: i2 })
            if o1 == o2 && i1 == i2 =>
        {
            Some((*o1, Some(*i1)))
        }
        _ => None,
    }
}

/// The compound fingerprint: `target = (read op v)` with `read` the
/// parser's clone of `target`. The logical ops never arrive here — the
/// parser desugars them the other way round ([`pin_logical_target`]).
pub(crate) fn pin_compound_target(
    ctx: &mut LowerCtx<'_>,
    target: ExprId,
    value: ExprId,
) -> Option<SharedTargetPin> {
    let Expr::BinOp { op, left, .. } = ctx.ast.get_expr(value) else {
        return None;
    };
    if matches!(op, BinOp::LAnd | BinOp::LOr) {
        return None;
    }
    let (obj, index) = shared_children(ctx, *left, target)?;
    Some(pin(ctx, obj, index))
}

/// The logical fingerprint: `read ?? (target = v)` / `read || (…)` /
/// `read && (…)` with `read` the parser's clone of `target`.
pub(crate) fn pin_logical_target(
    ctx: &mut LowerCtx<'_>,
    read: ExprId,
    assign: ExprId,
) -> Option<SharedTargetPin> {
    let Expr::Assign { target, .. } = ctx.ast.get_expr(assign) else {
        return None;
    };
    let (obj, index) = shared_children(ctx, read, *target)?;
    Some(pin(ctx, obj, index))
}

/// Lower the shared children once, receiver before key (§13.15.2
/// step 1 evaluates the whole LeftHandSideExpression first), and
/// record what each one owned BEFORE the pin makes the probes answer
/// borrow. A literal key is left alone: it is a pure value, and the
/// literal-key lanes never lower it at all.
fn pin(ctx: &mut LowerCtx<'_>, obj: ExprId, index: Option<ExprId>) -> SharedTargetPin {
    let mut entries = Vec::with_capacity(2);
    let mut one = |ctx: &mut LowerCtx<'_>, eid: ExprId| {
        let op = ctx.lower_expr(eid);
        let owned = ctx.expr_transfers_ownership(eid) || ctx.expr_minted_closure(eid, &op);
        ctx.pins.operands.insert(eid, op.clone());
        entries.push((eid, op, owned));
    };
    one(ctx, obj);
    if let Some(index) = index
        && !matches!(
            ctx.ast.get_expr(index),
            Expr::String(_) | Expr::Number(_) | Expr::Bool(_) | Expr::Null
        )
    {
        one(ctx, index);
    }
    SharedTargetPin { entries }
}

/// Clear the pins and settle what they hid. `result` is the whole
/// expression's value: a logical form's short-circuit path answers
/// the guard READ, which under the pin is a borrow of the receiver's
/// storage — when that receiver is an owned temp about to be released
/// here, the value is detached first (an owned inc, recorded on the
/// join's eid), the `ssa_lower_member::lower` owned-receiver
/// contract. A compound form's value is its BinOp result, minted
/// owned by every store lane, so it needs nothing.
pub(crate) fn unpin(
    ctx: &mut LowerCtx<'_>,
    pin: SharedTargetPin,
    result_eid: ExprId,
    result: &Operand,
) {
    let receiver_owned = pin.entries.first().is_some_and(|(_, _, owned)| *owned);
    for (eid, _, _) in &pin.entries {
        ctx.pins.operands.remove(eid);
    }
    if receiver_owned {
        let ty = ctx.operand_ty(result);
        if ty.is_refcounted() && !ctx.expr_is_fresh_owned(result_eid) {
            ctx.emit_owned_result_inc(result.clone(), ty);
            ctx.owned_member_reads.insert(result_eid);
        }
    }
    for (_, op, owned) in pin.entries {
        if !owned {
            continue;
        }
        let ty = ctx.operand_ty(&op);
        if !ty.is_copy() {
            ctx.emit_drop_value(op, ty);
        }
    }
}

/// `Expr::Assign` target dispatch — Ident / Member / Index shapes
/// route to their per-shape sibling lowerers.
pub(crate) fn lower(ctx: &mut LowerCtx, eid: ExprId, target: ExprId, value: ExprId) -> Operand {
    let pin = pin_compound_target(ctx, target, value);
    let out = lower_by_target(ctx, eid, target, value);
    if let Some(pin) = pin {
        unpin(ctx, pin, eid, &out);
    }
    out
}

fn lower_by_target(ctx: &mut LowerCtx, eid: ExprId, target: ExprId, value: ExprId) -> Operand {
    match ctx.ast.get_expr(target).clone() {
        Expr::Ident(name) => {
            // K.3 module-level data global + local-binding
            // assign (4-layer coercion: F64←I64 / Any←val
            // box_to_any / num←Any coerce / Str←Any
            // coerce_to_str), plus the RFC 20260730 undeclared-
            // write ReferenceError lane keyed by target eid. See
            // [`crate::ssa_lower_assign_ident::lower`].
            crate::ssa_lower_assign_ident::lower(ctx, eid, target, name, value)
        }
        Expr::Member { obj, name: field } => {
            // M1.4 — `obj.field = value`. 7-way dispatch
            // (Type::Any dynobj / Closure props / FnSig
            // fnprops / Arr length setter / Arr arrprops /
            // RegExp lastIndex / struct field store with
            // setter accessor + frozen guard). See
            // [`crate::ssa_lower_assign_member::lower`].
            crate::ssa_lower_assign_member::lower(ctx, eid, obj, field, value)
        }
        Expr::Index { obj, index } => {
            // bug-327 C3 — moved to ssa_lower_index_assign.rs
            // (bounds-honoring write: Array<Any> grows via
            // __torajs_arr_set_any_grow + write-back, typed
            // tier guards the inline store).
            ctx.lower_index_assign(eid, obj, index, value)
        }
        other => panic!("ssa-lower: unsupported assign target: {other:?}"),
    }
}
