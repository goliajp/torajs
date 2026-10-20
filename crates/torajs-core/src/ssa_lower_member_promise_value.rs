//! T-15.g.2 (v0.5.0) — `await p` (= parser-desugared `p.value`) on
//! a built-in `Type::Promise(T)` plus P10.4 fast-path identity for
//! `await <primitive>`. Pulled out of
//! [`crate::ssa_lower::lower_expr_inner`]'s `Expr::Member` god-arm
//! as chunk-62 of the decomp (chunks 1-61 = ... + T-27.c
//! `f.length`/`f.name` dual-path).
//!
//! The Promise path lowers to a runtime
//! `__torajs_promise_get_value(p)` call which reads the resolved
//! i64 value slot, with microtask drain before and value-cast +
//! rc_inc after. Static-only dispatch: only fire when we can
//! determine `obj`'s type IS `Type::Promise` without lowering
//! (eager lowering would double-side-effect the obj subexpression
//! on fall-through).
//!
//! **Built-in promise detection** — recognise the source shape from
//! the AST so the fast path stays narrow:
//! - Ident bound to `Type::Promise` in `LowerCtx::locals`
//! - Call result: `Promise.{resolve|reject|all|race|any|allSettled}`,
//!   `fs_promises.{readFile|writeFile|appendFile|unlink|mkdir|exists|
//!   readdir}`, `Bun.file(...).{text|exists}()`, `<promise>.{then|
//!   catch|finally}` chain result, user fn returning `Type::Promise`
//!   (via `signatures`), `fetch(url)`, `<response>.text()`
//! - Any other receiver typed `check::Type::Promise(_)` via
//!   `expr_types` (covers `await ps[i]` indexing, tuple destructure,
//!   etc.)
//!
//! **P10.4 identity fast-path** — `await <primitive>` collapses to
//! the operand itself per ES spec (`Promise.resolve(e)` for
//! non-thenable yields e). Whitelist of `check::Type` Number / String
//! / Boolean / Array(_) / BigInt: lower obj directly, no promise
//! alloc, no microtask drain. `Type::Object("...")` and
//! `Type::Struct` are NOT in the whitelist because they CAN carry a
//! real user-defined `.value` field — those fall through to the
//! regular Member path. Pair this arm with the matching check.rs
//! arm so typecheck + lowering agree.
//!
//! **Value-cast switch** — `promise_get_value` returns i64
//! regardless of T; SSA value-table needs the proper shape:
//! - heap T (`Str / Substr / Obj / Arr / Closure / RegExp / Date /
//!   Symbol / Promise / Any / Ptr`): `IntToPtr` + `emit_rc_inc`
//!   (T-19.j: Promise owns ONE ref on inner value; when temp
//!   Promise drops, value_drop_heap dec's that ref — without
//!   `rc_inc` the value frees BEFORE the user reads it, UAF that
//!   accidentally worked for n≤32 via pool block reuse but
//!   corrupted at n>32 where libc free is real).
//! - `Bool`: `TruncI64ToBool`.
//! - `F64`: `BitCastI64ToF64` (②.6b — resolve/cb-thunk write f64
//!   bits, decode symmetrically).
//! - Other / None: raw i64 (covers Promise<Number> and legacy
//!   `lower(...)` entry point with empty check map).
//!
//! P10.4-A2: post-call `emit_throw_check(None)` propagates
//! catchable throw slot rejections into the user's `try/catch`.
//!
//! Returns `Some(op)` on hit; `None` on miss (Member name not
//! `value`, or obj is neither built-in Promise nor primitive
//! whitelist).

use crate::ast::{Expr, ExprId};
use crate::check::{self as check_mod};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_parse_type::parse_type;

mod detect;
mod object_await;

pub(crate) use detect::detect_obj_is_builtin_promise;
use object_await::{lower_object_await, lower_typed_await, try_p10_4_primitive_identity};

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    obj: ExprId,
    name: &str,
) -> Option<Operand> {
    if name != "value" {
        return None;
    }
    if ctx.suspend_body && ctx.ast.await_value_reads.contains(&eid) {
        return Some(crate::ssa_lower_fn::suspend::lower_await(ctx, eid, obj));
    }
    let obj_is_builtin_promise = detect_obj_is_builtin_promise(ctx, obj);
    if obj_is_builtin_promise {
        // a promise of a thenable reads in the `any` lane its await is typed
        let any_lane = matches!(ctx.expr_types.get(&eid), Some(check_mod::Type::Any));
        return Some(lower_promise_value_read(ctx, obj, any_lane));
    }
    // rotation 233 — a parser-minted `await e` read
    // (`Ast::await_value_reads`) on a NON-promise operand is identity
    // for every type per §27.7.5.1 (`Promise.resolve(e)` of a
    // non-thenable yields e) — including Struct / class receivers,
    // which used to fall through to the field lookup and win a user
    // `{value: T}` field (`await {value: 1}` answered `1`). The
    // P10.4 whitelist below stays for unmarked `.value` reads (a
    // desugar-rebuilt Member that lost its mark degrades to the old
    // name-based posture, never to a crash).
    if ctx.ast.await_value_reads.contains(&eid) {
        let op = ctx.lower_expr(obj);
        // An `any` operand only knows its form at runtime — route
        // through the `__torajs_anyv_await` hook (heap Promise cell
        // unwraps per its repr stamp, everything else identity).
        if matches!(ctx.operand_ty(&op), Type::Any) {
            return Some(lower_any_await(ctx, op, obj));
        }
        // an object awaits as `any` (its checker type): it settles
        // through PromiseResolve, which runs a `then` it carries
        if matches!(ctx.expr_types.get(&eid), Some(check_mod::Type::Any)) {
            let borrowed = !ctx.expr_transfers_ownership(obj);
            let boxed = ctx.box_owned_from_expr(obj, op, borrowed);
            ctx.owned_member_reads.insert(eid);
            return Some(lower_object_await(ctx, boxed));
        }
        return Some(lower_typed_await(ctx, eid, obj, op));
    }
    try_p10_4_primitive_identity(ctx, obj)
}

/// rotation 233 — `await <any>`: drain microtasks (same as the typed
/// await route), hand the box to the runtime's by-VALUE dispatch,
/// propagate a rejection via the throw slot. The hook's result
/// carries its own +1 stake; an owned source temp is released here,
/// mirroring [`lower_promise_get_value`]'s borrow classification.
pub(crate) fn lower_any_await(ctx: &mut LowerCtx<'_>, op: Operand, obj: ExprId) -> Operand {
    let is_borrow = matches!(
        ctx.ast.get_expr(obj),
        Expr::Ident(_) | Expr::Member { .. } | Expr::Index { .. }
    );
    // the drain may rebind a borrowed operand's binding and release it
    if is_borrow {
        ctx.emit_owned_result_inc(op, Type::Any);
    }
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Call(ctx.intrinsics.microtask_drain, vec![]),
    );
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.anyv_await, vec![op]),
        Type::Any,
        None,
    );
    ctx.emit_throw_check_owned(None, op, Type::Any);
    ctx.emit_drop_value(op, Type::Any);
    Operand::Value(v)
}

/// `pub(crate)`: also the for-await element unwrap —
/// `ssa_lower_stmt_for_of` calls this with the elem `Expr::Index`
/// when the checker typed the element `Promise(_)` (hole Z removed
/// the `.value` Member wrap that used to route it here).
pub(crate) fn lower_promise_get_value(ctx: &mut LowerCtx<'_>, obj: ExprId) -> Operand {
    lower_promise_value_read(ctx, obj, false)
}

fn lower_promise_value_read(ctx: &mut LowerCtx<'_>, obj: ExprId, any_lane: bool) -> Operand {
    let obj_op = ctx.lower_expr(obj);
    // the drain below runs jobs that may rebind the binding a borrowed
    // operand was read from and release the cell: the await takes its
    // own stake on a borrowed cell, so every path below owns one
    if matches!(
        ctx.ast.get_expr(obj),
        Expr::Ident(_) | Expr::Member { .. } | Expr::Index { .. }
    ) {
        ctx.emit_owned_result_inc(obj_op, Type::Promise);
    }
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Call(ctx.intrinsics.microtask_drain, vec![]),
    );
    let inner_ssa_ty = if any_lane {
        Some(Type::Any)
    } else {
        let inner = recover_inner_ssa_ty(ctx, obj);
        ctx.widen_promise_inner_ty(inner, obj)
    };
    let cur_block = ctx.cur_block;
    // RFC 20260727 blade 3 — tell the read which lane it is feeding.
    // `cast_promise_value` below casts by the STATIC inner type; when
    // the cell was settled from an `any` its slot holds a NaN-box
    // pointer, and that cast reinterprets it (NaN for a number, a wild
    // deref for a string). The stamp on the cell is the only runtime
    // record of the real form, so the read consults it.
    let want_repr = awaited_lane_repr(inner_ssa_ty.as_ref());
    // a still-pending cell raises here rather than answering 0 into
    // the slot (null for a heap lane, `0` / `false` for a Copy lane)
    let raw_v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.promise_get_value_as,
            vec![obj_op, Operand::ConstI64(want_repr)],
        ),
        Type::I64,
        None,
    );
    // A rejected cell leaves through the throw path, which never
    // reaches the drop below, so the stake goes with it there.
    ctx.emit_throw_check_owned(None, obj_op, Type::Promise);
    let v = cast_promise_value(ctx, raw_v, inner_ssa_ty);
    ctx.emit_drop_value(obj_op, Type::Promise);
    Operand::Value(v)
}

/// The repr code `__torajs_promise_get_value_as` should convert into,
/// for the lane [`cast_promise_value`] is about to cast to. `0` means
/// "leave the slot alone" — the lane is unknown, so there is nothing
/// to convert towards.
///
/// An `any` lane asks for REPR_ANY rather than the slot verbatim: the
/// cast rc_incs what it gets as a NaN box, and a cell reached through
/// `Promise.resolve(<any>)`'s same-object pass-through carries the
/// SOURCE promise's repr, which is routinely a plain i64.
pub(crate) fn awaited_lane_repr(inner_ssa_ty: Option<&Type>) -> i64 {
    let Some(ty) = inner_ssa_ty else {
        return 0;
    };
    if matches!(ty, Type::Any) {
        return crate::ssa_lower_promise_repr_mark::REPR_ANY;
    }
    let as_f64 = matches!(ty, Type::F64);
    crate::ssa_lower_promise_repr_mark::promise_value_repr(ty, as_f64, false).unwrap_or(0)
}

/// `pub(crate)`: the `Promise.all` lowering runs the same recovery on
/// its own call expression, so the array it BUILDS and the `await` that
/// READS it name the same element lane (`ssa_lower_call_promise_static`).
pub(crate) fn recover_inner_ssa_ty(ctx: &mut LowerCtx<'_>, obj: ExprId) -> Option<Type> {
    let ann = if let Some(check_mod::Type::Promise(inner)) = ctx.expr_types.get(&obj) {
        check_mod::type_to_ann(inner)
    } else {
        return None;
    };
    Some(parse_type(
        Some(&ann),
        ctx.aliases,
        ctx.arr_layouts,
        ctx.fn_sigs,
        ctx.generic_struct_decls,
        ctx.struct_layouts,
        ctx.inst_memo,
    ))
}

pub(crate) fn cast_promise_value(
    ctx: &mut LowerCtx<'_>,
    raw_v: crate::ssa::ValueId,
    inner_ssa_ty: Option<Type>,
) -> crate::ssa::ValueId {
    let cur_block = ctx.cur_block;
    match inner_ssa_ty {
        Some(t)
            if matches!(
                t,
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
                    | Type::Ptr
            ) =>
        {
            let ptr = ctx.f.append_inst(
                cur_block,
                InstKind::IntToPtr(Operand::Value(raw_v)),
                t,
                None,
            );
            ctx.emit_rc_inc(Operand::Value(ptr));
            ptr
        }
        Some(Type::Bool) => ctx.f.append_inst(
            cur_block,
            InstKind::TruncI64ToBool(Operand::Value(raw_v)),
            Type::Bool,
            None,
        ),
        Some(Type::F64) => ctx.f.append_inst(
            cur_block,
            InstKind::BitCastI64ToF64(Operand::Value(raw_v)),
            Type::F64,
            None,
        ),
        _ => raw_v,
    }
}
