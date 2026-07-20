//! Per-method body emitters (`map` / `filter` / `reduce`) for the
//! M6.2 higher-order loop — moved verbatim from the parent module
//! (file-size split, rotation 161; the parent had drifted to 506).
//! Child-module placement reaches the parent's private `emit_do_call`
//! / `cb_args` / `emit_undef_any_box` with zero visibility changes.

use super::{cb_args, emit_do_call, emit_undef_any_box};
use crate::ssa::{FuncId, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::LowerCtx;

#[allow(clippy::too_many_arguments)]
pub(super) fn emit_map(
    ctx: &mut LowerCtx<'_>,
    dst_slot: Option<ValueId>,
    dst_arr_ty: Type,
    elem: ValueId,
    known_fid: Option<FuncId>,
    fn_val: &Operand,
    fn_ty: Type,
    sig_params: &[Type],
    this_arg: Option<&Operand>,
) {
    // RC-1 (RFC 20260706-test262-bug-corpus) — a Void-ret callback
    // returns `undefined`: emit the call for side effects, push a
    // boxed ANY_UNDEF per element (dst is Arr<Any> — the dispatch
    // entry maps a Void callback ret to an Any dst elem).
    let mapped_arg = if ctx.callback_ret_ty(fn_ty) == Some(Type::Void) {
        let _ = emit_do_call(
            ctx,
            sig_params,
            known_fid,
            fn_val,
            fn_ty,
            cb_args(this_arg, elem),
            usize::from(this_arg.is_some()),
        );
        Operand::Value(emit_undef_any_box(ctx))
    } else {
        let mapped = emit_do_call(
            ctx,
            sig_params,
            known_fid,
            fn_val,
            fn_ty,
            cb_args(this_arg, elem),
            usize::from(this_arg.is_some()),
        );
        ctx.raw_slot_arg(Operand::Value(mapped))
    };
    // M6.2 fast-path — dst was reserve'd above the loop, so the unchecked
    // push elides the per-call capacity check.
    let cur_dst = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(dst_arr_ty, Operand::Value(dst_slot.unwrap()), 0),
        dst_arr_ty,
        None,
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.arr_push_unchecked,
            vec![Operand::Value(cur_dst), mapped_arg],
        ),
    );
}

#[allow(clippy::too_many_arguments)]
pub(super) fn emit_filter(
    ctx: &mut LowerCtx<'_>,
    dst_slot: Option<ValueId>,
    dst_arr_ty: Type,
    elem: ValueId,
    known_fid: Option<FuncId>,
    fn_val: &Operand,
    fn_ty: Type,
    sig_params: &[Type],
    this_arg: Option<&Operand>,
) {
    // RC-1 — Void-ret predicate: ToBoolean(undefined) = false, so no
    // element is ever kept. Emit the call for side effects only.
    if ctx.callback_ret_ty(fn_ty) == Some(Type::Void) {
        let _ = emit_do_call(
            ctx,
            sig_params,
            known_fid,
            fn_val,
            fn_ty,
            cb_args(this_arg, elem),
            usize::from(this_arg.is_some()),
        );
        return;
    }
    let keep = emit_do_call(
        ctx,
        sig_params,
        known_fid,
        fn_val,
        fn_ty,
        cb_args(this_arg, elem),
        usize::from(this_arg.is_some()),
    );
    let push_blk = ctx.f.add_block();
    let next_blk = ctx.f.add_block();
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(keep),
            then_blk: push_blk,
            else_blk: next_blk,
        },
    );
    ctx.cur_block = push_blk;
    let cur_dst = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(dst_arr_ty, Operand::Value(dst_slot.unwrap()), 0),
        dst_arr_ty,
        None,
    );
    let elem_arg = ctx.raw_slot_arg(Operand::Value(elem));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.arr_push_unchecked,
            vec![Operand::Value(cur_dst), elem_arg],
        ),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(next_blk));
    ctx.cur_block = next_blk;
}

#[allow(clippy::too_many_arguments)]
pub(super) fn emit_reduce(
    ctx: &mut LowerCtx<'_>,
    acc_slot: Option<ValueId>,
    acc_ty: Type,
    elem: ValueId,
    known_fid: Option<FuncId>,
    fn_val: &Operand,
    fn_ty: Type,
    sig_params: &[Type],
) {
    let acc_now = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(acc_ty, Operand::Value(acc_slot.unwrap()), 0),
        acc_ty,
        None,
    );
    // RC-1 — Void-ret callback: the new accumulator is `undefined`
    // (boxed ANY_UNDEF; the dispatch entry maps a Void callback ret
    // to an Any acc slot).
    let new_acc = if ctx.callback_ret_ty(fn_ty) == Some(Type::Void) {
        let _ = emit_do_call(
            ctx,
            sig_params,
            known_fid,
            fn_val,
            fn_ty,
            vec![Operand::Value(acc_now), Operand::Value(elem)],
            0,
        );
        emit_undef_any_box(ctx)
    } else {
        emit_do_call(
            ctx,
            sig_params,
            known_fid,
            fn_val,
            fn_ty,
            vec![Operand::Value(acc_now), Operand::Value(elem)],
            0,
        )
    };
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(
            Operand::Value(new_acc),
            Operand::Value(acc_slot.unwrap()),
            0,
        ),
    );
}
