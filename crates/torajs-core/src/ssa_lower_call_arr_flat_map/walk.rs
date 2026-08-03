//! flatMap loop-body emitters — the inner-array walk, the scalar /
//! dynamic push arms, and the loop close. Moved verbatim from the
//! parent when the spec-arity + knife-4 + dynamic-spread work
//! (rotation 286) pushed it past the 500-line cap.

use crate::ssa::{BinOp as SsaBinOp, BlockId, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx};

/// Emit the inner-array walk (`j in 0..inner_arr.length` pushing each
/// element into `dst`). Caller must have `ctx.cur_block` pointing at the
/// outer body block (after the closure call). On exit, `ctx.cur_block`
/// points at the inner-loop after-block (the caller continues with
/// drop + outer i++).
pub(super) fn emit_inner_walk(
    ctx: &mut LowerCtx<'_>,
    inner_arr: ValueId,
    dst_slot: ValueId,
    dst_arr_ty: Type,
    dst_elem_ty: Type,
) {
    let ih = ctx.f.add_block();
    let ib = ctx.f.add_block();
    let ia = ctx.f.add_block();
    let j_slot = ctx.alloca(Type::I64, Some("__fm_j"));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::ConstI64(0), Operand::Value(j_slot), 0),
    );
    let inner_len = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(inner_arr), ARR_LEN_OFF),
        Type::I64,
        None,
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(ih));
    ctx.cur_block = ih;
    let j_now = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(j_slot), 0),
        Type::I64,
        None,
    );
    let jcmp = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Slt, Operand::Value(j_now), Operand::Value(inner_len)),
        Type::Bool,
        None,
    );
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(jcmp),
            then_blk: ib,
            else_blk: ia,
        },
    );
    ctx.cur_block = ib;
    // T-13.5: head-aware offset for flatMap inner walk.
    // Chunk 625 — same kind-aware read for the cb-returned inner
    // array's Any elems (a typed array return is a typed block).
    let inner_elem = if dst_elem_ty == Type::Any {
        ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.arr_get_any_boxed,
                vec![Operand::Value(inner_arr), Operand::Value(j_now)],
            ),
            Type::Any,
            None,
        )
    } else {
        let (joff_base, joff) = ctx.emit_arr_slot_byte_offset(
            Operand::Value(inner_arr),
            Operand::Value(j_now),
            3,
            false,
        );
        ctx.f.append_inst(
            ctx.cur_block,
            InstKind::LoadDyn(dst_elem_ty, joff_base, joff),
            dst_elem_ty,
            None,
        )
    };
    if dst_elem_ty.is_refcounted() {
        ctx.emit_rc_inc(Operand::Value(inner_elem));
    }
    let cur_dst = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(dst_arr_ty, Operand::Value(dst_slot), 0),
        dst_arr_ty,
        None,
    );
    // RFC 20260726-array-elem-width knife 7 — arr_push takes the slot
    // as raw i64 bits, so an f64 element must be bitcast, not handed
    // over as a value: passing it straight converted 0.5 to 0 on the
    // way in, and the f64-typed read afterwards turned the stored
    // integer back into a denormal. Every other push site already goes
    // through this shorthand (`emit_map` in the shared loop).
    let push_elem = ctx.raw_slot_arg(Operand::Value(inner_elem));
    let new_dst = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.arr_push,
            vec![Operand::Value(cur_dst), push_elem],
        ),
        dst_arr_ty,
        None,
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(new_dst), Operand::Value(dst_slot), 0),
    );
    let j_next = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::BinOp(SsaBinOp::Add, Operand::Value(j_now), Operand::ConstI64(1)),
        Type::I64,
        None,
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(j_next), Operand::Value(j_slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(ih));
    ctx.cur_block = ia;
}

/// Scalar-arm variant of [`emit_inner_walk`] + [`emit_close_and_load`]
/// fused: `cb_ret` is an owned scalar U (not an Array<U>), so we push
/// it into `dst` verbatim (the +1 the cb returned carries transfers to
/// the dst slot) then do the outer i++ + br. No inner_arr drop — there
/// was no wrapping array. Caller must have `ctx.cur_block` at the
/// outer body block (after the closure call).
///
/// Refcount contract: an owned refcounted scalar (Str / Substr / Any)
/// has rc = 1 at cb return; arr_push stores it in the dst slot (which
/// takes over that share). Inline scalars (I64 / F64 / Bool) carry no
/// rc — the push just stores the raw bits.
#[allow(clippy::too_many_arguments)]
pub(super) fn emit_scalar_push_and_close(
    ctx: &mut LowerCtx<'_>,
    cb_ret: ValueId,
    i_slot: ValueId,
    oh: BlockId,
    oa: BlockId,
    dst_slot: ValueId,
    dst_arr_ty: Type,
) -> ValueId {
    // Load current dst, push scalar, store back the returned pointer
    // (arr_push may realloc the data buffer — cell itself never moves
    // but the buffer ptr in the cell may, so re-capture the return).
    let cur_dst = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(dst_arr_ty, Operand::Value(dst_slot), 0),
        dst_arr_ty,
        None,
    );
    // Knife 7, scalar arm — same raw-bits contract as the inner walk:
    // a callback answering an f64 scalar per §23.1.3.11 step 8.d must
    // reach arr_push as bits, not as a value. Knife 8 — when the class
    // made the dst wider than this callback's return, convert rather
    // than bitcast the integer into an f64 slot.
    let scalar = Operand::Value(cb_ret);
    let scalar = if ctx.operand_ty(&scalar) == Type::I64
        && matches!(dst_arr_ty, Type::Arr(id) if ctx.arr_layouts[id.0 as usize] == Type::F64)
    {
        ctx.coerce_to_f64(scalar)
    } else {
        scalar
    };
    let push_ret = ctx.raw_slot_arg(scalar);
    let new_dst = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.arr_push,
            vec![Operand::Value(cur_dst), push_ret],
        ),
        dst_arr_ty,
        None,
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(new_dst), Operand::Value(dst_slot), 0),
    );
    // Outer i++.
    let i_then = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(i_slot), 0),
        Type::I64,
        None,
    );
    let i_next = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::BinOp(SsaBinOp::Add, Operand::Value(i_then), Operand::ConstI64(1)),
        Type::I64,
        None,
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(i_next), Operand::Value(i_slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(oh));
    ctx.cur_block = oa;
    ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(dst_arr_ty, Operand::Value(dst_slot), 0),
        dst_arr_ty,
        None,
    )
}

/// Emit the inner after-block close (drop inner_arr; outer i++ + br oh)
/// then position at `oa` and load the final `dst`. Caller wraps the
/// returned `ValueId` into an `Operand::Value` for return.
#[allow(clippy::too_many_arguments)]
pub(super) fn emit_close_and_load(
    ctx: &mut LowerCtx<'_>,
    inner_arr: ValueId,
    inner_arr_ty: Type,
    i_slot: ValueId,
    oh: BlockId,
    oa: BlockId,
    dst_slot: ValueId,
    dst_arr_ty: Type,
) -> ValueId {
    // Drop the inner array (its elements are now in dst, and we already
    // rc_inc'd for refcounted dst push).
    ctx.emit_drop_value(Operand::Value(inner_arr), inner_arr_ty);
    // Outer i++.
    let i_then = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(i_slot), 0),
        Type::I64,
        None,
    );
    let i_next = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::BinOp(SsaBinOp::Add, Operand::Value(i_then), Operand::ConstI64(1)),
        Type::I64,
        None,
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(i_next), Operand::Value(i_slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(oh));
    ctx.cur_block = oa;
    ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(dst_arr_ty, Operand::Value(dst_slot), 0),
        dst_arr_ty,
        None,
    )
}

/// Dynamic arm — the callback's static return is `Any`, so ES
/// §23.1.3.11 step 8's IsArray test must run at runtime: an array
/// answer spreads through the inner walk (kind-aware reads, matching
/// any inner flavor), anything else pushes as the element itself.
/// The spread side drops the inner array's owned box after its
/// elements were rc_inc'd into dst; the scalar side transfers the
/// box's stake to the dst slot verbatim.
#[allow(clippy::too_many_arguments)]
pub(super) fn emit_dynamic_push_and_close(
    ctx: &mut LowerCtx<'_>,
    cb_ret: ValueId,
    i_slot: ValueId,
    oh: BlockId,
    oa: BlockId,
    dst_slot: ValueId,
    dst_arr_ty: Type,
) -> ValueId {
    let spread_blk = ctx.f.add_block();
    let scalar_blk = ctx.f.add_block();
    let join_blk = ctx.f.add_block();
    let is_arr = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.any_is_arr, vec![Operand::Value(cb_ret)]),
        Type::Bool,
        None,
    );
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(is_arr),
            then_blk: spread_blk,
            else_blk: scalar_blk,
        },
    );
    // Spread — unbox the array pointer, walk its elements into dst
    // (the Any dst flavor reads every inner flavor kind-aware), then
    // drop the callback's owned box.
    ctx.cur_block = spread_blk;
    let inner_ptr = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.any_unbox_value, vec![Operand::Value(cb_ret)]),
        Type::I64,
        None,
    );
    emit_inner_walk(ctx, inner_ptr, dst_slot, dst_arr_ty, Type::Any);
    ctx.emit_drop_value(Operand::Value(cb_ret), Type::Any);
    ctx.f.set_term(ctx.cur_block, Terminator::Br(join_blk));
    // Scalar — push the box bits verbatim (stake transfers to dst).
    ctx.cur_block = scalar_blk;
    let cur_dst = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(dst_arr_ty, Operand::Value(dst_slot), 0),
        dst_arr_ty,
        None,
    );
    let push_bits = ctx.raw_slot_arg(Operand::Value(cb_ret));
    let new_dst = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.arr_push,
            vec![Operand::Value(cur_dst), push_bits],
        ),
        dst_arr_ty,
        None,
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(new_dst), Operand::Value(dst_slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(join_blk));
    // Join — outer i++ and loop back.
    ctx.cur_block = join_blk;
    let i_then = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(i_slot), 0),
        Type::I64,
        None,
    );
    let i_next = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::BinOp(SsaBinOp::Add, Operand::Value(i_then), Operand::ConstI64(1)),
        Type::I64,
        None,
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(i_next), Operand::Value(i_slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(oh));
    ctx.cur_block = oa;
    ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(dst_arr_ty, Operand::Value(dst_slot), 0),
        dst_arr_ty,
        None,
    )
}
