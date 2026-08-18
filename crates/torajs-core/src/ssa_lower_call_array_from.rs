//! `Array.from(iter, mapFn?)` namespace dispatch pulled out of
//! [`crate::ssa_lower::lower_expr_inner`] `Expr::Call` dispatch as
//! chunk-9 of the `Expr::Call` god-arm decomp (chunks 1-8 = Arr
//! higher-order + Map dispatch + Set dispatch + Arr.push + Number
//! instance methods + bare-name globals + Str regex methods + Number
//! namespace).
//!
//! Two phases per call:
//! 1. **src materialization** — `Array.from(s)` accepts three iter
//!    shapes per ES §23.1.2.1: string (`arr_from_string` runtime walk),
//!    typed `Array<T>` (shallow clone via `arr_slice`+per-element
//!    `rc_inc` for refcounted elems), and `Set` (delegates to
//!    `ssa_lower_arr_from_set::emit`). Other arg types panic at
//!    SSA lower-time.
//! 2. **optional mapFn loop** — `Array.from(iter, mapFn)` walks `src`,
//!    calls `mapFn(elem)` or `mapFn(elem, idx)` per arity, pushes the
//!    result into a `Array<R>` pre-sized to `src.length`. mapFn can be
//!    known FuncId (devirt) or any Type::Closure / Type::FnSig value.
//!
//! Returns `Some` when the callee shape matches `Array.from(...)` with
//! at least 1 arg; `None` otherwise so dispatch falls through to the
//! next arm.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx, intern_arr_layout};
use crate::ssa_lower_call_array_from_loop::{emit_map_loop, from_mapfn_promoted};

/// Try to lower `Array.from(...)`. Returns `Some` when dispatched.
pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let (ns_id, m_name) = match ctx.ast.get_expr(callee) {
        Expr::Member { obj, name } => (*obj, name.clone()),
        _ => return None,
    };
    if m_name != "from" && m_name != "fromAsync" {
        return None;
    }
    if !matches!(ctx.ast.get_expr(ns_id), Expr::Ident(n) if n == "Array") {
        return None;
    }
    if args.is_empty() {
        return None;
    }
    if m_name == "fromAsync" {
        // proposal-array-from-async §2.1.1 sync-source MVP — box the
        // items arg (and the mapfn when present) and hand them to
        // the dyn kernel (array-like step protocol + settled-promise
        // element unwrap; the mapped entry interleaves await /
        // mapfn / await per element). The mapped kernel also takes
        // the step-3.j.ii.6.a receiver: args[2] boxes through as the
        // thisArg (undefined box when absent). args[3..] (trailing)
        // eval-and-drop per the S275 posture.
        let arg_op = ctx.lower_expr(args[0]);
        let arg_ty = ctx.operand_ty(&arg_op);
        let boxed = ctx.box_to_any_from_expr(args[0], arg_op.clone());
        let mut call_args = vec![boxed];
        let mut cb_temp: Option<(Operand, Type)> = None;
        let mut recv_temp: Option<(Operand, Type)> = None;
        if args.len() >= 2 {
            let cb_op = ctx.lower_expr(args[1]);
            let cb_ty = ctx.operand_ty(&cb_op);
            call_args.push(ctx.box_to_any_from_expr(args[1], cb_op.clone()));
            cb_temp = Some((cb_op, cb_ty));
            let recv_boxed = if let Some(&ta) = args.get(2) {
                let ta_op = ctx.lower_expr(ta);
                let ta_ty = ctx.operand_ty(&ta_op);
                let b = ctx.box_to_any_from_expr(ta, ta_op.clone());
                recv_temp = Some((ta_op, ta_ty));
                b
            } else {
                let cur_block = ctx.cur_block;
                Operand::Value(ctx.f.append_inst(
                    cur_block,
                    InstKind::Call(
                        ctx.intrinsics.any_box,
                        vec![Operand::ConstI64(5), Operand::ConstI64(0)],
                    ),
                    Type::Any,
                    None,
                ))
            };
            call_args.push(recv_boxed);
        }
        for &a in args.iter().skip(3) {
            let _ = ctx.lower_expr(a);
        }
        let fid = if args.len() >= 2 {
            ctx.intrinsics.array_from_async_map_dyn
        } else {
            ctx.intrinsics.array_from_async_dyn
        };
        let cur_block = ctx.cur_block;
        let v = ctx.f.append_inst(
            cur_block,
            InstKind::Call(fid, call_args),
            Type::Promise,
            None,
        );
        // The boxes share; fresh owned temps still owe their stake
        // (the combinator dyn-entry ownership shape).
        if arg_ty.is_refcounted() && ctx.expr_is_fresh_owned(args[0]) {
            ctx.emit_drop_value(arg_op, arg_ty);
        }
        if let Some((cb_op, cb_ty)) = cb_temp
            && cb_ty.is_refcounted()
            && ctx.expr_is_fresh_owned(args[1])
        {
            ctx.emit_drop_value(cb_op, cb_ty);
        }
        if let Some((ta_op, ta_ty)) = recv_temp
            && ta_ty.is_refcounted()
            && ctx.expr_is_fresh_owned(args[2])
        {
            ctx.emit_drop_value(ta_op, ta_ty);
        }
        return Some(Operand::Value(v));
    }
    // B6 刀 3 — the kernel route (shared predicate with the checker:
    // non-fast-lane source / non-Function mapFn / explicit thisArg).
    // Box the three operands and hand them to
    // `__torajs_array_from_dyn` — the §23.1.2.1 any-tier walk with
    // the spec's exact «kValue, k» call shape, real thisArg binding
    // and per-index Get/map interleave.
    if args.len() >= 2
        && let (Some(src_ty), Some(fn_ty)) =
            (ctx.expr_types.get(&args[0]), ctx.expr_types.get(&args[1]))
        && crate::check_type_of_call_array_from::from_mapfn_routes_kernel(src_ty, fn_ty, args.len())
    {
        return Some(emit_kernel_route(ctx, args));
    }
    // S275 — widen `== 1 || == 2` to `>= 1`. The 2-arg path below handles
    // iter+mapFn; trailing args (thisArg + extras) are eval-and-dropped
    // at the end so side-effect exprs fire per ES §23.1.2.1 trailing-arg
    // ignore.
    let arg_op = ctx.lower_expr(args[0]);
    let arg_ty = ctx.operand_ty(&arg_op);
    let src_arr_op = materialize_src(ctx, arg_op, arg_ty, args[0]);
    if args.len() == 1 {
        return Some(src_arr_op);
    }
    // S275 — eval-and-drop trailing args so side-effect exprs fire per
    // ES §23.1.2.1 trailing-arg ignore. A plain callback doesn't bind
    // `this`, so args[2] (thisArg) is dropped too; a promoted fn-expr
    // callback's thisArg lowers inside the map loop instead (knife-4
    // mirror), so the drop walk starts after it.
    let promoted_skip = if from_mapfn_promoted(ctx, args[1]) {
        3
    } else {
        2
    };
    for &a in args.iter().skip(promoted_skip) {
        let _ = ctx.lower_expr(a);
    }
    Some(emit_map_loop(ctx, src_arr_op, args))
}

/// The B6 刀 3 kernel route: box (src, mapFn, thisArg) and call
/// `__torajs_array_from_dyn`. The kernel answers a raw fresh
/// `Array<Any>` pointer (NULL under a pending throw, which the check
/// right after forwards before anything consumes it). Owned temps
/// keep the fromAsync arm's release shape.
fn emit_kernel_route(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Operand {
    let src_op = ctx.lower_expr(args[0]);
    let src_ty = ctx.operand_ty(&src_op);
    let src_boxed = ctx.box_to_any_from_expr(args[0], src_op.clone());
    let fn_op = ctx.lower_expr(args[1]);
    let fn_ty = ctx.operand_ty(&fn_op);
    let fn_boxed = ctx.box_to_any_from_expr(args[1], fn_op.clone());
    let mut this_temp: Option<(Operand, Type)> = None;
    let this_boxed = if let Some(&t) = args.get(2) {
        let op = ctx.lower_expr(t);
        let ty = ctx.operand_ty(&op);
        let boxed = ctx.box_to_any_from_expr(t, op.clone());
        this_temp = Some((op, ty));
        boxed
    } else {
        Operand::Value(crate::ssa_lower_call_arr_ho_loop::emit_undef_any_box(ctx))
    };
    for &a in args.iter().skip(3) {
        let _ = ctx.lower_expr(a);
    }
    let fid = *ctx
        .fn_table
        .get("__torajs_array_from_dyn")
        .expect("__torajs_array_from_dyn intrinsic missing");
    let arr_id = intern_arr_layout(ctx.arr_layouts, Type::Any);
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(fid, vec![src_boxed, fn_boxed, this_boxed]),
        Type::Arr(arr_id),
        None,
    );
    ctx.emit_throw_check(None);
    if src_ty.is_refcounted() && ctx.expr_is_fresh_owned(args[0]) {
        ctx.emit_drop_value(src_op, src_ty);
    }
    if fn_ty.is_refcounted() && ctx.expr_is_fresh_owned(args[1]) {
        ctx.emit_drop_value(fn_op, fn_ty);
    }
    if let Some((op, ty)) = this_temp
        && ty.is_refcounted()
        && ctx.expr_is_fresh_owned(args[2])
    {
        ctx.emit_drop_value(op, ty);
    }
    Operand::Value(v)
}

/// Materialize `src_arr` from the iter arg per the four accepted shapes
/// (string / typed Array<T> / Set / array-like `{length: n}` struct).
/// Other input types panic at SSA lower-time per ES §23.1.2.1 subset.
fn materialize_src(
    ctx: &mut LowerCtx<'_>,
    arg_op: Operand,
    arg_ty: Type,
    arg_eid: ExprId,
) -> Operand {
    if matches!(arg_ty, Type::Str) {
        let arr_id = intern_arr_layout(ctx.arr_layouts, Type::Str);
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.arr_from_string, vec![arg_op]),
            Type::Arr(arr_id),
            None,
        );
        return Operand::Value(v);
    }
    if let Type::Arr(arr_id) = arg_ty {
        // S132 — `Array.from(<typed Array<T>>)` shallow copy. Same deep-
        // clone pattern as Object.values's Arr arm (arr_slice(0, len) +
        // per-element rc_inc for refcounted elem types). Bun spec
        // §23.1.2.1.
        let len = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(Type::I64, arg_op, ARR_LEN_OFF),
            Type::I64,
            None,
        );
        let cloned = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.arr_slice,
                vec![arg_op, Operand::ConstI64(0), Operand::Value(len)],
            ),
            Type::Arr(arr_id),
            None,
        );
        let elem_ty = ctx.arr_layouts[arr_id.0 as usize];
        if elem_ty.is_refcounted() {
            let cloned_len = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(Type::I64, Operand::Value(cloned), ARR_LEN_OFF),
                Type::I64,
                None,
            );
            ctx.emit_arr_rc_inc_range(
                Operand::Value(cloned),
                elem_ty,
                Operand::ConstI64(0),
                Operand::Value(cloned_len),
            );
        }
        return Operand::Value(cloned);
    }
    if matches!(arg_ty, Type::Set) {
        // `Array.from(set)` substrate (ES §23.1.2.1 + §24.2.3.13) shares
        // its map-iter walk with `[...set]` spread; the helper lives in
        // `ssa_lower_arr_from_set`.
        return crate::ssa_lower_arr_from_set::emit(ctx, arg_op);
    }
    if matches!(arg_ty, Type::Obj(_)) {
        // ES §23.1.2.1 step 3 — a struct source, iterable OR
        // array-like: the boxed cell drives the same unified walk the
        // `any` arm uses, whose cascade asks for an iterator first (a
        // generator object / `[Symbol.iterator]()` class instance)
        // and whose tail walks `length` + index keys otherwise. B6
        // 刀 3 retires the undefined-filled stub the array-like side
        // used to mint (it never read the index properties —
        // `Array.from({'0': 41, length: 1})` answered `[undefined]`).
        let boxed = ctx.box_to_any(arg_op.clone());
        let materialized = crate::ssa_lower_arr_from_any::emit_for_array_from(ctx, boxed);
        ctx.release_owned_temp(arg_eid, &arg_op);
        return materialized;
    }
    if matches!(arg_ty, Type::Any) {
        // RFC 20260725-getiterator-getmethod knife 5 — an `any` source
        // is already boxed, so it goes straight into the materializer
        // the arm below boxes FOR. Which lane it lands in is §7.4.2
        // GetIterator's call at runtime: a user `@@iterator`, else a
        // string / array / collection / class instance.
        let materialized = crate::ssa_lower_arr_from_any::emit_for_array_from(ctx, arg_op.clone());
        ctx.release_owned_temp(arg_eid, &arg_op);
        return materialized;
    }
    // B6 刀 3 — a nullish source (checker-typed Null / Undefined)
    // boxes and rides the unified walk to §23.1.2.1 step 5's RUNTIME
    // TypeError (items-is-null-throws); the SSA lattice has no
    // nullish variant, so the split keys on the checker record.
    if matches!(
        ctx.expr_types.get(&arg_eid),
        Some(crate::check::Type::Null | crate::check::Type::Undefined)
    ) {
        let boxed = ctx.box_to_any_from_expr(arg_eid, arg_op.clone());
        let materialized = crate::ssa_lower_arr_from_any::emit_for_array_from(ctx, boxed);
        ctx.release_owned_temp(arg_eid, &arg_op);
        return materialized;
    }
    if matches!(arg_ty, Type::Map | Type::MapIter | Type::ArrIter) {
        // `Array.from(map / iterator)` — box the heap source (tag-4
        // ANY_HEAP, rc-neutral encode) and drive the unified runtime
        // iteration protocol into a fresh `Array<Any>`, sharing the
        // materializer with `[...x]` spread's iterator arm. Ownership
        // mirrors the spread arm: `emit` borrows the boxed source and
        // yields an owned rc=1 array; `release_owned_temp` settles an
        // owned-temp source (`m.keys()` call) and no-ops a borrow.
        // A nullish source rides the same walk to §23.1.2.1 step 5's
        // RUNTIME TypeError (B6 刀 3, items-is-null-throws).
        let boxed = ctx.box_to_any(arg_op.clone());
        let materialized = crate::ssa_lower_arr_from_any::emit_for_array_from(ctx, boxed);
        ctx.release_owned_temp(arg_eid, &arg_op);
        return materialized;
    }
    panic!(
        "ssa-lower: Array.from requires a string, Array<T>, Set, or array-like arg, got {arg_ty:?}"
    )
}
