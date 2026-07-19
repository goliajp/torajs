//! `Promise.<static>` cluster (`Promise.all` / `.race` / `.any` /
//! `.allSettled` / `.resolve` / `.reject`) pulled out of
//! [`crate::ssa_lower::lower_expr_inner`] `Expr::Call` dispatch as
//! chunk-27 of the `Expr::Call` god-arm decomp (chunks 1-26 = ... +
//! fs_promises async wrappers).
//!
//! Three arms share the `Promise.<m>(...)` namespace:
//!
//! - **T-17.a / .b / .c / .d (v0.5.0)** — `Promise.all` / `.race` /
//!   `.any` / `.allSettled` sync fast paths. Lower `args[0]` (iterable),
//!   eval-and-drop `args[1..]` per S273 ES §27.2.4.{1,3,5,2} trailing-
//!   arg ignore, dispatch to the matching `promise_<m>_sync` intrinsic.
//!   Falls through (returns `None`) when `args.is_empty()` so a more
//!   generic call lowering can fire.
//!
//! - **P10.2-A1** — 0-arg `Promise.resolve()` / `Promise.reject()`.
//!   Spec-equivalent to `Promise.{resolve,reject}(undefined)` which
//!   shares the i64-0 sentinel ABI with `null`; synthesize
//!   `ConstI64(0)` and dispatch the non-heap fulfilled/rejected
//!   allocator (same path 1-arg primitive takes).
//!
//! - **T-15.g.1 / T-15.g.5** — 1+arg `Promise.resolve(v)` /
//!   `Promise.reject(e)`. S322 lower-and-drop trailing `args[1..]` per
//!   S272 idiom. T-19.f thenable absorption: `Promise.resolve(p)` on
//!   `Type::Promise` arg routes through `promise_resolve_thenable`
//!   (unwrap inner) instead of wrapping the pointer — reject side
//!   keeps simple-heap path per spec. Heap-vs-primitive dispatch by
//!   `arg_ty`; `Bool` → `coerce_bool_to_i64`; `F64` → `BitCastF64ToI64`
//!   so the value slot uniformly holds 8 bytes the receiver decodes
//!   per the promise's value class (②.6b). When `arg_ty == I64` but
//!   the field width table says this anon-ExprId slot is f64-shaped,
//!   `coerce_to_f64` + `BitCastF64ToI64` first.
//!
//! Returns `Some(op)` on hit; `None` on miss so the caller falls
//! through to subsequent arms or the generic call lowering.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let Expr::Member {
        obj: ns_id,
        name: m_name,
    } = ctx.ast.get_expr(callee)
    else {
        return None;
    };
    let recv_id = *ns_id;
    let method = m_name.clone();
    let Expr::Ident(ns) = ctx.ast.get_expr(recv_id) else {
        return None;
    };
    if ns != "Promise" {
        return None;
    }
    let m = method.as_str();
    match m {
        "all" | "race" | "any" | "allSettled" if !args.is_empty() => {
            Some(lower_aggregate(ctx, m, args))
        }
        "resolve" | "reject" if args.is_empty() => Some(lower_zero_arg(ctx, m)),
        "resolve" | "reject" => Some(lower_one_plus(ctx, eid, m, args)),
        _ => None,
    }
}

/// `Promise.all/race/any/allSettled(xs, ...)` — lower `args[0]` and
/// drop the rest for side-effects per S273 / ES §27.2.4.
fn lower_aggregate(ctx: &mut LowerCtx<'_>, method: &str, args: &[ExprId]) -> Operand {
    let arr_op = ctx.lower_expr(args[0]);
    for &a in &args[1..] {
        let _ = ctx.lower_expr(a);
    }
    let fid = match method {
        "all" => ctx.intrinsics.promise_all_sync,
        "race" => ctx.intrinsics.promise_race_sync,
        "any" => ctx.intrinsics.promise_any_sync,
        "allSettled" => ctx.intrinsics.promise_allsettled_sync,
        _ => unreachable!(),
    };
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(fid, vec![arr_op.clone()]),
        Type::Promise,
        None,
    );
    // The combinator borrows the promises array (walks slots, incs
    // what it keeps), so an Ident arg keeps its stake and its scope
    // drop — the old consume path orphaned the array AND its promises
    // (RFC 20260705 ledger #3, 35MB probe). Owned temps release here.
    ctx.release_owned_temp(args[0], &arr_op);
    Operand::Value(v)
}

/// `Promise.{resolve,reject}()` — synthesize the undefined sentinel
/// and route to the primitive fulfilled/rejected allocator.
fn lower_zero_arg(ctx: &mut LowerCtx<'_>, method: &str) -> Operand {
    let fid = match method {
        "resolve" => ctx.intrinsics.promise_alloc_fulfilled,
        "reject" => ctx.intrinsics.promise_alloc_rejected,
        _ => unreachable!(),
    };
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(fid, vec![Operand::ConstI64(0)]),
        Type::Promise,
        None,
    );
    // RFC 20260720-anylane-promise-methods knife 1 — the zero-arg
    // form settles with undefined; the any-lane bridge answers it
    // as such.
    let out = Operand::Value(v);
    ctx.emit_promise_stamp_repr(&out, crate::ssa_lower_promise_repr_mark::REPR_VOID);
    out
}

/// `Promise.{resolve,reject}(v, ...)` — thenable absorption + heap /
/// primitive dispatch + S322 trailing-arg side-effect.
fn lower_one_plus(ctx: &mut LowerCtx<'_>, eid: ExprId, method: &str, args: &[ExprId]) -> Operand {
    let arg_op = ctx.lower_expr(args[0]);
    for &a in args.iter().skip(1) {
        let _ = ctx.lower_expr(a);
    }
    let arg_ty = ctx.operand_ty(&arg_op);
    // T-19.f — thenable absorption. `promise_resolve_thenable` borrows
    // the promise (shares its inner value into the fresh one), so an
    // Ident arg keeps its stake; owned temps release post-call.
    if matches!(arg_ty, Type::Promise) && method == "resolve" {
        let cur_block = ctx.cur_block;
        let v = ctx.f.append_inst(
            cur_block,
            InstKind::Call(
                ctx.intrinsics.promise_resolve_thenable,
                vec![arg_op.clone()],
            ),
            Type::Promise,
            None,
        );
        ctx.release_owned_temp(args[0], &arg_op);
        return Operand::Value(v);
    }
    let is_heap = matches!(
        arg_ty,
        Type::Str
            | Type::Substr
            | Type::Obj(_)
            | Type::Arr(_)
            | Type::Closure(_)
            | Type::RegExp
            | Type::Date
            | Type::Symbol
            | Type::Promise
            | Type::Any
    );
    let fid = match (method, is_heap) {
        ("resolve", false) => ctx.intrinsics.promise_alloc_fulfilled,
        ("reject", false) => ctx.intrinsics.promise_alloc_rejected,
        ("resolve", true) => ctx.intrinsics.promise_alloc_fulfilled_heap,
        ("reject", true) => ctx.intrinsics.promise_alloc_rejected_heap,
        _ => unreachable!(),
    };
    // `promise_alloc_*_heap` ADOPTS the value (caller transfers one
    // ref, pool.rs contract). A borrow-shaped arg therefore shares:
    // +1 so the source binding keeps its own stake — the old consume
    // path stole it (UAF once the promise dropped first, reuse-window
    // probe printed filler vs bun val42). Owned temps transfer their
    // fresh ref as-is.
    if is_heap && !ctx.expr_transfers_ownership(args[0]) {
        ctx.emit_owned_result_inc(arg_op.clone(), arg_ty);
    }
    // RFC 20260720-anylane-promise-methods knife 1 — the repr stamp
    // mirrors the STORED form this site emits (the I64→f64-slot
    // widening below changes it), so capture the predicates the
    // coercion chain is about to consume.
    let is_null = matches!(arg_op, Operand::ConstPtrNull);
    let stored_as_f64 = matches!(arg_ty, Type::F64)
        || (matches!(arg_ty, Type::I64)
            && ctx
                .num_f64_slots
                .field_is_f64(&crate::num_width::SlotKey::Anon(eid.0), "value"));
    mark_arr_value(ctx, &arg_op, &arg_ty);
    let arg_i64 = if matches!(arg_ty, Type::Bool) {
        ctx.coerce_bool_to_i64(arg_op)
    } else if matches!(arg_ty, Type::F64) {
        let cur_block = ctx.cur_block;
        Operand::Value(ctx.f.append_inst(
            cur_block,
            InstKind::BitCastF64ToI64(arg_op),
            Type::I64,
            None,
        ))
    } else if matches!(arg_ty, Type::I64)
        && ctx
            .num_f64_slots
            .field_is_f64(&crate::num_width::SlotKey::Anon(eid.0), "value")
    {
        let as_f64 = ctx.coerce_to_f64(arg_op);
        let cur_block = ctx.cur_block;
        Operand::Value(ctx.f.append_inst(
            cur_block,
            InstKind::BitCastF64ToI64(as_f64),
            Type::I64,
            None,
        ))
    } else {
        arg_op
    };
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(fid, vec![arg_i64]),
        Type::Promise,
        None,
    );
    let out = Operand::Value(v);
    if let Some(repr) =
        crate::ssa_lower_promise_repr_mark::promise_value_repr(&arg_ty, stored_as_f64, is_null)
    {
        ctx.emit_promise_stamp_repr(&out, repr);
    }
    out
}

/// The repr stamp makes the cell any-consumable, so a typed-Arr
/// value settles as an any-readable cell too: mark its elem kind
/// (RFC 20260704 S1 self-describing posture — the bridge's boxed
/// hand-off is exactly the typed→any crossing the mark exists for).
fn mark_arr_value(ctx: &mut LowerCtx<'_>, arg_op: &Operand, arg_ty: &Type) {
    if matches!(arg_ty, Type::Arr(_)) {
        ctx.emit_arr_mark_kind(arg_op);
    }
}
