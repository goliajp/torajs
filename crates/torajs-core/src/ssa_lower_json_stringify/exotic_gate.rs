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
//!
//! The general path is entered only on an instance whose header
//! carries `FLAG_OBJ_EXOTIC_FIELD` or `FLAG_OBJ_EXPANDO`, and each bit
//! has runtime writers alone: the redefine raises the first through
//! `__torajs_obj_flag_exotic_field`, and every expando write lands in
//! a props bag only `__torajs_obj_props_attach` gives an instance. So
//! the walk goes through a link seam ([`WALK_SEAM`], default the
//! any-lane serializer) the link stubs when neither writer is live —
//! the serializer roots the whole any world. A program that mints the
//! absent set raises the third bit the gate tests from its own object
//! literals, which no link fact can see; it calls the serializer
//! directly.

use std::collections::HashMap;

use crate::ast::PropKey;
use crate::check;
use crate::ssa::{FuncId, InstKind, Module, Operand, StructId, Type};
use crate::ssa_lower::{LowerCtx, declare_intrinsic};
use crate::ssa_lower_struct_exotic_gate::with_exotic_field_gate;

/// The walk's seam: `(value, gap Str or NULL, depth) -> Str`.
const WALK_SEAM: &str = "__torajs_anyv_json_stringify_exotic_obj";

pub(crate) fn declare(module: &mut Module, fn_table: &mut HashMap<String, FuncId>) {
    declare_intrinsic(
        module,
        fn_table,
        WALK_SEAM,
        &[Type::Any, Type::Str, Type::I64],
        Type::Str,
    );
}

/// The general path behind the gate, through [`WALK_SEAM`] unless the
/// program mints the absent set.
fn emit_exotic_walk(
    ctx: &mut LowerCtx,
    boxed: Operand,
    gap: Option<Operand>,
    depth: u32,
) -> Operand {
    if ctx.ast.mints_absent_set {
        return super::emit_any_walk(ctx, boxed, gap, depth);
    }
    let seam = ctx.fn_table[WALK_SEAM];
    let args = vec![
        boxed,
        gap.unwrap_or(Operand::ConstPtrNull),
        Operand::ConstI64(depth as i64),
    ];
    let v = ctx
        .f
        .append_inst(ctx.cur_block, InstKind::Call(seam, args), Type::Str, None);
    Operand::Value(v)
}

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
    let gated = val_op;
    // a layout that reaches itself has no finite compile-time unfold;
    // the any-lane walk recurses on the live value and owns the
    // §25.5.2 cycle TypeError
    // so does a layout whose cells can enumerate in a creation order
    // other than the layout's
    // and so does a layout whose getter may delete a member the
    // unfold reads after it (§25.5.2.4 fixes K, then reads each key)
    let walk_at_runtime = is_error
        || layout_reaches_itself(ctx, sid)
        || ctx.obj_order_varies(&Type::Obj(sid))
        || getter_may_delete(ctx, sid);
    super::composite::with_null_gate(ctx, &gated, "__json_obj_exotic", move |ctx| {
        lower_obj_nonnull(ctx, val_op, sid, fe_fields, gap, depth, walk_at_runtime)
    })
}

fn getter_may_delete(ctx: &LowerCtx, sid: StructId) -> bool {
    let props = crate::ssa_lower_struct_own_props::own_props(
        &ctx.struct_layouts[sid.0 as usize],
        ctx.fn_sigs,
    );
    crate::ssa_lower_struct_own_props::getter_may_delete(ctx, &props)
}

fn layout_reaches_itself(ctx: &LowerCtx, sid: StructId) -> bool {
    let mut seen_obj = vec![false; ctx.struct_layouts.len()];
    let mut seen_arr = vec![false; ctx.arr_layouts.len()];
    let mut work: Vec<Type> = ctx.struct_layouts[sid.0 as usize]
        .iter()
        .map(|(_, t)| *t)
        .collect();
    while let Some(t) = work.pop() {
        match t {
            Type::Obj(s) if s == sid => return true,
            Type::Obj(s) if !seen_obj[s.0 as usize] => {
                seen_obj[s.0 as usize] = true;
                work.extend(ctx.struct_layouts[s.0 as usize].iter().map(|(_, t)| *t));
            }
            Type::Arr(a) if !seen_arr[a.0 as usize] => {
                seen_arr[a.0 as usize] = true;
                work.push(ctx.arr_layouts[a.0 as usize]);
            }
            _ => {}
        }
    }
    false
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
    walk_at_runtime: bool,
) -> Operand {
    let walk_val = val_op;
    let walk_gap = gap;
    with_exotic_field_gate(
        ctx,
        &val_op.clone(),
        Type::Str,
        move |ctx| {
            let boxed = ctx.box_to_any(walk_val);
            let walked = emit_exotic_walk(ctx, boxed, walk_gap, depth);
            ctx.emit_throw_check(None);
            walked
        },
        move |ctx| {
            super::composite_obj::lower_obj(
                ctx,
                val_op,
                sid,
                fe_fields,
                gap,
                depth,
                walk_at_runtime,
            )
        },
    )
}
