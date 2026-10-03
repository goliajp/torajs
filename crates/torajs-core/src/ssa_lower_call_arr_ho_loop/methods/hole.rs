//! `map`'s hole arm — what the product receives for a source index
//! §23.1.3.20's HasProperty gate says is absent. Moved out of
//! [`super`] (file-size split) when the arm learned the pre-reserved
//! append.

use super::emit_undef_any_box;
use crate::ssa::{BlockId, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::{LowerCtx, PreReserveState};

/// The element type of a `map` product that can hold a hole: a boxed
/// one, or a typed one whose slots have a hole word (F64's sentinel, a
/// boolean 2, a NULL pointer). An integer product cannot say absent,
/// so that shape keeps today's answer (the callback runs on the hole)
/// rather than a shorter product or a corrupt slot. Recorded boundary;
/// it lifts the day a typed product can express absence, which is the
/// day the typed `delete` does.
fn map_product_holes(ctx: &LowerCtx<'_>, dst_arr_ty: Type) -> Option<Type> {
    let Type::Arr(id) = dst_arr_ty else {
        return None;
    };
    let elem = ctx.arr_layouts[id.0 as usize];
    (matches!(elem, Type::Any | Type::F64 | Type::Bool)
        || crate::ssa_lower_index_undef::has_undef_spelling(elem))
    .then_some(elem)
}

/// The block `map` jumps to when the source index is absent: the
/// product has the source's length, so the skipped index arrives as a
/// hole of its own (step 6.c leaves it uncreated) — append the hole
/// word and mark it, exactly as the array-literal elision lowering
/// does. `None` = this product cannot hole (see [`map_product_holes`]).
///
/// `seam` picks the mark's entry: the link-judged seam
/// (`arr_map_hole_slow`) for an arm the loop reaches only after the
/// source's header said it can have an absent index — the exotic,
/// slot-hole and sparse-tail bits, each with one writer entry the
/// judgment reads — or the writer itself (`arr_mark_last_hole`) for an
/// arm a loop that read no such bit can reach (a shortened length).
pub(in crate::ssa_lower_call_arr_ho_loop) fn emit_map_hole_block(
    ctx: &mut LowerCtx<'_>,
    dst_arr_ty: Type,
    dst: PreReserveState,
    step_blk: BlockId,
    seam: bool,
) -> Option<BlockId> {
    let elem_ty = map_product_holes(ctx, dst_arr_ty)?;
    let hole_blk = ctx.f.add_block();
    let saved = ctx.cur_block;
    ctx.cur_block = hole_blk;
    // A boxed product takes the undefined box; a typed one its element
    // type's hole word, which `arr_mark_last_hole` leaves in place.
    let undef = if elem_ty == Type::Any {
        Operand::Value(emit_undef_any_box(ctx))
    } else {
        crate::ssa_lower_index_undef::typed_hole_raw(elem_ty)
    };
    // The mark finds its index through the cell's length word, so the
    // running count settles there first. The count stays authoritative
    // for the appends that follow.
    let _ = ctx.emit_prereserved_push(dst, undef);
    ctx.emit_prereserved_len_writeback(dst);
    let mark = if seam {
        ctx.intrinsics.arr_map_hole_slow
    } else {
        ctx.intrinsics.arr_mark_last_hole
    };
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(mark, vec![Operand::Value(dst.arr_ptr)]),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(step_blk));
    ctx.cur_block = saved;
    Some(hole_blk)
}
