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
    eid: ExprId,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let (ns_id, m_name) = match ctx.ast.get_expr(callee) {
        Expr::Member { obj, name } => (*obj, name.clone()),
        _ => return None,
    };
    if m_name != "from" {
        return None;
    }
    if !matches!(ctx.ast.get_expr(ns_id), Expr::Ident(n) if n == "Array") {
        return None;
    }
    if args.is_empty() {
        return None;
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
    // §23.1.2.1 step 4 reads `GetMethod(items, @@iterator)`; the fast
    // lanes below read the source's backing store instead. That is the
    // right trade while nothing can install an iteration hook — and a
    // module that installs one takes the kernel route for every
    // `Array.from`, source shape regardless. Same posture as the
    // spread lanes (`ssa_lower_array_spread`).
    if ctx.scans.proto_shadow.iterator_hook_written() {
        return Some(emit_kernel_route(ctx, args));
    }
    // S275 — widen `== 1 || == 2` to `>= 1`. The 2-arg path below handles
    // iter+mapFn; trailing args (thisArg + extras) are eval-and-dropped at
    // the end so side-effect exprs fire per §23.1.2.1 trailing-arg ignore.
    let arg_op = ctx.lower_expr(args[0]);
    // §23.1.2.1 step 4 is GetMethod(items, @@iterator), which reads a
    // property off `items` — a TypeError for undefined. The typed
    // route below reads the source's backing store instead, and
    // `apply_rest_args` spells `g(...r)` as `Array.from(r)`, so a
    // rest-parameter call with a source holding the cell died here.
    crate::ssa_lower_nullable_guard::emit_undef_cell_guard(ctx, args[0], &arg_op);
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
    Some(emit_map_loop(ctx, src_arr_op, args, eid))
}

/// The B6 刀 3 kernel route: box (src, mapFn, thisArg) and call
/// `__torajs_array_from_dyn`. The kernel answers a raw fresh
/// `Array<Any>` pointer (NULL under a pending throw, which the check
/// right after forwards before anything consumes it). Owned temps
/// are released after the call (the combinator dyn-entry shape).
fn emit_kernel_route(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Operand {
    let src_op = ctx.lower_expr(args[0]);
    let src_ty = ctx.operand_ty(&src_op);
    let src_boxed = ctx.box_to_any_from_expr(args[0], src_op);
    // `Array.from(src)` reaches here too when the module installs a
    // `Symbol.iterator` (see [`try_lower`]); its mapFn is absent, and
    // §23.1.2.1 spells that `mapping = false` — which the kernel reads
    // off an undefined second argument.
    let mapfn = args.get(1).copied();
    let (fn_op, fn_ty, fn_boxed) = match mapfn {
        Some(a) => {
            let op = ctx.lower_expr(a);
            let ty = ctx.operand_ty(&op);
            let boxed = ctx.box_to_any_from_expr(a, op);
            (op, ty, boxed)
        }
        None => (
            Operand::ConstPtrNull,
            Type::Void,
            Operand::Value(crate::ssa_lower_call_arr_ho_loop::emit_undef_any_box(ctx)),
        ),
    };
    let mut this_temp: Option<(Operand, Type)> = None;
    let this_boxed = if let Some(&t) = args.get(2) {
        let op = ctx.lower_expr(t);
        let ty = ctx.operand_ty(&op);
        let boxed = ctx.box_to_any_from_expr(t, op);
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
    if let Some(a) = mapfn
        && fn_ty.is_refcounted()
        && ctx.expr_is_fresh_owned(a)
    {
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
        return ctx.emit_arr_from_string(arg_op, -1).0;
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
        let out_id = ctx.copied_arr_layout(arr_id);
        let elem_ty = ctx.arr_layouts[arr_id.0 as usize];
        if elem_ty == Type::Any {
            // A boxed-element source can hold holes and accessor
            // indices; §23.1.2.1 iterates it, so each index is a [[Get]]
            // (a hole reads undefined, a getter runs) — the flag-aware
            // copy does that and takes its own stake per slot.
            let cloned = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.arr_any_slice,
                    vec![arg_op, Operand::ConstI64(0), Operand::Value(len)],
                ),
                Type::Arr(out_id),
                None,
            );
            ctx.emit_throw_check_owned(None, Operand::Value(cloned), Type::Arr(out_id));
            return Operand::Value(cloned);
        }
        let fill = crate::ssa_lower_str_arr_inplace::hole_fill_for(ctx, elem_ty);
        let cloned = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.arr_copy_get, vec![arg_op, fill]),
            // A copy out of an `Arr<Substr>` is `Arr<Str>` (views do
            // not leave their split block — rotation 468).
            Type::Arr(out_id),
            None,
        );
        if elem_ty.is_refcounted() {
            let cloned_len = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(Type::I64, Operand::Value(cloned), ARR_LEN_OFF),
                Type::I64,
                None,
            );
            ctx.emit_adopt_copied_range(
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
        let boxed = ctx.box_to_any(arg_op);
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
        let materialized = crate::ssa_lower_arr_from_any::emit_for_array_from(ctx, arg_op);
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
        let boxed = ctx.box_to_any_from_expr(arg_eid, arg_op);
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
        let boxed = ctx.box_to_any(arg_op);
        let materialized = crate::ssa_lower_arr_from_any::emit_for_array_from(ctx, boxed);
        ctx.release_owned_temp(arg_eid, &arg_op);
        return materialized;
    }
    panic!(
        "ssa-lower: Array.from requires a string, Array<T>, Set, or array-like arg, got {arg_ty:?}"
    )
}
