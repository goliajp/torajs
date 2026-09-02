//! `JSON.stringify`'s use of the redefined-member gate — RFC
//! 20260806-declared-field-redefine. The gate itself lives in
//! [`crate::ssa_lower_struct_exotic_gate`], shared with the
//! `Object.values` / `Object.entries` unfolds; §25.5.2
//! SerializeJSONObject walks EnumerableOwnProperties for the same
//! reason they do.
//!
//! What is local to this surface is WHERE the gate stands: behind
//! §25.5.2's null check, and what the general path is (the any-lane
//! serializer, byte-identical for every shape both tiers express).

use crate::ast::PropKey;
use crate::check;
use crate::ssa::{Operand, StructId, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_struct_exotic_gate::with_exotic_field_gate;

/// Wrap [`super::composite_obj::lower_obj`] in the exotic-field gate.
///
/// The header read sits INSIDE the §25.5.2 null gate. A `Nullable<Obj>`
/// slot holding JS null reaches here as a NULL pointer, and reading a
/// flags word off it is a segfault — which is the same mistake that
/// gate was written to fix, so the fix is to stand behind it rather
/// than to re-derive it. `lower_obj` keeps its own null gate; on this
/// path it is a branch that never taken, which costs one compare.
#[allow(clippy::too_many_arguments)]
pub(super) fn lower_obj_gated(
    ctx: &mut LowerCtx,
    val_op: Operand,
    sid: StructId,
    fe_fields: Option<Vec<(PropKey, check::Type)>>,
    gap: Option<Operand>,
    depth: u32,
    is_error: bool,
) -> Operand {
    let gated = val_op.clone();
    super::composite::with_null_gate(ctx, &gated, "__json_obj_exotic", move |ctx| {
        lower_obj_nonnull(ctx, val_op, sid, fe_fields, gap, depth, is_error)
    })
}

/// The gate body — reached only on a non-NULL cell.
#[allow(clippy::too_many_arguments)]
fn lower_obj_nonnull(
    ctx: &mut LowerCtx,
    val_op: Operand,
    sid: StructId,
    fe_fields: Option<Vec<(PropKey, check::Type)>>,
    gap: Option<Operand>,
    depth: u32,
    is_error: bool,
) -> Operand {
    let walk_val = val_op.clone();
    let walk_gap = gap.clone();
    with_exotic_field_gate(
        ctx,
        &val_op.clone(),
        Type::Str,
        move |ctx| {
            let boxed = ctx.box_to_any(walk_val);
            let walked = super::emit_any_walk(ctx, boxed, walk_gap, depth);
            ctx.emit_throw_check(None);
            walked
        },
        move |ctx| {
            super::composite_obj::lower_obj(ctx, val_op, sid, fe_fields, gap, depth, is_error)
        },
    )
}
