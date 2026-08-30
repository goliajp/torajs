//! Loop-frame + per-method body + step+produce for the M6.2
//! `xs.{map,filter,reduce,reduceRight,forEach}` lowering. Pulled out of
//! [`super::ssa_lower_call_arr_ho`] only to keep each sibling file ≤ 500
//! hard — semantically these stages are part of the same M6.2 carve-out.
//! `LoopFrame` carries i_slot / header_blk / after_blk across the
//! begin → body → end stages without threading 4 ids through every
//! signature. Two child modules carry the other two questions
//! (file-size splits): [`dispatch`] — how one callback invocation is
//! made; [`methods`] — what `map` / `filter` / `reduce` do with the
//! answer.

use crate::ssa::{
    BinOp as SsaBinOp, BlockId, FuncId, IPred, InstKind, Operand, Terminator, Type, ValueId,
};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx, PreReserveState};

mod dispatch;
mod methods;
use dispatch::{cb_args, emit_do_call};
pub(crate) use dispatch::{emit_argv_face_call, emit_undef_any_box};
use methods::{emit_filter, emit_map, emit_map_hole_block, emit_reduce};

/// Per-loop SSA state shared by `begin_loop` / `emit_per_method_body` /
/// `end_loop_and_produce`. Copy so callers don't borrow-check ping-pong.
#[derive(Copy, Clone)]
pub(crate) struct LoopFrame {
    pub(crate) i_slot: ValueId,
    pub(crate) header_blk: BlockId,
    /// Where the cursor advances. A block of its own so the §23.1.3
    /// hole gate has somewhere to jump that skips the visit without
    /// duplicating the step (`end_loop_and_produce` fills it).
    pub(crate) step_blk: BlockId,
    pub(crate) after_blk: BlockId,
    /// The preheader's answer to "can any index of the source be
    /// absent?" — read once, before the loop, so the body's hole gate
    /// is a register test on a loop-invariant value.
    pub(crate) sparse: Option<ValueId>,
    /// S7-b — the product's pre-reserved append state, when this loop
    /// can take it. `Some` puts the per-element append inline (a slot
    /// store plus an add); `None` leaves it on the `arr_push_unchecked`
    /// call. `begin_loop` decides, `end_loop_and_produce` settles the
    /// length word it left stale.
    pub(crate) dst_fast: Option<PreReserveState>,
}

/// Set up i_slot + init_i, fast-path `arr_reserve(dst, len)` for
/// map/filter, Br to header, emit header `i {< len | > -1}` CondBr →
/// body | after. Leaves `ctx.cur_block` at the body block ready for
/// per-method work. Returns the frame so the body / end helpers can pick
/// up i_slot / header_blk / after_blk without re-threading.
///
/// S132 — reduceRight walks last → first (spec §22.1.3.22): init cursor
/// to `len - 1`, header `i > -1`, body `i = i - 1`. 1-arg
/// reduce/reduceRight: seed already consumed → cursor starts one step in
/// (i = 1 / len - 2).
pub(crate) fn begin_loop(
    ctx: &mut LowerCtx<'_>,
    method: &str,
    src_arr: ValueId,
    dst_slot: Option<ValueId>,
    dst_arr_ty: Type,
    src_elem_ty: Type,
    reduce_no_init: bool,
) -> LoopFrame {
    let header_blk = ctx.f.add_block();
    let body_blk = ctx.f.add_block();
    let step_blk = ctx.f.add_block();
    let after_blk = ctx.f.add_block();
    let i_slot = ctx.alloca(Type::I64, Some("__iter_i"));
    // Only a boxed-element source can hold an interior hole, and only
    // it pays the gate — see `ssa_lower_arr_hole_gate`'s module doc for
    // the measurement that decided this.
    let sparse = (src_elem_ty == Type::Any).then(|| ctx.emit_hof_sparse_probe(src_arr));
    let len = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(src_arr), ARR_LEN_OFF),
        Type::I64,
        None,
    );
    let init_i: Operand = if method == "reduceRight" {
        let seed_step = if reduce_no_init { 2 } else { 1 };
        let i_init = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::BinOp(
                SsaBinOp::Sub,
                Operand::Value(len),
                Operand::ConstI64(seed_step),
            ),
            Type::I64,
            None,
        );
        Operand::Value(i_init)
    } else if reduce_no_init {
        Operand::ConstI64(1)
    } else {
        Operand::ConstI64(0)
    };
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(init_i, Operand::Value(i_slot), 0),
    );
    // M6.2 fast-path — reserve dst capacity = src.length up front so the
    // per-element push doesn't have to grow. filter worst case allocates
    // the full src length; shrinking to actual count would save memory
    // but add a second pass — deferred.
    let mut dst_fast = None;
    if let Some(slot) = dst_slot {
        let cur_dst = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(dst_arr_ty, Operand::Value(slot), 0),
            dst_arr_ty,
            None,
        );
        let reserved = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.arr_reserve,
                vec![Operand::Value(cur_dst), Operand::Value(len)],
            ),
            dst_arr_ty,
            None,
        );
        // B1 — reserve never moves the cell; write-back retired.
        //
        // S7-b — the reservation is exactly the proof the inline append
        // wants, so take it: no per-element cross-archive `bl`, and the
        // running length lives in a register instead of the cell's own
        // length word. `ssa_lower_arr_prereserve` carries the shape.
        //
        // The one loop that cannot have it is the `map` whose product is
        // boxed: that is the only shape `emit_map_hole_block` exists for,
        // and marking a hole goes through `arr_mark_last_hole`, which
        // reads the cell's length — the very word this shape leaves
        // stale until the exit. Same predicate as that block's own gate.
        let dst_can_hole = method == "map"
            && matches!(dst_arr_ty, Type::Arr(id) if ctx.arr_layouts[id.0 as usize] == Type::Any);
        if !dst_can_hole {
            dst_fast = Some(ctx.emit_prereserved_state(reserved));
        }
    }
    ctx.f.set_term(ctx.cur_block, Terminator::Br(header_blk));
    ctx.cur_block = header_blk;
    let i_now = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(i_slot), 0),
        Type::I64,
        None,
    );
    let cmp = if method == "reduceRight" {
        ctx.f.append_inst(
            ctx.cur_block,
            InstKind::ICmp(IPred::Sgt, Operand::Value(i_now), Operand::ConstI64(-1)),
            Type::Bool,
            None,
        )
    } else {
        ctx.f.append_inst(
            ctx.cur_block,
            InstKind::ICmp(IPred::Slt, Operand::Value(i_now), Operand::Value(len)),
            Type::Bool,
            None,
        )
    };
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(cmp),
            then_blk: body_blk,
            else_blk: after_blk,
        },
    );
    ctx.cur_block = body_blk;
    LoopFrame {
        i_slot,
        header_blk,
        step_blk,
        after_blk,
        sparse,
        dst_fast,
    }
}

/// Emit body work: load i, head-aware byte offset → elem load, per-method
/// dispatch through `emit_do_call`. Devirts to direct call when `known_fid`
/// is set. Sig-mismatched arg widths (an f64 parameter reading an i64
/// element) used to be coerced inline here; that is one direction of the
/// shared argument contract, applied by the call lanes themselves.
#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_per_method_body(
    ctx: &mut LowerCtx<'_>,
    frame: LoopFrame,
    method: &str,
    src_arr: ValueId,
    dst_slot: Option<ValueId>,
    acc_slot: Option<ValueId>,
    dst_arr_ty: Type,
    acc_ty: Type,
    elem_ty: Type,
    known_fid: Option<FuncId>,
    fn_val: &Operand,
    fn_ty: Type,
    this_arg: Option<&Operand>,
    argv_face: bool,
) {
    let i_now2 = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(frame.i_slot), 0),
        Type::I64,
        None,
    );
    // §23.1.3 — these methods gate the visit on HasProperty, so a
    // hole is skipped and the callback never sees it.
    //
    // `map` alone cannot just step past one: its product has the
    // source's length, so the skipped index has to arrive in the
    // destination as a hole of its own (§23.1.3.20 step 6.c leaves it
    // uncreated) — push the undefined slot and mark it, exactly as the
    // array-literal elision lowering does. That needs a destination
    // whose slots can SAY hole, which is the same requirement the
    // delete side has: only a boxed-element product can. A `map` whose
    // callback answers a number builds a `number[]`, and marking a
    // hole in one is the element-kind change the runtime refuses — so
    // that shape keeps today's answer (the callback runs on the hole)
    // rather than a shorter product or a corrupt slot. Recorded
    // boundary; it lifts when a typed product can express absence,
    // which is the same day the typed `delete` does.
    let map_hole_blk = if method == "map" {
        emit_map_hole_block(ctx, dst_slot, dst_arr_ty, frame.step_blk)
    } else {
        Some(frame.step_blk)
    };
    if let (Some(sparse), Some(skip_blk)) = (frame.sparse, map_hole_blk) {
        ctx.emit_hof_present_gate(sparse, src_arr, i_now2, skip_blk);
    }
    // T-13.5: head-aware offset for map/filter/reduce src walk.
    // RFC 20260707 chunk 625 — an Any elem reads through the
    // kind-aware borrowed-box helper instead of a raw LoadDyn: a
    // typed block behind the static Arr<Any> view keeps raw slots,
    // which the raw load misread (same borrow contract, so the cb /
    // push consumers below are unchanged).
    let elem = if elem_ty == Type::Any {
        ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.arr_get_any_boxed,
                vec![Operand::Value(src_arr), Operand::Value(i_now2)],
            ),
            Type::Any,
            None,
        )
    } else {
        let (off_base, off) = ctx.emit_arr_slot_byte_offset(
            Operand::Value(src_arr),
            Operand::Value(i_now2),
            3,
            false,
        );
        ctx.f.append_inst(
            ctx.cur_block,
            InstKind::LoadDyn(elem_ty, off_base, off),
            elem_ty,
            None,
        )
    };
    // Argv-face callbacks take the FULL spec list — their sig's
    // params are the synthetic argv head, not positional slots, so
    // the declared-arity truncation must not apply (the boxed pack
    // wants «kValue, k, O» whole).
    let cb_arity = if argv_face {
        3
    } else {
        ctx.sig_param_tys(fn_ty).map_or(1, |p| p.len())
    };
    match method {
        "map" => emit_map(
            ctx,
            dst_slot,
            dst_arr_ty,
            frame.dst_fast,
            elem,
            known_fid,
            fn_val,
            fn_ty,
            this_arg,
            i_now2,
            src_arr,
            cb_arity,
            argv_face,
        ),
        "filter" => emit_filter(
            ctx,
            dst_slot,
            dst_arr_ty,
            frame.dst_fast,
            elem,
            elem_ty,
            known_fid,
            fn_val,
            fn_ty,
            this_arg,
            i_now2,
            src_arr,
            cb_arity,
            argv_face,
        ),
        "reduce" | "reduceRight" => emit_reduce(
            ctx, acc_slot, acc_ty, elem, known_fid, fn_val, fn_ty, i_now2, src_arr, cb_arity,
            argv_face,
        ),
        "forEach" => {
            // §23.1.3.15 step 5.c — Call(cb, thisArg, «kValue, k, O»).
            let _ = emit_do_call(
                ctx,
                known_fid,
                fn_val,
                fn_ty,
                cb_args(this_arg, elem, i_now2, src_arr, cb_arity),
                usize::from(this_arg.is_some()),
                3,
                argv_face,
            );
        }
        _ => unreachable!(),
    }
}

/// Step (`i = i ± 1`, Br back to header) + after-block produce
/// (`map`/`filter` → dst load; `reduce`/`reduceRight` → acc load;
/// `forEach` → ConstI64(0)). S132 — reduceRight decrements; others
/// increment.
pub(crate) fn end_loop_and_produce(
    ctx: &mut LowerCtx<'_>,
    frame: LoopFrame,
    method: &str,
    dst_slot: Option<ValueId>,
    acc_slot: Option<ValueId>,
    dst_arr_ty: Type,
    acc_ty: Type,
) -> Operand {
    let cur = ctx.cur_block;
    ctx.f.set_term(cur, Terminator::Br(frame.step_blk));
    ctx.cur_block = frame.step_blk;
    let i_then = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(frame.i_slot), 0),
        Type::I64,
        None,
    );
    let step_op = if method == "reduceRight" {
        SsaBinOp::Sub
    } else {
        SsaBinOp::Add
    };
    let i_next = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::BinOp(step_op, Operand::Value(i_then), Operand::ConstI64(1)),
        Type::I64,
        None,
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(i_next), Operand::Value(frame.i_slot), 0),
    );
    ctx.f
        .set_term(ctx.cur_block, Terminator::Br(frame.header_blk));
    ctx.cur_block = frame.after_blk;
    match method {
        "map" | "filter" => {
            // S7-b — the running length has been in a register since the
            // preheader; settle the cell's word before anyone reads it.
            if let Some(state) = frame.dst_fast {
                ctx.emit_prereserved_len_writeback(state);
            }
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(dst_arr_ty, Operand::Value(dst_slot.unwrap()), 0),
                dst_arr_ty,
                None,
            );
            Operand::Value(v)
        }
        "reduce" | "reduceRight" => {
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(acc_ty, Operand::Value(acc_slot.unwrap()), 0),
                acc_ty,
                None,
            );
            Operand::Value(v)
        }
        "forEach" => Operand::ConstI64(0),
        _ => unreachable!(),
    }
}
