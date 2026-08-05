//! Storage allocation for a typed array literal — split out of
//! [`crate::ssa_lower_array`] when the scalar-nullable lane gate
//! pushed that file past the 500-line limit.
//!
//! Two shapes, chosen by the escape verifier: a stack alloca the
//! literal is born self-describing in, and the ordinary heap cell.
//! Building the element list and choosing a lane stay in the parent;
//! only the question "where do these N slots live" is answered here.

use crate::ssa::{BinOp as SsaBinOp, InstKind, Operand, Type};
use crate::ssa_lower::{ARR_PROPS_OFF, LowerCtx};

pub(crate) fn alloc_stack_arr(
    ctx: &mut LowerCtx<'_>,
    arr_id: crate::ssa::ArrId,
    n: i64,
) -> crate::ssa::ValueId {
    let total_bytes = crate::ssa_lower::ARR_CELL_SIZE + (n as u64) * 8;
    let cur_block = ctx.cur_block;
    let p = ctx.f.append_inst(
        cur_block,
        InstKind::AllocaBytes(total_bytes),
        Type::Arr(arr_id),
        None,
    );
    // Header packed: tag=2 (ARR) bits 32..48, flags bits 48..64 =
    // STATIC (4) | element-kind field (bits 10-12).
    //
    // The kind is baked HERE rather than written by
    // `__torajs_arr_mark_kind` at the boxing boundary: that helper
    // refuses every FLAG_STATIC_LITERAL block ("never write
    // `.rodata`"), so a stack literal would reach the kind-aware
    // readers as ARR_KIND_UNSET and answer `undefined`. A stack
    // alloca is writable, but it is also known-typed at emit time —
    // so it is born self-describing and the runtime gate stays
    // intact. `on_stack` implies a non-refcounted element, so the
    // chain is always one scalar level (I64 / F64 / Bool).
    let elem = ctx.arr_layouts[arr_id.0 as usize];
    let kind = ctx.arr_kind_chain(&elem, 0);
    let hdr_packed: i64 = (2i64 << 32) | ((4i64 | ((kind as i64) << 10)) << 48);
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Store(Operand::ConstI64(hdr_packed), Operand::Value(p), 0),
    );
    ctx.f.append_void(
        cur_block,
        InstKind::Store(Operand::ConstI64(n), Operand::Value(p), 16),
    );
    ctx.f.append_void(
        cur_block,
        InstKind::Store(Operand::ConstI64(0), Operand::Value(p), ARR_PROPS_OFF),
    );
    // B1 — self-referential data pointer into the alloca's own
    // inline region.
    let inline0 = ctx.f.append_inst(
        cur_block,
        InstKind::BinOp(
            SsaBinOp::Add,
            Operand::Value(p),
            Operand::ConstI64(crate::ssa_lower::ARR_CELL_SIZE as i64),
        ),
        Type::I64,
        None,
    );
    ctx.f.append_void(
        cur_block,
        InstKind::Store(
            Operand::Value(inline0),
            Operand::Value(p),
            crate::ssa_lower::ARR_DATA_PTR_OFF,
        ),
    );
    p
}

pub(crate) fn alloc_heap_arr(
    ctx: &mut LowerCtx<'_>,
    arr_id: crate::ssa::ArrId,
    n: i64,
) -> crate::ssa::ValueId {
    let cur_block = ctx.cur_block;
    ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.arr_alloc, vec![Operand::ConstI64(n)]),
        Type::Arr(arr_id),
        None,
    )
}
