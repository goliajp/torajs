//! The typed grow-kernel call shared by the push lanes — moved out of
//! `ssa_lower_call_arr_push.rs` at the 500-line limit. The host decides
//! which receiver shapes a push takes; this file emits the append and
//! settles the pushed value's stake on both edges.

use crate::ast::ExprId;
use crate::ssa::{BinOp, FuncId, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::{ARR_CAP_HEAD_OFF, ARR_DATA_PTR_OFF, ARR_LEN_OFF, LowerCtx};

/// The out-of-line push an opened push falls back to: the same kernel
/// under a name codegen leaves a call (it opens `__torajs_arr_push`
/// itself, which would put a second fast path on the cold arm).
pub(crate) const PUSH_COLD: &str = "__torajs_arr_push_cold";

/// The typed grow-kernel call (`arr_push` / `arr_unshift`) with its
/// throw check and the value's stake settled on both edges.
///
/// The kernels refuse a non-extensible receiver by recording a
/// TypeError and storing nothing (`__torajs_arr_extend_guard`). They
/// are intrinsics, which `emit_throw_check(Some)` skips, so the check
/// is forced with `None` — pop's precedent. Before it, a frozen
/// `number[]`'s `push(9)` appended through the freeze and answered
/// the new length; the any-side doors had been closed one commit
/// earlier and this lane was left for last because of ownership:
///
/// At call time the raw slot value is BORROWED — the array's stake is
/// taken after the call ([`take_pushed_stake`]: a borrow's inc, or an
/// owned temp's own reference moved into the slot). On the throw edge
/// that does not run, so the arg's own stake (or the fresh Str the
/// Substr-coerce lane minted) would leak; parking it on `throw_live`
/// for the call's duration hands it to the throw path's drain instead.
///
/// Returns the (possibly-realloc'd) array; the caller loads the new
/// length from it exactly as before.
// operands of one kernel call plus their ownership facts
#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_grow_kernel_checked(
    ctx: &mut LowerCtx<'_>,
    fid: crate::ssa::FuncId,
    recv: Operand,
    slot_arg: Operand,
    arg_eid: ExprId,
    val: Operand,
    elem_ty: Type,
    val_owned_from_substr: bool,
    ret_ty: Type,
) -> ValueId {
    if fid == ctx.intrinsics.arr_push
        && !elem_ty.is_refcounted()
        && !val_owned_from_substr
        && let Operand::Value(arr) = recv
    {
        return emit_push_opened(ctx, arr, slot_arg, arg_eid, val, ret_ty);
    }
    let token = if val_owned_from_substr {
        Some(ctx.push_throw_temp(val, elem_ty))
    } else {
        ctx.park_owned_temp(arg_eid, &val)
    };
    let new_arr = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(fid, vec![recv, slot_arg]),
        ret_ty,
        None,
    );
    ctx.emit_throw_check(None);
    ctx.unpark_owned_temp(token);
    take_pushed_stake(ctx, arg_eid, val, elem_ty, val_owned_from_substr);
    new_arr
}

/// The array's stake in a value a push lane just stored as raw bits.
/// A borrow shape takes a +1. An owned temp (the predicate
/// `release_owned_temp` drops by) moves its own +1 into the slot, the
/// way the coerce lanes' fresh value does: an inc plus a release of
/// the temp was a net zero. On the kernel's throw edge nothing was
/// stored and this does not run; the temp is still parked on
/// `throw_live` and the throw path releases it.
pub(crate) fn take_pushed_stake(
    ctx: &mut LowerCtx<'_>,
    arg_eid: ExprId,
    val: Operand,
    elem_ty: Type,
    val_owned_from_substr: bool,
) {
    if elem_ty.is_refcounted()
        && !val_owned_from_substr
        && ctx.owned_temp_ty(arg_eid, &val).is_none()
    {
        ctx.emit_rc_inc(val);
    }
}

/// A scalar push opened in place: one that lands in spare capacity of
/// an extensible array is a slot store and a length bump, with no
/// call. A lock bit or a full buffer (a deque head to fold back
/// included) takes the out-of-line kernel and its throw check. The
/// array cell never moves, so `arr` is the result on both edges.
///
/// Opening it here rather than only in codegen lets the loop passes
/// see the length word as an ordinary load and store, so a queue
/// loop's length stays in a register across the push.
fn emit_push_opened(
    ctx: &mut LowerCtx<'_>,
    arr: ValueId,
    slot_arg: Operand,
    arg_eid: ExprId,
    val: Operand,
    ret_ty: Type,
) -> ValueId {
    let recv = Operand::Value(arr);
    // the header bits that refuse a new index (§10.1.8.1) or a length
    // write (a non-writable `length`)
    let lock_bits = u64::from(
        torajs_rc::FLAG_FROZEN
            | torajs_rc::FLAG_SEALED
            | torajs_rc::FLAG_NON_EXTENSIBLE
            | torajs_rc::FLAG_ARR_LENGTH_RO,
    ) << 48;
    let load = |ctx: &mut LowerCtx<'_>, ty: Type, off: u64| {
        Operand::Value(
            ctx.f
                .append_inst(ctx.cur_block, InstKind::Load(ty, recv, off), ty, None),
        )
    };
    let hdr = load(ctx, Type::I64, 0);
    let masked = ctx.bin(
        BinOp::And,
        hdr,
        Operand::ConstI64(lock_bits as i64),
        Type::I64,
    );
    let locked = ctx.cmp(IPred::Ne, masked, Operand::ConstI64(0));
    let room_blk = ctx.f.add_block();
    let fast_blk = ctx.f.add_block();
    let slow_blk = ctx.f.add_block();
    let join_blk = ctx.f.add_block();
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: locked,
            then_blk: slow_blk,
            else_blk: room_blk,
        },
    );

    // cap in the low half of the packed word, the deque head in the high
    ctx.cur_block = room_blk;
    let packed = load(ctx, Type::I64, ARR_CAP_HEAD_OFF);
    let head = ctx.bin(BinOp::LShr, packed, Operand::ConstI64(32), Type::I64);
    let cap = ctx.bin(
        BinOp::And,
        packed,
        Operand::ConstI64(0xffff_ffff),
        Type::I64,
    );
    let len = load(ctx, Type::I64, ARR_LEN_OFF);
    let phys = ctx.bin(BinOp::Add, len, head, Type::I64);
    let full = ctx.cmp(IPred::Uge, phys, cap);
    ctx.f.set_term(
        room_blk,
        Terminator::CondBr {
            cond: full,
            then_blk: slow_blk,
            else_blk: fast_blk,
        },
    );

    ctx.cur_block = fast_blk;
    let data = load(ctx, Type::Ptr, ARR_DATA_PTR_OFF);
    let off = ctx.bin(BinOp::Shl, phys, Operand::ConstI64(3), Type::I64);
    ctx.f
        .append_void(fast_blk, InstKind::StoreDyn(slot_arg, data, off));
    let new_len = ctx.bin(BinOp::Add, len, Operand::ConstI64(1), Type::I64);
    ctx.f
        .append_void(fast_blk, InstKind::Store(new_len, recv, ARR_LEN_OFF));
    ctx.f.set_term(fast_blk, Terminator::Br(join_blk));

    ctx.cur_block = slow_blk;
    let cold: FuncId = ctx.fn_table[PUSH_COLD];
    let token = ctx.park_owned_temp(arg_eid, &val);
    ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(cold, vec![recv, slot_arg]),
        ret_ty,
        None,
    );
    ctx.emit_throw_check(None);
    ctx.unpark_owned_temp(token);
    ctx.f.set_term(ctx.cur_block, Terminator::Br(join_blk));
    ctx.cur_block = join_blk;
    arr
}
