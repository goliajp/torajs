//! Predicate-iter loop engine (`emit_predicate_iter` +
//! `emit_body_and_step`) for the six short-circuit predicate methods
//! — moved verbatim from the parent module (file-size split,
//! rotation 364: the argv-face knife needed room the parent's 495
//! didn't have). Child-module placement reaches the parent's
//! private items with zero visibility changes.

use crate::ssa::{BinOp as SsaBinOp, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx};
use crate::ssa_lower_arr_hole_gate::TypedGate;

/// Emit the full predicate-iter loop: i_slot init + forward/reverse cmp +
/// body (load elem → call predicate → branch hit/next) + i++ step + after-
/// block result load. Returns the final loaded result operand.
// one loop's operands and lane flags
#[allow(clippy::too_many_arguments)]
pub(super) fn emit_predicate_iter(
    ctx: &mut LowerCtx<'_>,
    method: &str,
    src_arr: ValueId,
    elem_ty: Type,
    fn_val: Operand,
    fn_ty: Type,
    this_arg: Option<&Operand>,
    result_ty: Type,
    result_slot: ValueId,
    argv_face: bool,
    typed_gate: TypedGate,
) -> Operand {
    let is_reverse = method == "findLastIndex" || method == "findLast";
    let i_slot = ctx.alloca(Type::I64, Some("__pred_i"));
    let len = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(src_arr), ARR_LEN_OFF),
        Type::I64,
        None,
    );
    // Forward: i = 0; loop while i < len; step i + 1.
    // Reverse (findLastIndex): i = len - 1; loop while i >= 0; step i - 1.
    let i_init: Operand = if is_reverse {
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::BinOp(SsaBinOp::Sub, Operand::Value(len), Operand::ConstI64(1)),
            Type::I64,
            None,
        );
        Operand::Value(v)
    } else {
        Operand::ConstI64(0)
    };
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(i_init, Operand::Value(i_slot), 0),
    );
    let header_blk = ctx.f.add_block();
    let body_blk = ctx.f.add_block();
    let after_blk = ctx.f.add_block();
    // §23.1.3.{29,6} — `some` and `every` gate each visit on
    // HasProperty; the other four Get every index, holes included.
    // Only a boxed-element source pays the §23.1.3 HasProperty gate —
    // see `ssa_lower_arr_hole_gate`'s module doc. Rotation 652: the
    // gate probes per step and reads the element itself (dense: the
    // borrowed slot box; exotic: the accessor-aware owned read), so
    // the loop carries the three slots it writes.
    let gate = ((method == "some" || method == "every") && elem_ty == Type::Any).then(|| {
        (
            ctx.alloca(Type::Any, Some("__hof_elem")),
            ctx.alloca(Type::Any, Some("__hof_owned")),
            ctx.alloca(Type::Bool, Some("__hof_owned_flag")),
        )
    });
    ctx.f.set_term(ctx.cur_block, Terminator::Br(header_blk));
    // header
    ctx.cur_block = header_blk;
    let i_now = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(i_slot), 0),
        Type::I64,
        None,
    );
    let cmp = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(
            if is_reverse { IPred::Sge } else { IPred::Slt },
            Operand::Value(i_now),
            if is_reverse {
                Operand::ConstI64(0)
            } else {
                Operand::Value(len)
            },
        ),
        Type::Bool,
        None,
    );
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(cmp),
            then_blk: body_blk,
            else_blk: after_blk,
        },
    );
    emit_body_and_step(
        ctx,
        method,
        is_reverse,
        body_blk,
        header_blk,
        after_blk,
        i_slot,
        src_arr,
        elem_ty,
        fn_val,
        fn_ty,
        this_arg,
        gate,
        result_slot,
        argv_face,
        typed_gate,
    );
    ctx.cur_block = after_blk;
    let r = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(result_ty, Operand::Value(result_slot), 0),
        result_ty,
        None,
    );
    Operand::Value(r)
}

/// The element as the predicate's first argument. An element holding
/// its type's undefined — the F64 sentinel or a pointer type's
/// undefined cell, a hole the find family reads with [[Get]]
/// (§23.1.3.9) or a stored undefined — is `undefined` to an `any`
/// parameter, not the NaN or the heap box the plain box makes.
fn pred_elem_arg(
    ctx: &mut LowerCtx<'_>,
    elem: ValueId,
    elem_ty: Type,
    fn_ty: Type,
    argv_face: bool,
) -> Operand {
    let sig_first = ctx.sig_param_tys(fn_ty).and_then(|p| p.first().copied());
    if argv_face || sig_first != Some(Type::Any) {
        Operand::Value(elem)
    } else if elem_ty == Type::F64 {
        ctx.box_f64_or_undef(Operand::Value(elem))
    } else if crate::ssa_lower_index_undef::has_undef_spelling(elem_ty) {
        ctx.box_heap_slot_or_undef(Operand::Value(elem))
    } else {
        Operand::Value(elem)
    }
}

/// The element the predicate sees: the gate's slot when the source is
/// boxed and gated (rotation 652), else the kind-aware borrowed-box
/// helper for an Any element (RFC 20260707 chunk 625 — a typed block
/// behind the static `Arr<Any>` view misread under a raw LoadDyn),
/// else the head-aware raw slot load (T-13.5).
fn read_elem(
    ctx: &mut LowerCtx<'_>,
    gated: Option<ValueId>,
    elem_ty: Type,
    src_arr: ValueId,
    i_now: ValueId,
) -> ValueId {
    if let Some(es) = gated {
        return ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(Type::Any, Operand::Value(es), 0),
            Type::Any,
            None,
        );
    }
    if elem_ty == Type::Any {
        return crate::ssa_lower_anyv_inline::arr_get_any_borrowed(
            ctx,
            Operand::Value(src_arr),
            Operand::Value(i_now),
        );
    }
    let (off_base, off) =
        ctx.emit_arr_slot_byte_offset(Operand::Value(src_arr), Operand::Value(i_now), 8, false);
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::LoadDyn(elem_ty, off_base, off),
        elem_ty,
        None,
    );
    if !crate::ssa_lower_index_undef::has_undef_spelling(elem_ty) {
        return v;
    }
    // A hole of a pointer cell is a NULL slot, which [[Get]] answers
    // with `undefined` — and the find family visits holes (§23.1.3.9):
    // the predicate gets the type's own undefined, not a NULL.
    let slot = ctx.alloca_in_entry(elem_ty, Some("__pred_elem"));
    let merge = ctx.f.add_block();
    crate::ssa_lower_index_undef::hole_slot_is_undefined(ctx, v, elem_ty, slot, merge);
    let cur = ctx.cur_block;
    ctx.f.append_void(
        cur,
        InstKind::Store(Operand::Value(v), Operand::Value(slot), 0),
    );
    ctx.f.set_term(cur, Terminator::Br(merge));
    ctx.cur_block = merge;
    ctx.f.append_inst(
        merge,
        InstKind::Load(elem_ty, Operand::Value(slot), 0),
        elem_ty,
        None,
    )
}

/// Emit the loop body (load elem + call predicate) + hit-vs-next branch +
/// i++ step. Caller has already placed the `cmp` cond-br into body_blk;
/// after this fn returns body's slots are wired and cur_block points at
/// after_blk's predecessor (no, actually we leave cur_block on whatever the
/// final ctx.f.set_term left it — the caller resets to after_blk before
/// loading the result).
// one loop's blocks, slots and lane flags
#[allow(clippy::too_many_arguments)]
fn emit_body_and_step(
    ctx: &mut LowerCtx<'_>,
    method: &str,
    is_reverse: bool,
    body_blk: crate::ssa::BlockId,
    header_blk: crate::ssa::BlockId,
    after_blk: crate::ssa::BlockId,
    i_slot: ValueId,
    src_arr: ValueId,
    elem_ty: Type,
    fn_val: Operand,
    fn_ty: Type,
    this_arg: Option<&Operand>,
    gate: Option<(ValueId, ValueId, ValueId)>,
    result_slot: ValueId,
    argv_face: bool,
    typed_gate: TypedGate,
) {
    // body — load elem, run predicate
    ctx.cur_block = body_blk;
    let i_now2 = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(i_slot), 0),
        Type::I64,
        None,
    );
    // A hole is skipped and the predicate never sees it
    // (`[1, <hole>, 3].some(v => v === undefined)` is false). The step
    // block is minted here so the gate has somewhere to jump.
    let next_blk = ctx.f.add_block();
    if let Some((es, os, of)) = gate {
        ctx.emit_hof_present_gate(src_arr, i_now2, next_blk, next_blk, es, os, of);
    } else if typed_gate != TypedGate::None && (method == "some" || method == "every") {
        // A typed source its predicate may shorten (`hof_len_shrink`),
        // or — in the loop copy a holey array runs — one with holes.
        ctx.emit_hof_typed_gate(typed_gate, src_arr, i_now2, next_blk);
    }
    // The find family has no HasProperty step (§23.1.3.9): an index
    // the predicate cut off is visited, with Get's `undefined`.
    let elem = if typed_gate == TypedGate::Len
        && gate.is_none()
        && crate::ssa_lower_index_undef::has_undef_spelling(elem_ty)
        && !(method == "some" || method == "every")
    {
        crate::ssa_lower_index_undef::emit_get_past_length(ctx, src_arr, i_now2, elem_ty, |ctx| {
            read_elem(ctx, None, elem_ty, src_arr, i_now2)
        })
    } else {
        read_elem(ctx, gate.map(|(es, _, _)| es), elem_ty, src_arr, i_now2)
    };
    // RC-1 (RFC 20260706-test262-bug-corpus) — a Void-ret predicate
    // returns `undefined`; ToBoolean folds every hit test to false
    // (ES §23.1.3.{8-11,30}). Emit the call for its side effects
    // only — consuming the void call's value is the SIGTRAP lane.
    let cb_ret_void = ctx.callback_ret_ty(fn_ty) == Some(Type::Void);
    // Spec §23.1.3.{8-11,30} — the predicate's trailing (index,
    // sourceArray) slots, appended only when its sig declares them;
    // materialize_call_args aligns the reprs. An argv-face predicate
    // takes the FULL spec list — its sig's params are the synthetic
    // argv head, not positional slots (rotation 364, the ho-loop
    // family's posture).
    let cb_arity = if argv_face {
        3
    } else {
        ctx.sig_param_tys(fn_ty).map_or(1, |p| p.len())
    };
    // Rotation 261 — a promoted receiver-first predicate takes the
    // boxed thisArg as its leading `__this` argv entry, which is not
    // in the sig; `sig_skip` starts positional alignment after it
    // (knife-4 protocol, cb_args mirror).
    let mut pred_args = Vec::with_capacity(4);
    if let Some(t) = this_arg {
        pred_args.push(*t);
    }
    let elem_arg = pred_elem_arg(ctx, elem, elem_ty, fn_ty, argv_face);
    pred_args.push(elem_arg);
    if cb_arity >= 2 {
        pred_args.push(Operand::Value(i_now2));
    }
    if cb_arity >= 3 {
        pred_args.push(Operand::Value(src_arr));
    }
    let sig_skip = usize::from(this_arg.is_some());
    let pred_op: Operand = if cb_ret_void {
        // Rotation 364 — the argv-face route boxes the spec triple
        // into a stack argv and rides the dual-entry adapter; the
        // direct call would land the positional args in the reshaped
        // sig's argv-pointer slot (the r363 silent no-op lesson).
        if argv_face {
            let _ = crate::ssa_lower_call_arr_ho_loop::emit_argv_face_call(
                ctx, &fn_val, fn_ty, pred_args, sig_skip, 3,
            );
        } else {
            let _ = ctx.call_fn_value(fn_val, fn_ty, pred_args, sig_skip, 3);
            // 550-01 — §23.1.3.{8-11,30} step 5.c ReturnIfAbrupt: a
            // throwing predicate ends the walk. This lane had no check
            // on its direct-call paths (`a(i).find(x => x > boom())`
            // caught once in three; the argv face's variadic conv
            // carries its own).
            ctx.emit_throw_check(None);
        }
        Operand::ConstBool(false)
    } else {
        let pred_v = if argv_face {
            crate::ssa_lower_call_arr_ho_loop::emit_argv_face_call(
                ctx, &fn_val, fn_ty, pred_args, sig_skip, 3,
            )
        } else {
            let v = ctx.call_fn_value(fn_val, fn_ty, pred_args, sig_skip, 3);
            ctx.emit_throw_check(None);
            v
        };
        // rotation 284 — the predicate return folds through ToBoolean
        // (ES §23.1.3.{8-11,30}): a non-Bool cb ret (1/0 counters,
        // strings, boxed values) coerces here; coerce_to_bool is a
        // no-op on Bool so the exact-sig path is unchanged. An owned
        // heap ret is released after the truthiness read — the test
        // is its only consumer.
        let raw = Operand::Value(pred_v);
        let ret_ty = ctx.operand_ty(&raw);

        let b = ctx.coerce_to_bool(raw);
        if ret_ty.is_refcounted() {
            ctx.emit_drop_value(raw, ret_ty);
        }
        b
    };
    // The exotic arm's owned element read is spent once the predicate
    // has seen it (a store inside the predicate took its own stake).
    if let Some((_, os, of)) = gate {
        ctx.emit_hof_release_owned(os, of);
    }
    // some + findIndex break on `pred == true`; every breaks on
    // `pred == false`.
    let break_cond = if method == "every" {
        let inv = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::ICmp(IPred::Eq, pred_op, Operand::ConstBool(false)),
            Type::Bool,
            None,
        );
        Operand::Value(inv)
    } else {
        pred_op
    };
    let hit_blk = ctx.f.add_block();
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: break_cond,
            then_blk: hit_blk,
            else_blk: next_blk,
        },
    );
    // hit: write the appropriate result and exit. For find / findLast the
    // elem is the result; refcounted elements get rc_inc'd so the caller's
    // binding owns a ref independent of the source array's slot.
    ctx.cur_block = hit_blk;
    let hit_val = hit_value(ctx, method, elem, elem_ty, i_now2);
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(hit_val, Operand::Value(result_slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(after_blk));
    // next: i++ and loop
    ctx.cur_block = next_blk;
    let i_then = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(i_slot), 0),
        Type::I64,
        None,
    );
    let i_next = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::BinOp(
            if is_reverse {
                SsaBinOp::Sub
            } else {
                SsaBinOp::Add
            },
            Operand::Value(i_then),
            Operand::ConstI64(1),
        ),
        Type::I64,
        None,
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(i_next), Operand::Value(i_slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(header_blk));
}

/// The value a hit writes to the result slot, per method: the index,
/// a bool, or the element itself — a refcounted element taking its
/// own +1, a bool boxed, and a substring VIEW copied out as an owned
/// string (a view does not leave its split block — rotation 468).
fn hit_value(
    ctx: &mut LowerCtx<'_>,
    method: &str,
    elem: ValueId,
    elem_ty: Type,
    i_now2: ValueId,
) -> Operand {
    match method {
        "findIndex" | "findLastIndex" => Operand::Value(i_now2),
        "some" => Operand::ConstBool(true),
        "every" => Operand::ConstBool(false),
        "find" | "findLast" => {
            // A found VIEW leaves its split block as an owned copy
            // (rotation 468); the result slot is typed Str for it.
            if elem_ty == Type::Substr {
                let owned = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Call(ctx.intrinsics.substr_to_owned, vec![Operand::Value(elem)]),
                    Type::Str,
                    None,
                );
                Operand::Value(owned)
            } else {
                if elem_ty.is_refcounted() {
                    ctx.emit_rc_inc(Operand::Value(elem));
                }
                if elem_ty == Type::Bool {
                    ctx.box_to_any(Operand::Value(elem))
                } else {
                    Operand::Value(elem)
                }
            }
        }
        _ => unreachable!(),
    }
}
