//! Date instance method dispatch pulled out of
//! [`crate::ssa_lower::lower_expr_inner`] `Expr::Call` dispatch as
//! chunk-34 of the `Expr::Call` god-arm decomp (chunks 1-33 = ... +
//! `iter.next()` / IteratorResult synth).
//!
//! v0.2 #2 — Date instance methods (.getTime / .valueOf /
//! .toISOString / .toJSON / 16 `getX` field getters + UTC variants /
//! .toGMTString / .toUTCString / .toDateString / .toLocaleX / annexB
//! .setYear / .getYear / 7 `setX` field setters). Recognized via
//! receiver type `Type::Date`, routed to the matching
//! `__torajs_date_*` runtime intrinsic.
//!
//! **Arg-shape modes** (per ES §21.4.4.*):
//!
//! - **0-arg getter / format** (S295) — recv only; trailing args
//!   typecheck-and-dropped (check.rs S261) and lower-and-dropped
//!   here for step()-side-effect semantics (S272 idiom).
//! - **`setTime` / `setYear` (1-arg, S268)** — `recv + f64(args[0])`;
//!   trailing args silent-drop per ES §21.4.4.27 / annexB §B.2.4.2.
//! - **Per-field setters** `setFullYear` (3) / `setMonth` (2) /
//!   `setDate` (1) / `setHours` (4) / `setMinutes` (3) /
//!   `setSeconds` (2) / `setMilliseconds` (1) — `recv +
//!   f64(args[..arity]) + i64 present-mask` (RFC
//!   20260713-date-invalid-time): missing trailing fields ride
//!   ConstF64(0) with the mask bit clear so the runtime keeps the
//!   current component, while a supplied NaN invalidates the date.
//!   Extras silent-drop with side-effect eval per S268.
//!
//! Returns `Some(op)` on hit; `None` on miss (callee not a
//! `Member`-call, name not in the allowlist, or receiver isn't
//! `Type::Date`) so the caller falls through to the RegExp / String
//! / generic method dispatch arms below.

use crate::ast::{Expr, ExprId};
use crate::ssa::{FuncId, InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

const DATE_METHOD_ALLOWLIST: &[&str] = &[
    "getTime",
    "valueOf",
    "toISOString",
    "toJSON",
    "getFullYear",
    "getUTCFullYear",
    "getMonth",
    "getUTCMonth",
    "getDate",
    "getUTCDate",
    "getHours",
    "getUTCHours",
    "getMinutes",
    "getUTCMinutes",
    "getSeconds",
    "getUTCSeconds",
    "getMilliseconds",
    "getUTCMilliseconds",
    "getDay",
    "getUTCDay",
    "getTimezoneOffset",
    "setTime",
    "setYear",
    "getYear",
    "toGMTString",
    "toUTCString",
    "toDateString",
    "toTimeString",
    "toString",
    "toLocaleString",
    "toLocaleDateString",
    "toLocaleTimeString",
    "setFullYear",
    "setMonth",
    "setDate",
    "setHours",
    "setMinutes",
    "setSeconds",
    "setMilliseconds",
    "setUTCFullYear",
    "setUTCMonth",
    "setUTCDate",
    "setUTCHours",
    "setUTCMinutes",
    "setUTCSeconds",
    "setUTCMilliseconds",
];

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let Expr::Member { obj, name } = ctx.ast.get_expr(callee) else {
        return None;
    };
    if !DATE_METHOD_ALLOWLIST.iter().any(|m| *m == name.as_str()) {
        return None;
    }
    let recv_id = *obj;
    let method = name.clone();
    let recv_op = ctx.lower_expr(recv_id);
    let recv_ty = ctx.operand_ty(&recv_op);
    if recv_ty != Type::Date {
        // RFC 20260705 chunk 555 — the receiver is already lowered;
        // park the operand for the next cascade arm (single-eval).
        ctx.redispatch_lowered = Some((recv_id, recv_op));
        return None;
    }
    crate::ssa_lower_nullable_guard::emit_undefable_heap_guard(ctx, recv_id, &recv_op);

    let arg_ops = build_arg_ops(ctx, &method, recv_op, args);
    let (target, ret_ty) = resolve_intrinsic(ctx, &method);
    let cur_block = ctx.cur_block;
    let v = ctx
        .f
        .append_inst(cur_block, InstKind::Call(target, arg_ops), ret_ty, None);
    if method == "toISOString" {
        // RFC 20260713-date-invalid-time — invalid time value records
        // a pending RangeError (§21.4.4.36 step 3); propagate it.
        // toJSON is NOT here: §21.4.4.37 steps 2-3 answer null for a
        // non-finite time value (the kernel returns Str-slot NULL).
        ctx.emit_throw_check(None);
    }
    Some(Operand::Value(v))
}

/// Coerce a Date-method numeric argument to the f64 ABI slot: spec
/// ToNumber semantics — Any routes through `any_to_number`, an
/// undefined payload (SSA Ptr) is NaN, numerics widen via SiToFp.
fn coerce_date_num(ctx: &mut LowerCtx<'_>, op: Operand) -> Operand {
    match ctx.operand_ty(&op) {
        Type::Any => {
            let v = ctx.coerce_any_to_number(op, Type::F64);
            // Rotation 365 — `? ToNumber(arg)` (§21.4.4.20 step 2 et
            // al.): a user valueOf/toString throw recorded by the
            // any_to_number kernel must abort HERE, before the setter
            // kernel writes [[DateValue]] — without the check the
            // throw was silently dropped AND the date absorbed the
            // garbage coercion result (the t262 Date `-err` family
            // asserts both faces: the throw propagates and getTime()
            // is unchanged).
            ctx.emit_throw_check(None);
            v
        }
        Type::Ptr => Operand::ConstF64(f64::NAN),
        _ => ctx.coerce_to_f64(op),
    }
}

fn build_arg_ops(
    ctx: &mut LowerCtx<'_>,
    method: &str,
    recv_op: Operand,
    args: &[ExprId],
) -> Vec<Operand> {
    // T-30 setters take f64 args (spec ToNumber). The per-field
    // setters (setFullYear / setMonth / setHours / …) take 1-4 args
    // plus a trailing i64 present-mask (bit k = arg k supplied):
    // missing trailing slots ride ConstF64(0) with the bit clear so
    // the runtime keeps the current field, while a supplied NaN
    // invalidates the date (RFC 20260713-date-invalid-time — the
    // retired i64::MIN KEEP sentinel collided with NaN's saturating
    // i64 cast).
    let per_field_arity: Option<usize> = match method {
        "setFullYear" | "setUTCFullYear" => Some(3),
        "setMonth" | "setUTCMonth" => Some(2),
        "setDate" | "setUTCDate" => Some(1),
        "setHours" | "setUTCHours" => Some(4),
        "setMinutes" | "setUTCMinutes" => Some(3),
        "setSeconds" | "setUTCSeconds" => Some(2),
        "setMilliseconds" | "setUTCMilliseconds" => Some(1),
        _ => None,
    };
    if method == "setTime" || method == "setYear" {
        // S268 — trailing args silent-drop per ES §21.4.4.27 (setTime)
        // / annexB §B.2.4.2 (setYear): sig is 1; extras eval-and-drop.
        debug_assert!(!args.is_empty());
        let a = ctx.lower_expr(args[0]);
        for ai in args.iter().skip(1) {
            let _ = ctx.lower_expr(*ai);
        }
        let a = coerce_date_num(ctx, a);
        return vec![recv_op, a];
    }
    if let Some(target_arity) = per_field_arity {
        // S268 — trailing args beyond `target_arity` silent-drop per
        // ES §21.4.4.{20-26}; preserve side effects via the
        // skip(arity) eval loop.
        debug_assert!(!args.is_empty());
        let mut ops = Vec::with_capacity(target_arity + 2);
        ops.push(recv_op);
        let mut present = 0i64;
        for i in 0..target_arity {
            if i < args.len() {
                let a = ctx.lower_expr(args[i]);
                let a = coerce_date_num(ctx, a);
                ops.push(a);
                present |= 1 << i;
            } else {
                ops.push(Operand::ConstF64(0.0));
            }
        }
        for ai in args.iter().skip(target_arity) {
            let _ = ctx.lower_expr(*ai);
        }
        ops.push(Operand::ConstI64(present));
        return ops;
    }
    // S295 — Date 0-arg getter / format methods (getTime / valueOf /
    // toISOString / toJSON / getFullYear / ... / toLocaleString)
    // accept trailing args per ES §21.4.4.* trailing-arg ignore.
    // check.rs S261 typecheck-and-drops them; lower-and-drop here so
    // step()-style side-effect exprs fire (S272 idiom). The helper's
    // 1-arg ABI (recv only) is preserved.
    for ai in args.iter() {
        let _ = ctx.lower_expr(*ai);
    }
    vec![recv_op]
}

fn resolve_intrinsic(ctx: &LowerCtx<'_>, method: &str) -> (FuncId, Type) {
    match method {
        "getTime" | "valueOf" => (ctx.intrinsics.date_get_time, Type::F64),
        "toISOString" => (ctx.intrinsics.date_to_iso_string, Type::Str),
        "toJSON" => (ctx.intrinsics.date_to_json, Type::Str),
        "getFullYear" => (ctx.intrinsics.date_get_full_year, Type::F64),
        "getUTCFullYear" => (ctx.intrinsics.date_get_utc_full_year, Type::F64),
        "getMonth" => (ctx.intrinsics.date_get_month, Type::F64),
        "getUTCMonth" => (ctx.intrinsics.date_get_utc_month, Type::F64),
        "getDate" => (ctx.intrinsics.date_get_date, Type::F64),
        "getUTCDate" => (ctx.intrinsics.date_get_utc_date, Type::F64),
        "getHours" => (ctx.intrinsics.date_get_hours, Type::F64),
        "getUTCHours" => (ctx.intrinsics.date_get_utc_hours, Type::F64),
        "getMinutes" => (ctx.intrinsics.date_get_minutes, Type::F64),
        "getUTCMinutes" => (ctx.intrinsics.date_get_utc_minutes, Type::F64),
        "getSeconds" => (ctx.intrinsics.date_get_seconds, Type::F64),
        "getUTCSeconds" => (ctx.intrinsics.date_get_utc_seconds, Type::F64),
        "getMilliseconds" => (ctx.intrinsics.date_get_milliseconds, Type::F64),
        "getUTCMilliseconds" => (ctx.intrinsics.date_get_utc_milliseconds, Type::F64),
        "getDay" => (ctx.intrinsics.date_get_day, Type::F64),
        "getUTCDay" => (ctx.intrinsics.date_get_utc_day, Type::F64),
        "getTimezoneOffset" => (ctx.intrinsics.date_get_timezone_offset, Type::F64),
        "setTime" => (ctx.intrinsics.date_set_time, Type::F64),
        "setYear" => (ctx.intrinsics.date_set_year, Type::F64),
        "getYear" => (ctx.intrinsics.date_get_year, Type::F64),
        "toGMTString" | "toUTCString" => (ctx.intrinsics.date_to_gmt_string, Type::Str),
        "toDateString" => (ctx.intrinsics.date_to_date_string, Type::Str),
        "toTimeString" => (ctx.intrinsics.date_to_time_string, Type::Str),
        "toString" => (ctx.intrinsics.date_to_string, Type::Str),
        "toLocaleString" => (ctx.intrinsics.date_to_locale_string, Type::Str),
        "toLocaleDateString" => (ctx.intrinsics.date_to_locale_date_string, Type::Str),
        "toLocaleTimeString" => (ctx.intrinsics.date_to_locale_time_string, Type::Str),
        "setFullYear" => (ctx.intrinsics.date_set_full_year, Type::F64),
        "setMonth" => (ctx.intrinsics.date_set_month, Type::F64),
        "setDate" => (ctx.intrinsics.date_set_date, Type::F64),
        "setHours" => (ctx.intrinsics.date_set_hours, Type::F64),
        "setMinutes" => (ctx.intrinsics.date_set_minutes, Type::F64),
        "setSeconds" => (ctx.intrinsics.date_set_seconds, Type::F64),
        "setMilliseconds" => (ctx.intrinsics.date_set_milliseconds, Type::F64),
        "setUTCFullYear" => (ctx.intrinsics.date_set_utc_full_year, Type::F64),
        "setUTCMonth" => (ctx.intrinsics.date_set_utc_month, Type::F64),
        "setUTCDate" => (ctx.intrinsics.date_set_utc_date, Type::F64),
        "setUTCHours" => (ctx.intrinsics.date_set_utc_hours, Type::F64),
        "setUTCMinutes" => (ctx.intrinsics.date_set_utc_minutes, Type::F64),
        "setUTCSeconds" => (ctx.intrinsics.date_set_utc_seconds, Type::F64),
        "setUTCMilliseconds" => (ctx.intrinsics.date_set_utc_milliseconds, Type::F64),
        _ => unreachable!(),
    }
}
