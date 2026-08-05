//! The redefined-field gate in front of the static struct unfold —
//! RFC 20260806-declared-field-redefine.
//!
//! `JSON.stringify` on a statically typed class instance unfolds the
//! layout's field list at compile time, which is the whole reason it
//! is fast. But §25.5.2 SerializeJSONObject walks EnumerableOwnProperties,
//! and `Object.defineProperty(c, "f", {enumerable: false})` moves that
//! set at RUNTIME — a fact no unfold can carry.
//!
//! The two are reconciled the way the array tier already reconciles
//! the same tension (`emit_index_integrity_guard` over
//! `FLAG_ARR_EXOTIC_INDEX`): one masked test on the header word the
//! instance carries anyway, and only a hit takes the slower, fully
//! general path — here the any-lane runtime walk, which consults the
//! sidecar per field and produces byte-identical output for every
//! shape both tiers can express.
//!
//! An instance that was never passed to `defineProperty` pays one
//! not-taken branch and reaches the same unfold as before.

use crate::check;
use crate::ssa::{IPred, InstKind, Operand, StructId, Terminator, Type};
use crate::ssa_lower::LowerCtx;

/// Header word is `[refcount u32 | tag u16 | flags u16]`, so flags bit
/// N sits at bit 48+N of the I64 load.
const FLAGS_SHIFT: u32 = 48;

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
    fe_fields: Option<Vec<(String, check::Type)>>,
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
    fe_fields: Option<Vec<(String, check::Type)>>,
    gap: Option<Operand>,
    depth: u32,
    is_error: bool,
) -> Operand {
    let mask = (torajs_rc::FLAG_OBJ_EXOTIC_FIELD as i64) << FLAGS_SHIFT;
    let hdr = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, val_op.clone(), 0),
        Type::I64,
        None,
    );
    let bits = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::BinOp(
            crate::ssa::BinOp::And,
            Operand::Value(hdr),
            Operand::ConstI64(mask),
        ),
        Type::I64,
        None,
    );
    let exotic = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Ne, Operand::Value(bits), Operand::ConstI64(0)),
        Type::Bool,
        None,
    );
    let out_slot = ctx.alloca(Type::Str, None);
    let walk_blk = ctx.f.add_block();
    let unfold_blk = ctx.f.add_block();
    let done_blk = ctx.f.add_block();
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(exotic),
            then_blk: walk_blk,
            else_blk: unfold_blk,
        },
    );

    // Redefined — hand the cell to the any-lane walk, which reads each
    // field's attributes rather than assuming the layout's.
    ctx.cur_block = walk_blk;
    let boxed = ctx.box_to_any(val_op.clone());
    let walked = super::emit_any_walk(ctx, boxed, gap.clone(), depth);
    ctx.emit_throw_check(None);
    let blk = ctx.cur_block;
    ctx.f
        .append_void(blk, InstKind::Store(walked, Operand::Value(out_slot), 0));
    ctx.f.set_term(blk, Terminator::Br(done_blk));

    // The ordinary instance — the unfold, untouched.
    ctx.cur_block = unfold_blk;
    let unfolded =
        super::composite_obj::lower_obj(ctx, val_op, sid, fe_fields, gap, depth, is_error);
    let blk = ctx.cur_block;
    ctx.f
        .append_void(blk, InstKind::Store(unfolded, Operand::Value(out_slot), 0));
    ctx.f.set_term(blk, Terminator::Br(done_blk));

    ctx.cur_block = done_blk;
    let v = ctx.f.append_inst(
        done_blk,
        InstKind::Load(Type::Str, Operand::Value(out_slot), 0),
        Type::Str,
        None,
    );
    Operand::Value(v)
}
