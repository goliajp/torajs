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
use crate::ssa_lower_arr_hole_gate::TypedGate;

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
    /// Rotation 652 — a boxed-element source reads each element in
    /// the present gate (dense: the borrowed slot box; exotic: the
    /// accessor-aware owned read) into this slot; the body loads it.
    pub(crate) elem_slot: Option<ValueId>,
    /// The exotic read's owned box, and whether this step made one —
    /// the step block releases it after the visit.
    pub(crate) owned_slot: Option<ValueId>,
    pub(crate) owned_flag: Option<ValueId>,
    /// S7-b — the product's pre-reserved append state: `Some` exactly
    /// when the method builds a product (`map` / `filter`). The append
    /// is inline (a slot store plus an add); `end_loop_and_produce`
    /// settles the length word it left stale.
    pub(crate) dst_fast: Option<PreReserveState>,
    /// What a TYPED source checks before each visit (a boxed source
    /// runs its own gate). The caller sets it; `begin_loop` leaves it
    /// off.
    pub(crate) gate: TypedGate,
    /// This is the copy of a TYPED source's loop the preheader picks
    /// once the header says an index can be absent. The caller sets
    /// it; only here is `map`'s hole arm behind the link seam.
    pub(crate) holey: bool,
}

/// Set up i_slot + init_i, fast-path `arr_reserve(dst, len)` for
/// map/filter, Br to header, emit header `i {< len | > -1}` CondBr →
/// body | after. Leaves `ctx.cur_block` at the body block ready for
/// per-method work. Returns the frame so the body / end helpers can pick
/// up i_slot / header_blk / after_blk without re-threading.
///
/// S132 — reduceRight walks last → first (spec §22.1.3.22): init cursor
/// to `len - 1`, header `i > -1`, body `i = i - 1`. 1-arg
/// reduce/reduceRight: the seed was the first present element
/// (`seed_start`) and the cursor starts one step past it.
pub(crate) fn begin_loop(
    ctx: &mut LowerCtx<'_>,
    method: &str,
    src_arr: ValueId,
    dst_slot: Option<ValueId>,
    dst_arr_ty: Type,
    src_elem_ty: Type,
    seed_start: Option<ValueId>,
) -> LoopFrame {
    let header_blk = ctx.f.add_block();
    let body_blk = ctx.f.add_block();
    let step_blk = ctx.f.add_block();
    let after_blk = ctx.f.add_block();
    let i_slot = ctx.alloca(Type::I64, Some("__iter_i"));
    // Only a boxed-element source can hold an interior hole, and only
    // it pays the gate — see `ssa_lower_arr_hole_gate`'s module doc for
    // the measurement that decided this. The gate probes the header
    // per step (rotation 652) and reads the element into these slots.
    let boxed_src = src_elem_ty == Type::Any;
    let elem_slot = boxed_src.then(|| ctx.alloca(Type::Any, Some("__hof_elem")));
    let owned_slot = boxed_src.then(|| ctx.alloca(Type::Any, Some("__hof_owned")));
    let owned_flag = boxed_src.then(|| {
        // Cleared here as well as at each gate entry: a `map` whose
        // product cannot hole skips the gate entirely, and the step
        // block still reads this flag.
        let f = ctx.alloca(Type::Bool, Some("__hof_owned_flag"));
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(Operand::ConstBool(false), Operand::Value(f), 0),
        );
        f
    });
    let len = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(src_arr), ARR_LEN_OFF),
        Type::I64,
        None,
    );
    let init_i: Operand = if let Some(k) = seed_start {
        // The seed was the first present element; visiting resumes
        // one step past it.
        let step = if method == "reduceRight" { -1 } else { 1 };
        let i_init = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::BinOp(SsaBinOp::Add, Operand::Value(k), Operand::ConstI64(step)),
            Type::I64,
            None,
        );
        Operand::Value(i_init)
    } else if method == "reduceRight" {
        let i_init = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::BinOp(SsaBinOp::Sub, Operand::Value(len), Operand::ConstI64(1)),
            Type::I64,
            None,
        );
        Operand::Value(i_init)
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
        // The destination is a fresh array this lowering owns and no
        // user code can name until `map` / `filter` answers it, so the
        // length word may stay in a register until then (a `map` hole
        // mark settles it first) — see `PreReserveState::defer_len`.
        dst_fast = Some(ctx.emit_prereserved_state(reserved, true));
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
        elem_slot,
        owned_slot,
        owned_flag,
        dst_fast,
        gate: TypedGate::None,
        holey: false,
    }
}

fn map_dst(frame: LoopFrame) -> PreReserveState {
    frame.dst_fast.expect("begin_loop reserves every product")
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
    //
    // The mark sits behind a link seam where the arm is reached only
    // after the source's header said an index can be absent: the holey
    // typed copy and a boxed source's exotic arm. The dense typed copy
    // never read such a bit — its gate, `Len` or `Present`, is a guess
    // from the program text, and a shortened length alone takes it to
    // the arm — so it marks through the writer itself, as a boxed
    // source's dense arm does.
    let map_hole_blk = if method == "map" {
        let seam = frame.holey || frame.elem_slot.is_some();
        emit_map_hole_block(ctx, dst_arr_ty, map_dst(frame), frame.step_blk, seam)
    } else {
        Some(frame.step_blk)
    };
    // A boxed-element source reads its element inside the gate
    // (rotation 652): the dense arm keeps the borrowed slot box, the
    // exotic arm takes the accessor-aware owned read, and both land in
    // `elem_slot`. A `map` whose product cannot hole keeps today's
    // answer (`emit_map_hole_block` answered None) and reads plainly.
    let gated = match (
        frame.elem_slot,
        frame.owned_slot,
        frame.owned_flag,
        map_hole_blk,
    ) {
        (Some(es), Some(os), Some(of), Some(skip_blk)) => {
            let dense_skip = if method == "map" {
                emit_map_hole_block(ctx, dst_arr_ty, map_dst(frame), frame.step_blk, false)
                    .expect("the gated map has a hole arm")
            } else {
                skip_blk
            };
            ctx.emit_hof_present_gate(src_arr, i_now2, dense_skip, skip_blk, es, os, of);
            Some(es)
        }
        // A `map` whose product cannot hole still reads through the
        // gate — an accessor index runs its getter, a hole a prototype
        // key fills reads that key — but its "absent" exit visits the
        // index with undefined instead of skipping it (the recorded
        // boundary above: the product has no way to say hole).
        (Some(es), Some(os), Some(of), None) => {
            let absent_blk = ctx.f.add_block();
            ctx.emit_hof_present_gate(src_arr, i_now2, absent_blk, absent_blk, es, os, of);
            let work_blk = ctx.cur_block;
            ctx.cur_block = absent_blk;
            let undef = dispatch::emit_undef_any_box(ctx);
            ctx.f.append_void(
                absent_blk,
                InstKind::Store(Operand::Value(undef), Operand::Value(es), 0),
            );
            ctx.f.set_term(absent_blk, Terminator::Br(work_blk));
            ctx.cur_block = work_blk;
            Some(es)
        }
        // A typed source loses an index to a shorter length, or — in
        // the loop copy a holey array runs — to a hole. `map` into a
        // typed product has no way to say hole and keeps the recorded
        // boundary above.
        (None, None, None, Some(skip_blk)) if frame.gate != TypedGate::None => {
            ctx.emit_hof_typed_gate(frame.gate, src_arr, i_now2, skip_blk);
            None
        }
        _ => None,
    };
    // T-13.5: head-aware offset for map/filter/reduce src walk.
    // RFC 20260707 chunk 625 — an Any elem reads through the
    // kind-aware borrowed-box helper instead of a raw LoadDyn: a
    // typed block behind the static Arr<Any> view keeps raw slots,
    // which the raw load misread (same borrow contract, so the cb /
    // push consumers below are unchanged).
    let elem = if let Some(es) = gated {
        ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(Type::Any, Operand::Value(es), 0),
            Type::Any,
            None,
        )
    } else if elem_ty == Type::Any {
        crate::ssa_lower_anyv_inline::arr_get_any_borrowed(
            ctx,
            Operand::Value(src_arr),
            Operand::Value(i_now2),
        )
    } else {
        let (off_base, off) = ctx.emit_arr_slot_byte_offset(
            Operand::Value(src_arr),
            Operand::Value(i_now2),
            8,
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
            dst_arr_ty,
            map_dst(frame),
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
            map_dst(frame),
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
            ctx, acc_slot, acc_ty, elem, this_arg, known_fid, fn_val, fn_ty, i_now2, src_arr,
            cb_arity, argv_face,
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
    // The exotic arm's owned element read is spent here, after the
    // visit — the body's consumers keep their borrow contract.
    if let (Some(os), Some(of)) = (frame.owned_slot, frame.owned_flag) {
        ctx.emit_hof_release_owned(os, of);
    }
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
            // The running length has been in a register since the
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
