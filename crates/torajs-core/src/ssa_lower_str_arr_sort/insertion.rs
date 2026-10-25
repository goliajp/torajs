//! The inline O(n²) insertion sort of `sort` / `toSorted` — moved out
//! of the parent at the file-size target.

use crate::ssa::{BinOp as SsaBinOp, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx};

use super::{pred, shift};

/// Emit the inline O(n²) insertion-sort fallback over `arr_ptr` in
/// place: outer loop grows the sorted prefix, inner-shift loop walks
/// it backward moving elements until the [`pred::emit_sort_pred`]
/// predicate releases. Leaves `ctx.cur_block` at the loop exit.
pub(super) fn emit_insertion_sort(
    ctx: &mut LowerCtx<'_>,
    arr_ptr: ValueId,
    elem_ty: Type,
    cmp_val: &Option<Operand>,
    cmp_ty: &Option<Type>,
    route: &pred::CmpRoute,
) {
    let len = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(arr_ptr), ARR_LEN_OFF),
        Type::I64,
        None,
    );
    let i_slot = ctx.alloca(Type::I64, Some("__sort_i"));
    // i = 1
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::ConstI64(1), Operand::Value(i_slot), 0),
    );
    let outer_hdr = ctx.f.add_block();
    let outer_body = ctx.f.add_block();
    let outer_after = ctx.f.add_block();
    ctx.f.set_term(ctx.cur_block, Terminator::Br(outer_hdr));
    // outer header: i < len?
    ctx.cur_block = outer_hdr;
    let i_now = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(i_slot), 0),
        Type::I64,
        None,
    );
    let in_outer = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Slt, Operand::Value(i_now), Operand::Value(len)),
        Type::Bool,
        None,
    );
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(in_outer),
            then_blk: outer_body,
            else_blk: outer_after,
        },
    );
    // outer body: load cur = xs[i], j = i, then inner loop
    ctx.cur_block = outer_body;
    let i_now2 = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(i_slot), 0),
        Type::I64,
        None,
    );
    // T-13.5: head-aware byte offset for arr.sort() reads.
    let (off_i_base, off_i) =
        ctx.emit_arr_slot_byte_offset(Operand::Value(arr_ptr), Operand::Value(i_now2), 8, false);
    let cur = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::LoadDyn(elem_ty, off_i_base, off_i),
        elem_ty,
        None,
    );
    // RFC 20260707 chunk 625 — comparison inputs for an Any elem
    // read through the kind-aware borrowed-box helper: a typed
    // block behind the static Arr<Any> view keeps raw slots, which
    // the raw LoadDyn misreads. The raw `cur` above stays the
    // SHIFT/write-back value (slot moves are layout-blind 8-byte
    // copies); only the predicate sees the boxed view.
    let cur_cmp = if elem_ty == Type::Any {
        crate::ssa_lower_anyv_inline::arr_get_any_borrowed(
            ctx,
            Operand::Value(arr_ptr),
            Operand::Value(i_now2),
        )
    } else {
        cur
    };
    let j_slot = ctx.alloca(Type::I64, Some("__sort_j"));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(i_now2), Operand::Value(j_slot), 0),
    );
    shift::emit_inner_shift_loop(
        ctx, arr_ptr, elem_ty, cmp_val, cmp_ty, cur_cmp, j_slot, route,
    );
    // inner after: xs[j] = cur
    let j_final = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(j_slot), 0),
        Type::I64,
        None,
    );
    let (off_jf_base, off_jf) =
        ctx.emit_arr_slot_byte_offset(Operand::Value(arr_ptr), Operand::Value(j_final), 8, false);
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::StoreDyn(Operand::Value(cur), off_jf_base, off_jf),
    );
    // §23.1.3.30.2 step 3 — the comparator's abrupt completion ends
    // the sort; the array is whole again at this point
    if cmp_val.is_some() {
        ctx.emit_throw_check(None);
    }
    // i++
    let i_next = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::BinOp(SsaBinOp::Add, Operand::Value(i_now2), Operand::ConstI64(1)),
        Type::I64,
        None,
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(i_next), Operand::Value(i_slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(outer_hdr));
    ctx.cur_block = outer_after;
}
