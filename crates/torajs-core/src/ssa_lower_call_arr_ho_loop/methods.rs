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
    this_arg: Option<&Operand>,
    i_now: ValueId,
    src_arr: ValueId,
    cb_arity: usize,
) {
    // RC-1 (RFC 20260706-test262-bug-corpus) — a Void-ret callback
    // returns `undefined`: emit the call for side effects, push a
    // boxed ANY_UNDEF per element (dst is Arr<Any> — the dispatch
    // entry maps a Void callback ret to an Any dst elem).
    let mapped_arg = if ctx.callback_ret_ty(fn_ty) == Some(Type::Void) {
        let _ = emit_do_call(
            ctx,
            known_fid,
            fn_val,
            fn_ty,
            cb_args(this_arg, elem, i_now, src_arr, cb_arity),
            usize::from(this_arg.is_some()),
        );
        Operand::Value(emit_undef_any_box(ctx))
    } else {
        let mapped = emit_do_call(
            ctx,
            known_fid,
            fn_val,
            fn_ty,
            cb_args(this_arg, elem, i_now, src_arr, cb_arity),
            usize::from(this_arg.is_some()),
        );
        // RFC 20260726-array-elem-width knife 1 — the dst elem width is
        // the analysis class's answer, which can be wider than what this
        // callback returns (a fractional value reaching the product from
        // any other site widens the class). Convert rather than store the
        // integer's bits into an f64 slot.
        let mapped = Operand::Value(mapped);
        let mapped = if ctx.operand_ty(&mapped) == Type::I64
            && matches!(dst_arr_ty, Type::Arr(id) if ctx.arr_layouts[id.0 as usize] == Type::F64)
        {
            ctx.coerce_to_f64(mapped)
        } else {
            mapped
        };
        ctx.raw_slot_arg(mapped)
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
    elem_ty: Type,
    known_fid: Option<FuncId>,
    fn_val: &Operand,
    fn_ty: Type,
    this_arg: Option<&Operand>,
    i_now: ValueId,
    src_arr: ValueId,
    cb_arity: usize,
) {
    // RC-1 — Void-ret predicate: ToBoolean(undefined) = false, so no
    // element is ever kept. Emit the call for side effects only.
    if ctx.callback_ret_ty(fn_ty) == Some(Type::Void) {
        let _ = emit_do_call(
            ctx,
            known_fid,
            fn_val,
            fn_ty,
            cb_args(this_arg, elem, i_now, src_arr, cb_arity),
            usize::from(this_arg.is_some()),
        );
        return;
    }
    let keep = emit_do_call(
        ctx,
        known_fid,
        fn_val,
        fn_ty,
        cb_args(this_arg, elem, i_now, src_arr, cb_arity),
        usize::from(this_arg.is_some()),
    );
    // The keep test folds through ToBoolean (ES §23.1.3.7 step
    // 8.c.ii): a non-Bool cb ret (Any box, numbers, strings)
    // coerces; coerce_to_bool is a no-op on Bool so the exact-sig
    // path is unchanged. An owned heap ret is released after the
    // truthiness read — the test is its only consumer. (The
    // predicate-family mirror landed rotation 284; filter's kernel
    // is this trio loop, which kept the raw CondBr.)
    let raw = Operand::Value(keep);
    let ret_ty = ctx.operand_ty(&raw);
    let keep_b = ctx.coerce_to_bool(raw.clone());
    if ret_ty.is_refcounted() {
        ctx.emit_drop_value(raw, ret_ty);
    }
    let push_blk = ctx.f.add_block();
    let next_blk = ctx.f.add_block();
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: keep_b,
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
    // P0 chained-Array<Any>-temp bug fix (2026-07-23): the outer walker
    // loads `elem` via a borrowed read — `arr_get_any_boxed` for
    // Type::Any or a raw LoadDyn for typed heap kinds. Pushing that
    // borrow directly into dst leaves dst's slot non-owning; when the
    // src (typically a chain temp like `.map()`'s return) drops after
    // the filter, dst's payload dangles. `arr.map(n=>n.toString())
    // .filter(...)` inline chained: `"1-2-3"` decayed to `"1-2-1"`
    // (repro m4.ts); `.slice()` was fine because its runtime kernel
    // rc-incs. Emit an owned-result inc only for heap element kinds
    // (Copy scalars keep the raw copy) and only inside push_blk so
    // filtered-out elements don't inflate anyone's rc.
    ctx.emit_owned_result_inc(Operand::Value(elem), elem_ty);
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
    i_now: ValueId,
    src_arr: ValueId,
    cb_arity: usize,
) {
    let acc_now = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(acc_ty, Operand::Value(acc_slot.unwrap()), 0),
        acc_ty,
        None,
    );
    // Spec §23.1.3.24 — the reducer's trailing (index, sourceArray)
    // slots, appended only when its sig declares them (arity 3 / 4).
    let mut reduce_args = vec![Operand::Value(acc_now), Operand::Value(elem)];
    if cb_arity >= 3 {
        reduce_args.push(Operand::Value(i_now));
    }
    if cb_arity >= 4 {
        reduce_args.push(Operand::Value(src_arr));
    }
    // RC-1 — Void-ret callback: the new accumulator is `undefined`
    // (boxed ANY_UNDEF; the dispatch entry maps a Void callback ret
    // to an Any acc slot).
    let cb_ret = ctx.callback_ret_ty(fn_ty);
    let new_acc = if cb_ret == Some(Type::Void) {
        let _ = emit_do_call(ctx, known_fid, fn_val, fn_ty, reduce_args, 0);
        emit_undef_any_box(ctx)
    } else {
        let v = emit_do_call(ctx, known_fid, fn_val, fn_ty, reduce_args, 0);
        // rotation 285 — the hetero-seed acc lane (dispatch picked an
        // Any slot because the seed's type differs from the cb ret):
        // the typed ret boxes on the way in. box_to_any is rc-neutral
        // for every kind, so an owned heap ret transfers its stake
        // into the box; the post-store old-acc drop releases it as
        // Any on the next overwrite.
        if acc_ty == Type::Any && cb_ret.is_some_and(|r| r != Type::Any) {
            match ctx.box_to_any(Operand::Value(v)) {
                Operand::Value(b) => b,
                _ => unreachable!("box_to_any returns an SSA value"),
            }
        } else {
            v
        }
    };
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(
            Operand::Value(new_acc),
            Operand::Value(acc_slot.unwrap()),
            0,
        ),
    );
    // Rotation 185 — the overwritten accumulator's stake was never
    // released while the callback's return carries its own +1
    // (owned-result invariant: `return acc` retains at the return
    // root), so a pass-through callback grew the acc cell's rc by
    // one per iteration and a unique-per-iteration seed never freed
    // (`xs.reduce(cb, {t:i})` churn 25.8MB vs 6.4MB flat). Dropping
    // the OLD bits after the store balances both shapes: same-box
    // return pairs retain/drop, fresh return releases the retired
    // accumulator. Non-refcounted acc slots have no stake to settle.
    if acc_ty.is_refcounted() {
        ctx.emit_drop_value(Operand::Value(acc_now), acc_ty);
    }
}
