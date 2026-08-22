//! Predicate-iter loop engine (`emit_predicate_iter` +
//! `emit_body_and_step`) for the six short-circuit predicate methods
//! — moved verbatim from the parent module (file-size split,
//! rotation 364: the argv-face knife needed room the parent's 495
//! didn't have). Child-module placement reaches the parent's
//! private items with zero visibility changes.

use crate::ssa::{BinOp as SsaBinOp, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx};

/// Emit the full predicate-iter loop: i_slot init + forward/reverse cmp +
/// body (load elem → call predicate → branch hit/next) + i++ step + after-
/// block result load. Returns the final loaded result operand.
#[allow(clippy::too_many_arguments)]
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
        result_slot,
        argv_face,
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

/// Emit the loop body (load elem + call predicate) + hit-vs-next branch +
/// i++ step. Caller has already placed the `cmp` cond-br into body_blk;
/// after this fn returns body's slots are wired and cur_block points at
/// after_blk's predecessor (no, actually we leave cur_block on whatever the
/// final ctx.f.set_term left it — the caller resets to after_blk before
/// loading the result).
#[allow(clippy::too_many_arguments)]
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
    result_slot: ValueId,
    argv_face: bool,
) {
    // body — load elem, run predicate
    ctx.cur_block = body_blk;
    let i_now2 = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(i_slot), 0),
        Type::I64,
        None,
    );
    // §23.1.3.{29,6} — `some` and `every` gate the visit on
    // HasProperty, so a hole is skipped and the predicate never sees
    // it (`[1, <hole>, 3].some(v => v === undefined)` is false).
    // `find` / `findLast` / `findIndex` / `findLastIndex` are NOT in
    // that list: §23.1.3.9 Get's every index, holes included, so they
    // keep walking straight through. The step block is minted here so
    // the gate has somewhere to jump.
    let next_blk = ctx.f.add_block();
    if method == "some" || method == "every" {
        ctx.emit_hof_present_gate(src_arr, i_now2, next_blk);
    }
    // T-13.5: head-aware offset for some/every/findIndex.
    // RFC 20260707 chunk 625 — an Any elem reads through the
    // kind-aware borrowed-box helper (typed-behind-Arr<Any> raw
    // slots misread under a raw LoadDyn; same borrow contract).
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
        pred_args.push(t.clone());
    }
    pred_args.push(Operand::Value(elem));
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
                ctx, &fn_val, fn_ty, pred_args, 3,
            );
        } else {
            let _ = ctx.call_fn_value(fn_val, fn_ty, pred_args, sig_skip, 3);
        }
        Operand::ConstBool(false)
    } else {
        let pred_v = if argv_face {
            crate::ssa_lower_call_arr_ho_loop::emit_argv_face_call(
                ctx, &fn_val, fn_ty, pred_args, 3,
            )
        } else {
            ctx.call_fn_value(fn_val, fn_ty, pred_args, sig_skip, 3)
        };
        // rotation 284 — the predicate return folds through ToBoolean
        // (ES §23.1.3.{8-11,30}): a non-Bool cb ret (1/0 counters,
        // strings, boxed values) coerces here; coerce_to_bool is a
        // no-op on Bool so the exact-sig path is unchanged. An owned
        // heap ret is released after the truthiness read — the test
        // is its only consumer.
        let raw = Operand::Value(pred_v);
        let ret_ty = ctx.operand_ty(&raw);

        let b = ctx.coerce_to_bool(raw.clone());
        if ret_ty.is_refcounted() {
            ctx.emit_drop_value(raw, ret_ty);
        }
        b
    };
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
