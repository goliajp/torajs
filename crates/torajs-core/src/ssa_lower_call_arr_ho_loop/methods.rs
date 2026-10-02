//! Per-method body emitters (`map` / `filter` / `reduce`) for the
//! M6.2 higher-order loop — moved verbatim from the parent module
//! (file-size split, rotation 161; the parent had drifted to 506).
//! The call side (`emit_do_call` / `cb_args` / `emit_undef_any_box`)
//! moved on to the [`super::dispatch`] sibling in rotation 534 — same
//! parent, so reaching it stays a `use`, not a visibility widening.

use super::dispatch::{cb_args, emit_do_call, emit_undef_any_box};
use crate::ssa::{BlockId, FuncId, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::{LowerCtx, PreReserveState};

#[allow(clippy::too_many_arguments)]
pub(super) fn emit_map(
    ctx: &mut LowerCtx<'_>,
    dst_slot: Option<ValueId>,
    dst_arr_ty: Type,
    dst_fast: Option<PreReserveState>,
    elem: ValueId,
    known_fid: Option<FuncId>,
    fn_val: &Operand,
    fn_ty: Type,
    this_arg: Option<&Operand>,
    i_now: ValueId,
    src_arr: ValueId,
    cb_arity: usize,
    argv_face: bool,
) {
    // RC-1 (RFC 20260706-test262-bug-corpus) — a Void-ret callback
    // returns `undefined`: emit the call for side effects, push a
    // boxed ANY_UNDEF per element (dst is Arr<Any> — the dispatch
    // entry maps a Void callback ret to an Any dst elem).
    let mapped_val = if ctx.callback_ret_ty(fn_ty) == Some(Type::Void) {
        let _ = emit_do_call(
            ctx,
            known_fid,
            fn_val,
            fn_ty,
            cb_args(this_arg, elem, i_now, src_arr, cb_arity),
            usize::from(this_arg.is_some()),
            3,
            argv_face,
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
            3,
            argv_face,
        );
        // RFC 20260726-array-elem-width knife 1 — the dst elem width is
        // the analysis class's answer, which can be wider than what this
        // callback returns (a fractional value reaching the product from
        // any other site widens the class). Convert rather than store the
        // integer's bits into an f64 slot.
        let mapped = Operand::Value(mapped);

        if ctx.operand_ty(&mapped) == Type::I64
            && matches!(dst_arr_ty, Type::Arr(id) if ctx.arr_layouts[id.0 as usize] == Type::F64)
        {
            ctx.coerce_to_f64(mapped)
        } else if ctx.operand_ty(&mapped) == Type::Substr {
            // The callback's view answer is copied out as an owned
            // string and the view released as the push used to consume
            // it (rotation 468; the product is typed `Arr<Str>`).
            let owned = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.substr_to_owned, vec![mapped]),
                Type::Str,
                None,
            );
            ctx.emit_drop_value(mapped, Type::Substr);
            Operand::Value(owned)
        } else {
            mapped
        }
    };
    // S7-b — `begin_loop` reserved the product, so the append is a slot
    // store and an add, with the running length in a register.
    if let Some(state) = dst_fast {
        let _ = ctx.emit_prereserved_push(state, mapped_val);
        return;
    }
    // The call lane — a boxed product, which `map` can mark a hole in
    // and so must keep its length word live. M6.2: dst was reserve'd
    // above the loop, so the call is the unchecked one.
    let mapped_arg = ctx.raw_slot_arg(mapped_val);
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
    dst_fast: Option<PreReserveState>,
    elem: ValueId,
    elem_ty: Type,
    known_fid: Option<FuncId>,
    fn_val: &Operand,
    fn_ty: Type,
    this_arg: Option<&Operand>,
    i_now: ValueId,
    src_arr: ValueId,
    cb_arity: usize,
    argv_face: bool,
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
            3,
            argv_face,
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
        3,
        argv_face,
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
    let keep_b = ctx.coerce_to_bool(raw);
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
    // S7-b — the product's cell is only needed by the call lane.
    let cur_dst = dst_fast.is_none().then(|| {
        ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(dst_arr_ty, Operand::Value(dst_slot.unwrap()), 0),
            dst_arr_ty,
            None,
        )
    });
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
    // A kept VIEW is copied out as an owned string (a view does not
    // leave its split block — rotation 468; the product is `Arr<Str>`).
    let elem_val = if elem_ty == Type::Substr {
        let owned = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.substr_to_owned, vec![Operand::Value(elem)]),
            Type::Str,
            None,
        );
        Operand::Value(owned)
    } else {
        ctx.emit_owned_result_inc(Operand::Value(elem), elem_ty);
        Operand::Value(elem)
    };
    if let Some(state) = dst_fast {
        let _ = ctx.emit_prereserved_push(state, elem_val);
    } else {
        let elem_arg = ctx.raw_slot_arg(elem_val);
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.arr_push_unchecked,
                vec![
                    Operand::Value(cur_dst.expect("call lane loaded the cell")),
                    elem_arg,
                ],
            ),
        );
    }
    ctx.f.set_term(ctx.cur_block, Terminator::Br(next_blk));
    ctx.cur_block = next_blk;
}

#[allow(clippy::too_many_arguments)]
pub(super) fn emit_reduce(
    ctx: &mut LowerCtx<'_>,
    acc_slot: Option<ValueId>,
    acc_ty: Type,
    elem: ValueId,
    this_arg: Option<&Operand>,
    known_fid: Option<FuncId>,
    fn_val: &Operand,
    fn_ty: Type,
    i_now: ValueId,
    src_arr: ValueId,
    cb_arity: usize,
    argv_face: bool,
) {
    let acc_now = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(acc_ty, Operand::Value(acc_slot.unwrap()), 0),
        acc_ty,
        None,
    );
    // Spec §23.1.3.24 — the reducer's trailing (index, sourceArray)
    // slots, appended only when its sig declares them (arity 3 / 4).
    // RFC 20260905 residue 1 — a promoted reducer's leading `__this`
    // (undefined, §23.1.3.24 step 8.b.iii) rides ahead of the
    // accumulator and is skipped when aligning against the sig.
    let mut reduce_args: Vec<Operand> = this_arg.cloned().into_iter().collect();
    reduce_args.push(Operand::Value(acc_now));
    reduce_args.push(Operand::Value(elem));
    let sig_skip = usize::from(this_arg.is_some());
    // An argv-face reducer packs the WHOLE spec list — its sig's
    // params are the synthetic argv head, not positional slots.
    if cb_arity >= 3 || argv_face {
        reduce_args.push(Operand::Value(i_now));
    }
    if cb_arity >= 4 || argv_face {
        reduce_args.push(Operand::Value(src_arr));
    }
    // RC-1 — Void-ret callback: the new accumulator is `undefined`
    // (boxed ANY_UNDEF; the dispatch entry maps a Void callback ret
    // to an Any acc slot).
    let cb_ret = ctx.callback_ret_ty(fn_ty);
    let new_acc = if cb_ret == Some(Type::Void) {
        let _ = emit_do_call(
            ctx,
            known_fid,
            fn_val,
            fn_ty,
            reduce_args,
            sig_skip,
            4,
            argv_face,
        );
        emit_undef_any_box(ctx)
    } else {
        let v = emit_do_call(
            ctx,
            known_fid,
            fn_val,
            fn_ty,
            reduce_args,
            sig_skip,
            4,
            argv_face,
        );
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

/// The block `map` jumps to when §23.1.3.20's HasProperty gate says the
/// source index is absent: the product has the source's length, so the
/// skipped index has to arrive as a hole of its own (step 6.c leaves it
/// uncreated) — push the undefined slot and mark it, exactly as the
/// array-literal elision lowering does.
///
/// `None` = do not gate this `map` at all. Marking a hole needs a
/// destination whose slots can SAY hole, which is the same requirement
/// the `delete` side has: only a boxed-element product can. A callback
/// answering a number builds a `number[]`, and marking a hole in one is
/// the element-kind change the runtime refuses — so that shape keeps
/// today's answer (the callback runs on the hole) rather than a shorter
/// product or a corrupt slot. Recorded boundary; it lifts the day a
/// typed product can express absence, which is the day the typed
/// `delete` does.
/// The element type of a `map` product that can hold a hole: a boxed
/// one, or a typed one whose slots have a hole word (F64's sentinel, a
/// boolean 2, a NULL pointer). An integer product cannot say absent.
pub(super) fn map_product_holes(ctx: &LowerCtx<'_>, dst_arr_ty: Type) -> Option<Type> {
    let Type::Arr(id) = dst_arr_ty else {
        return None;
    };
    let elem = ctx.arr_layouts[id.0 as usize];
    (matches!(elem, Type::Any | Type::F64 | Type::Bool)
        || crate::ssa_lower_index_undef::has_undef_spelling(elem))
    .then_some(elem)
}

/// `seam` picks the mark's entry: the link-judged seam
/// (`arr_map_hole_slow`) for an arm the loop reaches only after the
/// source's header said it can have an absent index — the exotic,
/// slot-hole and sparse-tail bits, each with one writer entry the
/// judgment reads — or the writer itself (`arr_mark_last_hole`) for an
/// arm a loop that read no such bit can reach (a shortened length).
pub(super) fn emit_map_hole_block(
    ctx: &mut LowerCtx<'_>,
    dst_slot: Option<ValueId>,
    dst_arr_ty: Type,
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
    let cur_dst = ctx.f.append_inst(
        hole_blk,
        InstKind::Load(
            dst_arr_ty,
            Operand::Value(dst_slot.expect("map has a dst")),
            0,
        ),
        dst_arr_ty,
        None,
    );
    let slot_arg = ctx.raw_slot_arg(undef);
    ctx.f.append_void(
        hole_blk,
        InstKind::Call(
            ctx.intrinsics.arr_push_unchecked,
            vec![Operand::Value(cur_dst), slot_arg],
        ),
    );
    let mark = if seam {
        ctx.intrinsics.arr_map_hole_slow
    } else {
        ctx.intrinsics.arr_mark_last_hole
    };
    ctx.f.append_void(
        hole_blk,
        InstKind::Call(mark, vec![Operand::Value(cur_dst)]),
    );
    ctx.f.set_term(hole_blk, Terminator::Br(step_blk));
    ctx.cur_block = saved;
    Some(hole_blk)
}
