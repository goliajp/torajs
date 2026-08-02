//! `<Array<T>>.flatMap(fn)` higher-order method pulled out of
//! [`crate::ssa_lower::lower_expr_inner`] `Expr::Call` dispatch as chunk-11
//! of the `Expr::Call` god-arm decomp (chunks 1-10 = Arr higher-order +
//! Map dispatch + Set dispatch + Arr.push + Number instance methods +
//! bare-name globals + Str regex methods + Number namespace + Array.from +
//! Arr predicate iter).
//!
//! Shape: outer loop walks the source array, per element calls `fn(elem)`
//! to get an inner Array<T>, inner loop walks that and pushes each element
//! into `dst`. Inner array's element is `rc_inc`'d before push (refcounted
//! types only) so the inner array's own drop balances correctly. dst is
//! `cap=0`; arr_push grows on demand.
//!
//! S319 — ES §23.1.3.11 spec sig is `flatMap(cb, thisArg)`; tora's
//! callbacks lack `this` binding so thisArg + any further trailing args are
//! silently dropped per S270 (check.rs already typecheck-drops args[1..]
//! for the Array callback family — this arm widens the ssa_lower gate
//! `args.len() == 1` to `>= 1` and lower-and-drops trailing for spec
//! left-to-right side-effect order).
//!
//! Returns `Some(result)` when callee matches `Expr::Member` with name
//! `flatMap` and args is non-empty; non-array receivers panic at SSA
//! lower-time (preserving the original block's behavior). `None` lets the
//! caller fall through to the next M2/M3/M4 arm below.

use crate::ast::{Expr, ExprId};
use crate::ssa::{BinOp as SsaBinOp, BlockId, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx};

/// Try to lower a `xs.flatMap(fn, ...)` call. Returns `Some` when
/// dispatched.
pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let (obj, name) = match ctx.ast.get_expr(callee) {
        Expr::Member { obj, name } => (*obj, name.clone()),
        _ => return None,
    };
    if name != "flatMap" || args.is_empty() {
        return None;
    }
    let recv_op = ctx.lower_expr(obj);
    let recv_ty = ctx.operand_ty(&recv_op);
    let Type::Arr(_) = recv_ty else {
        panic!("ssa-lower: flatMap on non-array receiver {recv_ty:?}");
    };
    let arr_ty = recv_ty;
    let src_arr = match recv_op {
        Operand::Value(v) => v,
        _ => unreachable!(),
    };
    let fn_val = ctx.lower_expr(args[0]);
    // S319 — lower-and-drop trailing args[1..] (thisArg + any further
    // trailing) for spec left-to-right side-effect order. tora callbacks
    // lack `this` binding so thisArg is functionally dropped; check.rs
    // S270 already typecheck-drops these args.
    for &a in args.iter().skip(1) {
        let _ = ctx.lower_expr(a);
    }
    let fn_ty = ctx.operand_ty(&fn_val);
    let cb_ret_ty = match fn_ty {
        Type::FnSig(s) | Type::Closure(s) => ctx.fn_sigs[s.0 as usize].1,
        _ => panic!("flatMap callback must be callable"),
    };
    let (scalar_ret, dst_elem_ty, dst_arr_ty) = dst_shape(ctx, eid, cb_ret_ty);

    // Allocate dst (cap=0; arr_push grows on demand). Chunk 625 —
    // an Any dst allocates the FLAG_ARR_ANY flavor: the walk pushes
    // NaN-box bits raw (any slots ARE box bits, so arr_push's 8B
    // store is layout-correct), but the block must self-describe as
    // Any for the kind-aware readers — the old typed alloc left an
    // unmarked block whose reads fell to the UNSET arm (undefined).
    let dst_alloc_fn = if dst_elem_ty == Type::Any {
        ctx.intrinsics.arr_alloc_any
    } else {
        ctx.intrinsics.arr_alloc
    };
    let dst_init = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(dst_alloc_fn, vec![Operand::ConstI64(0)]),
        dst_arr_ty,
        None,
    );
    let dst_slot = ctx.alloca(dst_arr_ty, Some("__fm_dst"));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(dst_init), Operand::Value(dst_slot), 0),
    );

    // Outer loop: i in 0..src.length.
    let oh = ctx.f.add_block();
    let ob = ctx.f.add_block();
    let oa = ctx.f.add_block();
    let i_slot = ctx.alloca(Type::I64, Some("__fm_i"));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::ConstI64(0), Operand::Value(i_slot), 0),
    );
    let src_len = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(src_arr), ARR_LEN_OFF),
        Type::I64,
        None,
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(oh));

    // oh: check i < src_len.
    ctx.cur_block = oh;
    let i_now = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(i_slot), 0),
        Type::I64,
        None,
    );
    let cmp = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Slt, Operand::Value(i_now), Operand::Value(src_len)),
        Type::Bool,
        None,
    );
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(cmp),
            then_blk: ob,
            else_blk: oa,
        },
    );

    // ob: load src[i], call closure(elem) → inner_arr.
    ctx.cur_block = ob;
    let src_elem_ty = ctx.arr_layouts[match arr_ty {
        Type::Arr(id) => id.0 as usize,
        _ => unreachable!(),
    }];
    // T-13.5: head-aware offset for flatMap src walk. RFC 20260707
    // chunk 625 — an Any elem reads through the kind-aware
    // borrowed-box helper (typed-behind-Arr<Any> raw slots misread
    // under a raw LoadDyn).
    let elem = if src_elem_ty == Type::Any {
        ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.arr_get_any_boxed,
                vec![Operand::Value(src_arr), Operand::Value(i_now)],
            ),
            Type::Any,
            None,
        )
    } else {
        let (off_base, off) =
            ctx.emit_arr_slot_byte_offset(Operand::Value(src_arr), Operand::Value(i_now), 3, false);
        ctx.f.append_inst(
            ctx.cur_block,
            InstKind::LoadDyn(src_elem_ty, off_base, off),
            src_elem_ty,
            None,
        )
    };
    // RFC 20260726-array-elem-width knife 9 — the element's width and
    // the callback parameter's width answer to two different classes,
    // so they can legitimately disagree: a named callback widened by
    // another call site still reads an i64-elem array here. That
    // conversion used to be spelled out on this side, in the one
    // direction the crash showed; it is one direction of the shared
    // argument contract, which `call_fn_value` now applies for every
    // lane.
    let cb_ret = ctx.call_fn_value(fn_val, fn_ty, vec![Operand::Value(elem)], 0);

    let final_dst = if scalar_ret {
        // Scalar return — cb answered an owned scalar U directly (no
        // inner arr wrapping). Push it into dst as-is: the value
        // already carries the +1 the dst slot takes over (refcounted
        // types), or is an inline immediate (Number / Boolean). A
        // void callback has no result value — the pushed element is
        // the boxed-undefined sentinel.
        let push_val = if cb_ret_ty == Type::Void {
            crate::ssa_lower_call_arr_ho_loop::emit_undef_any_box(ctx)
        } else {
            cb_ret
        };
        emit_scalar_push_and_close(ctx, push_val, i_slot, oh, oa, dst_slot, dst_arr_ty)
    } else {
        emit_inner_walk(ctx, cb_ret, dst_slot, dst_arr_ty, dst_elem_ty);
        emit_close_and_load(ctx, cb_ret, cb_ret_ty, i_slot, oh, oa, dst_slot, dst_arr_ty)
    };
    // RFC 20260705 chunk 552 — release owned-shape temps after the
    // loop consumed them (inline arrow's minted env in the cb slot,
    // Call/New-shaped receiver).
    ctx.release_owned_temp(args[0], &fn_val);
    ctx.release_owned_temp(obj, &recv_op);
    Some(Operand::Value(final_dst))
}

/// The destination's shape: whether the callback answers a scalar, and
/// the element / array types the product is built with.
///
/// Callback return can be either `Array<U>` (standard flat_map shape —
/// the inner walk pushes each element) or a scalar `U` per ES
/// §23.1.3.11 step 8.d (a non-Array result acts like `[U]` — single
/// push per outer iter, no inner walk). The scalar arm is gated by the
/// sibling `check_type_of_call_arr_flat_map_scalar` checker wedge
/// (admits `(T) => U` for primitive U).
fn dst_shape(ctx: &mut LowerCtx<'_>, eid: ExprId, cb_ret_ty: Type) -> (bool, Type, Type) {
    match cb_ret_ty {
        Type::Arr(inner_arr_id) => (false, ctx.arr_layouts[inner_arr_id.0 as usize], cb_ret_ty),
        // Value-less callback — every outer iteration pushes the
        // boxed-undefined sentinel (§23.1.3.11 step 8.d: a non-Array
        // result acts like `[U]`, and a void call's result is
        // `undefined`). The dst takes the Any flavor; the push value
        // is minted by the caller (`emit_undef_any_box`).
        Type::Void => {
            let dst_id =
                crate::ssa_lower_interners::intern_arr_layout(&mut ctx.arr_layouts, Type::Any);
            (true, Type::Any, Type::Arr(dst_id))
        }
        Type::I64 | Type::F64 | Type::Str | Type::Substr | Type::Bool | Type::Any => {
            // RFC 20260726-array-elem-width knife 8 — the callback's
            // return is only one source of the product's elements, so
            // it cannot decide their width alone; the class the
            // analysis keys by this call's origin can be wider (a
            // fractional value reaching the product from anywhere else
            // widens it). Same ask an array literal already makes, and
            // the same one knife 1 gave map's product.
            let elem = if cb_ret_ty == Type::I64
                && ctx
                    .num_f64_slots
                    .elem_is_f64(&crate::num_width::SlotKey::Anon(eid.0))
            {
                Type::F64
            } else {
                cb_ret_ty
            };
            let dst_id = crate::ssa_lower_interners::intern_arr_layout(&mut ctx.arr_layouts, elem);
            (true, elem, Type::Arr(dst_id))
        }
        other => panic!("flatMap callback must return an array or primitive scalar, got {other:?}"),
    }
}

/// Emit the inner-array walk (`j in 0..inner_arr.length` pushing each
/// element into `dst`). Caller must have `ctx.cur_block` pointing at the
/// outer body block (after the closure call). On exit, `ctx.cur_block`
/// points at the inner-loop after-block (the caller continues with
/// drop + outer i++).
fn emit_inner_walk(
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
fn emit_scalar_push_and_close(
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
fn emit_close_and_load(
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
