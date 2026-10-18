//! The `Arr` field of a `__drop_<C>` body — the typed array release of
//! `ssa_lower_emit_drop_value::emit_drop_arr`, written into a bare
//! `ssa::Function`:
//!
//! ```text
//! nullish(a) → after
//! Str / Substr elements:  __torajs_arr_drop_str_elems(a)
//! any elements:           __torajs_arr_drop_any(a)
//! refcounted elements:    rc == 1 → release each slot in [0, len)
//!                         then __torajs_arr_drop(a)
//! scalar elements, or an
//! element held in place:  __torajs_arr_drop_scalar(a)
//! anything else:          __torajs_arr_drop(a)
//! ```
//!
//! The runtime's tag-dispatched release (`value_drop_heap`) walks an
//! array's slots by the element kind in its header, which is written
//! only when the array crosses into `any`; an array that never did
//! would go out through `__torajs_arr_drop` with its elements still
//! held. Element references belong to the array block, so only the
//! last owner walks them (`rc == 1`: the `arr_drop` below is the
//! hit-zero dec); an array the any side also holds is walked by the
//! runtime when that reference goes.

use crate::ssa::{
    self, ArrId, BinOp as SsaBinOp, BlockId, IPred, InstKind, Operand, Terminator, Type,
};
use crate::ssa_lower::{ARR_DATA_PTR_OFF, ARR_LEN_OFF};
use crate::ssa_lower_emit_drop_value::class_drop::{
    Cx, branch, branch_nullish, emit_release_value,
};

pub(crate) fn emit_release_arr(
    f: &mut ssa::Function,
    cx: &Cx<'_>,
    blk: BlockId,
    val: Operand,
    arr_id: ArrId,
    stack: &mut Vec<u32>,
) -> BlockId {
    let elem_ty = cx.arr_layouts[arr_id.0 as usize];
    let after = f.add_block();
    let mut cur = branch_nullish(f, blk, val, after);
    let drop_fid = if matches!(elem_ty, Type::Str | Type::Substr) {
        cx.intr.arr_drop_str_elems
    } else if elem_ty == Type::Any {
        cx.intr.arr_drop_any
    } else if elem_ty.elems_release_nothing() {
        cx.intr.arr_drop_scalar
    } else {
        if elem_ty.is_refcounted() {
            cur = emit_last_owner_walk(f, cx, cur, val, elem_ty, stack);
        }
        cx.intr.arr_drop
    };
    f.append_void(cur, InstKind::Call(drop_fid, vec![val]));
    f.set_term(cur, Terminator::Br(after));
    after
}

/// `if (rc == 1) for (i in 0..len) release(slot[i])` — answers the
/// block after the walk.
fn emit_last_owner_walk(
    f: &mut ssa::Function,
    cx: &Cx<'_>,
    blk: BlockId,
    arr: Operand,
    elem_ty: Type,
    stack: &mut Vec<u32>,
) -> BlockId {
    let rc = f.append_inst(blk, InstKind::Load(Type::I32, arr, 0), Type::I32, None);
    let is_last = f.append_inst(
        blk,
        InstKind::ICmp(IPred::Eq, Operand::Value(rc), Operand::ConstI32(1)),
        Type::Bool,
        None,
    );
    let walk = f.add_block();
    let done = f.add_block();
    branch(f, blk, Operand::Value(is_last), walk, done);
    // the loop counter lives in the fn's first block, like every
    // lowering-side alloca
    let i_slot = f.append_inst(cx.prelude, InstKind::Alloca(Type::I64), Type::Ptr, None);
    let i_slot = Operand::Value(i_slot);
    f.append_void(walk, InstKind::Store(Operand::ConstI64(0), i_slot, 0));
    let len = f.append_inst(
        walk,
        InstKind::Load(Type::I64, arr, ARR_LEN_OFF),
        Type::I64,
        None,
    );
    // head * 8 (the high half of the packed cap/head word at +16) and
    // the data pointer, both loop-invariant: a release never grows
    // the array
    let packed = f.append_inst(walk, InstKind::Load(Type::I64, arr, 16), Type::I64, None);
    let head = f.append_inst(
        walk,
        InstKind::BinOp(
            SsaBinOp::LShr,
            Operand::Value(packed),
            Operand::ConstI64(32),
        ),
        Type::I64,
        None,
    );
    let head_x8 = f.append_inst(
        walk,
        InstKind::BinOp(SsaBinOp::Shl, Operand::Value(head), Operand::ConstI64(3)),
        Type::I64,
        None,
    );
    let data = f.append_inst(
        walk,
        InstKind::Load(Type::Ptr, arr, ARR_DATA_PTR_OFF),
        Type::Ptr,
        None,
    );
    let header = f.add_block();
    let body = f.add_block();
    f.set_term(walk, Terminator::Br(header));
    let i_now = f.append_inst(
        header,
        InstKind::Load(Type::I64, i_slot, 0),
        Type::I64,
        None,
    );
    let more = f.append_inst(
        header,
        InstKind::ICmp(IPred::Slt, Operand::Value(i_now), Operand::Value(len)),
        Type::Bool,
        None,
    );
    branch(f, header, Operand::Value(more), body, done);
    let scaled = f.append_inst(
        body,
        InstKind::BinOp(SsaBinOp::Shl, Operand::Value(i_now), Operand::ConstI64(3)),
        Type::I64,
        None,
    );
    let off = f.append_inst(
        body,
        InstKind::BinOp(
            SsaBinOp::Add,
            Operand::Value(scaled),
            Operand::Value(head_x8),
        ),
        Type::I64,
        None,
    );
    let elem = f.append_inst(
        body,
        InstKind::LoadDyn(elem_ty, Operand::Value(data), Operand::Value(off)),
        elem_ty,
        None,
    );
    let cur = emit_release_value(f, cx, body, Operand::Value(elem), elem_ty, stack);
    let i_next = f.append_inst(
        cur,
        InstKind::BinOp(SsaBinOp::Add, Operand::Value(i_now), Operand::ConstI64(1)),
        Type::I64,
        None,
    );
    f.append_void(cur, InstKind::Store(Operand::Value(i_next), i_slot, 0));
    f.set_term(cur, Terminator::Br(header));
    done
}
