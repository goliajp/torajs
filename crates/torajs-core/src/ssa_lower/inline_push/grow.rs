//! The cold edges of an append to an array that holds its class in
//! place: the grow of a buffer with no room for the element, and the
//! trap a debug compiler sets on a lock bit (see the parent module).

use crate::ssa::{BinOp, BlockId, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::{ARR_CAP_HEAD_OFF, LowerCtx, PreReserveState};

use super::super::inline_elem::emit_scaled_index;

/// The capacity of `arr`'s buffer, in slots.
pub(super) fn load_cap(ctx: &mut LowerCtx<'_>, arr: Operand) -> Operand {
    let cb = ctx.cur_block;
    let packed = InstKind::Load(Type::I64, arr, ARR_CAP_HEAD_OFF);
    let packed = Operand::Value(ctx.f.append_inst(cb, packed, Type::I64, None));
    // the low half of the packed word; the deque head is the high half
    ctx.bin(
        BinOp::And,
        packed,
        Operand::ConstI64(0xffff_ffff),
        Type::I64,
    )
}

/// Grow `arr`, whose buffer holds `cap` slots of `k`-slot elements, to
/// at least `need` slots: twice what it holds, since the runtime grows
/// by exactly what it is asked, and never past the u32 slot count a
/// capacity can hold; a `need` past that raises the RangeError of
/// §10.4.2.4, from the block in `raise` (`emit_too_long_guard`).
pub(super) fn emit_grow_to_fit(
    ctx: &mut LowerCtx<'_>,
    arr: Operand,
    need: Operand,
    cap: Operand,
    k: u64,
    raise: &mut Option<BlockId>,
) {
    // the capacity is a u32 count of slots, so an array of `k`-slot
    // elements holds at most `u32::MAX / k` of them
    let max = (u32::MAX as u64 / k * k) as i64;
    let over = ctx.cmp(IPred::Sgt, need, Operand::ConstI64(max));
    ctx.emit_too_long_guard(over, raise);
    let doubled = ctx.bin(BinOp::Add, need, cap, Type::I64);
    // min(doubled, max) by arithmetic: the lowering may not form a
    // select, which only the passes after the egraph introduce
    let room = ctx.bin(BinOp::Sub, Operand::ConstI64(max), doubled, Type::I64);
    let sign = ctx.bin(BinOp::AShr, room, Operand::ConstI64(63), Type::I64);
    let short = ctx.bin(BinOp::And, room, sign, Type::I64);
    let target = ctx.bin(BinOp::Add, doubled, short, Type::I64);
    let cb = ctx.cur_block;
    let arr_ty = ctx.operand_ty(&arr);
    let reserve = InstKind::Call(ctx.intrinsics.arr_reserve, vec![arr, target]);
    ctx.f.append_inst(cb, reserve, arr_ty, None);
}

/// The header bits that refuse an append (§10.1.8.1) or the length
/// write that follows it.
pub(crate) const APPEND_LOCK_BITS: u16 = torajs_rc::FLAG_FROZEN
    | torajs_rc::FLAG_SEALED
    | torajs_rc::FLAG_NON_EXTENSIBLE
    | torajs_rc::FLAG_ARR_LENGTH_RO;

/// Traps when `arr` carries a lock bit, which the census promises no
/// array holding its elements in place can. Emitted only by a debug
/// compiler or under the value-class strict switch (the conformance
/// gate), so a shipped program pays nothing.
pub(super) fn assert_unlocked(ctx: &mut LowerCtx<'_>, arr: Operand) {
    let cb = ctx.cur_block;
    let word = ctx
        .f
        .append_inst(cb, InstKind::Load(Type::I32, arr, 4), Type::I32, None);
    let mask = Operand::ConstI32(((APPEND_LOCK_BITS as u32) << 16) as i32);
    let bits = ctx.bin(BinOp::And, Operand::Value(word), mask, Type::I32);
    let locked = ctx.cmp(IPred::Ne, bits, Operand::ConstI32(0));
    let trap_blk = ctx.f.add_block();
    let cont_blk = ctx.f.add_block();
    let cb = ctx.cur_block;
    ctx.f.set_term(
        cb,
        Terminator::CondBr {
            cond: locked,
            then_blk: trap_blk,
            else_blk: cont_blk,
        },
    );
    ctx.f.set_term(trap_blk, Terminator::Unreachable);
    ctx.cur_block = cont_blk;
}

/// The cold edge of a guarded append of a `k`-slot element at `len`:
/// grow the buffer when the slot is not there, then go on to
/// `fast_blk`. The budget may be behind a grow made earlier in the
/// loop, so the buffer's own capacity decides.
pub(super) fn make_room(
    ctx: &mut LowerCtx<'_>,
    st: PreReserveState,
    len: ValueId,
    k: u64,
    fast_blk: BlockId,
) {
    let arr = Operand::Value(st.arr_ptr);
    if cfg!(debug_assertions) || crate::ssa_lower_pass_2_5::value_class_check::strict() {
        assert_unlocked(ctx, arr);
    }
    // no head: the census keeps a class whose arrays' head may move
    // from being held in place
    let cap = load_cap(ctx, arr);
    let next = ctx.bin(
        BinOp::Add,
        Operand::Value(len),
        Operand::ConstI64(1),
        Type::I64,
    );
    let cb = ctx.cur_block;
    let need = Operand::Value(emit_scaled_index(ctx.f, cb, next, k));
    let full = ctx.cmp(IPred::Ult, cap, need);
    let grow_blk = ctx.f.add_block();
    let cb = ctx.cur_block;
    ctx.f.set_term(
        cb,
        Terminator::CondBr {
            cond: full,
            then_blk: grow_blk,
            else_blk: fast_blk,
        },
    );
    ctx.cur_block = grow_blk;
    // the grow may raise
    ctx.emit_settle_len(st, len);
    let mut raise = ctx.prereserve.walk_raise.take();
    emit_grow_to_fit(ctx, arr, need, cap, k, &mut raise);
    ctx.prereserve.walk_raise = raise;
    let cb = ctx.cur_block;
    ctx.f.set_term(cb, Terminator::Br(fast_blk));
}
