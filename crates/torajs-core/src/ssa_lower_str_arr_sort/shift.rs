//! Inner shift loop of the inline insertion sort — moved out of
//! the parent at the file-size limit.

use crate::ssa::{BinOp as SsaBinOp, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::LowerCtx;

use super::pred;

/// Inner shift loop of [`super::insertion::emit_insertion_sort`] (chunk 770
/// extraction): `while j > 0 && cmp(xs[j-1], cur) > 0` shift
/// xs[j] = xs[j-1], j-- . Leaves `ctx.cur_block` at the loop's
/// after-block (where the caller writes `xs[j] = cur`).
#[allow(clippy::too_many_arguments)]
pub(super) fn emit_inner_shift_loop(
    ctx: &mut LowerCtx<'_>,
    arr_ptr: ValueId,
    elem_ty: Type,
    cmp_val: &Option<Operand>,
    cmp_ty: &Option<Type>,
    cur_cmp: ValueId,
    j_slot: ValueId,
    route: &pred::CmpRoute,
) {
    let inner_hdr = ctx.f.add_block();
    let inner_check = ctx.f.add_block();
    let inner_body = ctx.f.add_block();
    let inner_after = ctx.f.add_block();
    ctx.f.set_term(ctx.cur_block, Terminator::Br(inner_hdr));
    // inner header: j > 0?
    ctx.cur_block = inner_hdr;
    let j_now = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(j_slot), 0),
        Type::I64,
        None,
    );
    let j_pos = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Sgt, Operand::Value(j_now), Operand::ConstI64(0)),
        Type::Bool,
        None,
    );
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(j_pos),
            then_blk: inner_check,
            else_blk: inner_after,
        },
    );
    // inner check: load xs[j-1], call cmp, test > 0
    ctx.cur_block = inner_check;
    let j_minus_1 = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::BinOp(SsaBinOp::Sub, Operand::Value(j_now), Operand::ConstI64(1)),
        Type::I64,
        None,
    );
    // Chunk 625 — the comparison's prev input takes the kind-aware
    // boxed read for Any elems (see cur_cmp in the caller); the raw
    // LoadDyn stays for the shift copy in inner_body.
    let prev = if elem_ty == Type::Any {
        crate::ssa_lower_anyv_inline::arr_get_any_borrowed(
            ctx,
            Operand::Value(arr_ptr),
            Operand::Value(j_minus_1),
        )
    } else {
        let (off_jm1_base, off_jm1) = ctx.emit_arr_slot_byte_offset(
            Operand::Value(arr_ptr),
            Operand::Value(j_minus_1),
            8,
            false,
        );
        ctx.f.append_inst(
            ctx.cur_block,
            InstKind::LoadDyn(elem_ty, off_jm1_base, off_jm1),
            elem_ty,
            None,
        )
    };
    // V3-18 wedge — branch on cmp_val presence.
    // With a user comparator: call it, test ret > 0.
    // Without: directly compare prev > cur using the
    // element-type-aware predicate (Sgt for I64,
    // Ogt for F64, str_locale_compare for Str).
    let pred_v = pred::emit_sort_pred(ctx, cmp_val, cmp_ty, prev, cur_cmp, elem_ty, route);
    // A comparator that threw ends the shift here: `cur` still has
    // to go back into the hole before the caller propagates, or the
    // slot it left keeps a duplicate of its neighbour.
    let shift_blk = if cmp_val.is_some() {
        ctx.f.add_block()
    } else {
        inner_body
    };
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(pred_v),
            then_blk: shift_blk,
            else_blk: inner_after,
        },
    );
    if shift_blk != inner_body {
        ctx.cur_block = shift_blk;
        let active = ctx.emit_throw_active_load();
        let threw = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::ICmp(IPred::Ne, Operand::Value(active), Operand::ConstI64(0)),
            Type::Bool,
            None,
        );
        ctx.f.set_term(
            ctx.cur_block,
            Terminator::CondBr {
                cond: Operand::Value(threw),
                then_blk: inner_after,
                else_blk: inner_body,
            },
        );
    }
    // inner body: xs[j] = xs[j-1]; j--
    ctx.cur_block = inner_body;
    let (off_j_base, off_j) =
        ctx.emit_arr_slot_byte_offset(Operand::Value(arr_ptr), Operand::Value(j_now), 8, false);
    // off_jm1 was computed in inner_check; recompute
    // here since this is a different block.
    let (off_jm1_base, off_jm1_b) =
        ctx.emit_arr_slot_byte_offset(Operand::Value(arr_ptr), Operand::Value(j_minus_1), 8, false);
    let prev2 = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::LoadDyn(elem_ty, off_jm1_base, off_jm1_b),
        elem_ty,
        None,
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::StoreDyn(Operand::Value(prev2), off_j_base, off_j),
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(j_minus_1), Operand::Value(j_slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(inner_hdr));
    ctx.cur_block = inner_after;
}
