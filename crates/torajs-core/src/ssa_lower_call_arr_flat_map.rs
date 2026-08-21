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
use crate::ssa::{IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx};

mod walk;
use walk::{
    emit_close_and_load, emit_dynamic_push_and_close, emit_inner_walk, emit_scalar_push_and_close,
};

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
    let (this_arg, this_temp) = lower_promoted_this(ctx, args);
    let fn_ty = ctx.operand_ty(&fn_val);
    // Rotation 364 — an argv-face callback (body reads `arguments`
    // values) must not take the direct call: its reshaped sig leads
    // with the synthetic argv pointer. The loop body routes it
    // through the boxed variadic dispatch (`emit_argv_face_call`)
    // with the full §23.1.3.13 «kValue, k, O» triple. The declared
    // ret still drives `dst_shape` — the variadic conv unboxes the
    // result back to it, so the downstream scalar / inner-walk
    // split is unchanged.
    let argv_face = matches!(ctx.ast.get_expr(args[0]),
        Expr::Closure { fn_name, .. } if ctx.ast.closure_argv_fns.contains(fn_name))
        || matches!(ctx.ast.get_expr(args[0]),
            Expr::Ident(n) if ctx.ast.closure_argv_locals.contains(n));
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
    let elem = emit_src_elem_load(ctx, src_elem_ty, src_arr, i_now);
    // RFC 20260726-array-elem-width knife 9 — the element's width and
    // the callback parameter's width answer to two different classes,
    // so they can legitimately disagree: a named callback widened by
    // another call site still reads an i64-elem array here. That
    // conversion used to be spelled out on this side, in the one
    // direction the crash showed; it is one direction of the shared
    // argument contract, which `call_fn_value` now applies for every
    // lane. Spec §23.1.3 arity — (index, srcArray) slots are appended
    // per the callback's own declared arity (`materialize_call_args`
    // aligns the reprs, same as the ho-loop family's cb_args).
    // An argv-face callback takes the FULL spec list — its sig's
    // params are the synthetic argv head, not positional slots.
    let cb_arity = if argv_face {
        3
    } else {
        ctx.sig_param_tys(fn_ty).map_or(1, |p| p.len())
    };
    // A promoted receiver-first callback takes the boxed thisArg as
    // its leading `__this` argv entry, which is not in the sig —
    // `sig_skip` starts positional alignment after it (knife-4).
    let mut call_args: Vec<Operand> = Vec::with_capacity(4);
    if let Some(t) = &this_arg {
        call_args.push(t.clone());
    }
    call_args.push(Operand::Value(elem));
    if cb_arity >= 2 {
        call_args.push(Operand::Value(i_now));
    }
    if cb_arity >= 3 {
        call_args.push(Operand::Value(src_arr));
    }
    let sig_skip = usize::from(this_arg.is_some());
    let cb_ret = if argv_face {
        crate::ssa_lower_call_arr_ho_loop::emit_argv_face_call(ctx, &fn_val, fn_ty, call_args, 3)
    } else {
        ctx.call_fn_value(fn_val.clone(), fn_ty, call_args, sig_skip, 3)
    };

    let final_dst = if scalar_ret && cb_ret_ty == Type::Any {
        // An Any return is only decidable at runtime — ES §23.1.3.11
        // step 8's IsArray test branches between the spread walk and
        // the scalar push per element.
        emit_dynamic_push_and_close(ctx, cb_ret, i_slot, oh, oa, dst_slot, dst_arr_ty)
    } else if scalar_ret {
        // Scalar return — cb answered an owned scalar U directly (no
        // inner arr wrapping). Push it into dst as-is: the value
        // already carries the +1 the dst slot takes over (refcounted
        // types), or is an inline immediate (Number / Boolean). A
        // void callback has no result value — the pushed element is
        // the boxed-undefined sentinel.
        let push_val = scalar_push_value(ctx, cb_ret, cb_ret_ty);
        emit_scalar_push_and_close(ctx, push_val, i_slot, oh, oa, dst_slot, dst_arr_ty)
    } else {
        let inner_elem_ty = match cb_ret_ty {
            Type::Arr(id) => ctx.arr_layouts[id.0 as usize],
            _ => dst_elem_ty,
        };
        emit_inner_walk(
            ctx,
            cb_ret,
            dst_slot,
            dst_arr_ty,
            dst_elem_ty,
            inner_elem_ty,
        );
        emit_close_and_load(ctx, cb_ret, cb_ret_ty, i_slot, oh, oa, dst_slot, dst_arr_ty)
    };
    // RFC 20260705 chunk 552 — release owned-shape temps after the
    // loop consumed them (inline arrow's minted env in the cb slot,
    // Call/New-shaped receiver, the boxed thisArg's payload).
    ctx.release_owned_temp(args[0], &fn_val);
    ctx.release_owned_temp(obj, &recv_op);
    if let Some((t, op)) = this_temp {
        ctx.release_owned_temp(t, &op);
    }
    Some(Operand::Value(final_dst))
}

/// Rotation 286 — a promoted fn-expr callback (inline Closure in
/// `fnexpr_recv_fns`, or a knife-2 variable-routed binding in
/// `fnexpr_recv_locals`) takes the §23.1.3 thisArg (T) as its leading
/// `__this` arg: args[1] lowers into an Any box (undefined when
/// absent) — the arr_ho knife-4 protocol mirrored onto flatMap, same
/// as the predicate family (rotation 261). Non-promoted callbacks
/// answer `(None, None)`. Also lowers-and-drops the trailing args
/// (S319 — spec left-to-right side-effect order).
/// The element a scalar-returning callback contributes: its result
/// as-is (it already carries the +1 the dst slot takes over, or is an
/// immediate); the boxed-undefined sentinel for a void callback; and
/// for a view answer an owned copy, the view released as the push used
/// to consume it (rotation 468). Carved out of [`try_lower`] under the
/// 200-line function limit.
fn scalar_push_value(
    ctx: &mut LowerCtx<'_>,
    cb_ret: crate::ssa::ValueId,
    cb_ret_ty: Type,
) -> crate::ssa::ValueId {
    if cb_ret_ty == Type::Void {
        crate::ssa_lower_call_arr_ho_loop::emit_undef_any_box(ctx)
    } else if cb_ret_ty == Type::Substr {
        let owned = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.substr_to_owned, vec![Operand::Value(cb_ret)]),
            Type::Str,
            None,
        );
        ctx.emit_drop_value(Operand::Value(cb_ret), Type::Substr);
        owned
    } else {
        cb_ret
    }
}

fn lower_promoted_this(
    ctx: &mut LowerCtx<'_>,
    args: &[ExprId],
) -> (Option<Operand>, Option<(ExprId, Operand)>) {
    let promoted = matches!(ctx.ast.get_expr(args[0]),
        Expr::Closure { fn_name, .. } if ctx.ast.fnexpr_recv_fns.contains(fn_name))
        || matches!(ctx.ast.get_expr(args[0]),
            Expr::Ident(n) if ctx.ast.fnexpr_recv_locals.contains(n));
    let mut this_temp: Option<(ExprId, Operand)> = None;
    let this_arg: Option<Operand> = if promoted {
        if let Some(&t) = args.get(1) {
            let op = ctx.lower_expr(t);
            let boxed = ctx.box_to_any_from_expr(t, op.clone());
            this_temp = Some((t, op));
            Some(boxed)
        } else {
            Some(Operand::Value(
                crate::ssa_lower_call_arr_ho_loop::emit_undef_any_box(ctx),
            ))
        }
    } else {
        None
    };
    for &a in args.iter().skip(if promoted { 2 } else { 1 }) {
        let _ = ctx.lower_expr(a);
    }
    (this_arg, this_temp)
}

/// T-13.5: head-aware offset for the flatMap src walk. RFC 20260707
/// chunk 625 — an Any elem reads through the kind-aware borrowed-box
/// helper (typed-behind-Arr<Any> raw slots misread under a raw
/// LoadDyn).
fn emit_src_elem_load(
    ctx: &mut LowerCtx<'_>,
    src_elem_ty: Type,
    src_arr: crate::ssa::ValueId,
    i_now: crate::ssa::ValueId,
) -> crate::ssa::ValueId {
    if src_elem_ty == Type::Any {
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
    }
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
        Type::Arr(inner_arr_id) => {
            let inner = ctx.arr_layouts[inner_arr_id.0 as usize];
            // An inner `Arr<Substr>` (the callback answered a split
            // product) is copied out as owned strings — a view does
            // not leave its split block (rotation 468).
            if inner == Type::Substr {
                let dst_id =
                    crate::ssa_lower_interners::intern_arr_layout(&mut ctx.arr_layouts, Type::Str);
                (false, Type::Str, Type::Arr(dst_id))
            } else {
                (false, inner, cb_ret_ty)
            }
        }
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
            } else if cb_ret_ty == Type::Substr {
                // A scalar view answer is pushed as an owned copy
                // (rotation 468); the product is `Arr<Str>`.
                Type::Str
            } else {
                cb_ret_ty
            };
            let dst_id = crate::ssa_lower_interners::intern_arr_layout(&mut ctx.arr_layouts, elem);
            (true, elem, Type::Arr(dst_id))
        }
        other => panic!("flatMap callback must return an array or primitive scalar, got {other:?}"),
    }
}
