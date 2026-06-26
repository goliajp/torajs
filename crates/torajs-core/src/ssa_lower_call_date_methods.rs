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
//! - **`setTime` / `setYear` (1-arg, S268)** — `recv + i64(args[0])`;
//!   trailing args silent-drop per ES §21.4.4.27 / annexB §B.2.4.2.
//! - **Per-field setters** `setFullYear` (3) / `setMonth` (2) /
//!   `setDate` (1) / `setHours` (4) / `setMinutes` (3) /
//!   `setSeconds` (2) / `setMilliseconds` (1) — `recv +
//!   i64(args[..arity])`; missing trailing fields padded with the
//!   `DATE_FIELD_KEEP` sentinel (`i64::MIN`) so the runtime can keep
//!   the current field value without a separate arity-N intrinsic
//!   per overload. Extras silent-drop with side-effect eval per
//!   S268.
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
        return None;
    }

    let arg_ops = build_arg_ops(ctx, &method, recv_op, args);
    let (target, ret_ty) = resolve_intrinsic(ctx, &method);
    let cur_block = ctx.cur_block;
    let v = ctx
        .f
        .append_inst(cur_block, InstKind::Call(target, arg_ops), ret_ty, None);
    Some(Operand::Value(v))
}

fn build_arg_ops(
    ctx: &mut LowerCtx<'_>,
    method: &str,
    recv_op: Operand,
    args: &[ExprId],
) -> Vec<Operand> {
    // T-30 setters take 1 arg (Number → I64). The per-field setters
    // (setFullYear / setMonth / setHours / …) take 1-4 args; missing
    // trailing args are padded with the DATE_FIELD_KEEP sentinel
    // (`i64::MIN`) so the runtime can keep the current field value
    // without a separate arity-N intrinsic per overload.
    let per_field_arity: Option<usize> = match method {
        "setFullYear" => Some(3),
        "setMonth" => Some(2),
        "setDate" => Some(1),
        "setHours" => Some(4),
        "setMinutes" => Some(3),
        "setSeconds" => Some(2),
        "setMilliseconds" => Some(1),
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
        return vec![recv_op, ctx.coerce_to_i64(a)];
    }
    if let Some(target_arity) = per_field_arity {
        // S268 — trailing args beyond `target_arity` silent-drop per
        // ES §21.4.4.{20-26}; preserve side effects via the
        // skip(arity) eval loop.
        debug_assert!(!args.is_empty());
        let mut ops = Vec::with_capacity(target_arity + 1);
        ops.push(recv_op);
        for i in 0..target_arity {
            if i < args.len() {
                let a = ctx.lower_expr(args[i]);
                ops.push(ctx.coerce_to_i64(a));
            } else {
                ops.push(Operand::ConstI64(i64::MIN));
            }
        }
        for ai in args.iter().skip(target_arity) {
            let _ = ctx.lower_expr(*ai);
        }
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
        "getTime" | "valueOf" => (ctx.intrinsics.date_get_time, Type::I64),
        "toISOString" | "toJSON" => (ctx.intrinsics.date_to_iso_string, Type::Str),
        "getFullYear" => (ctx.intrinsics.date_get_full_year, Type::I64),
        "getUTCFullYear" => (ctx.intrinsics.date_get_utc_full_year, Type::I64),
        "getMonth" => (ctx.intrinsics.date_get_month, Type::I64),
        "getUTCMonth" => (ctx.intrinsics.date_get_utc_month, Type::I64),
        "getDate" => (ctx.intrinsics.date_get_date, Type::I64),
        "getUTCDate" => (ctx.intrinsics.date_get_utc_date, Type::I64),
        "getHours" => (ctx.intrinsics.date_get_hours, Type::I64),
        "getUTCHours" => (ctx.intrinsics.date_get_utc_hours, Type::I64),
        "getMinutes" => (ctx.intrinsics.date_get_minutes, Type::I64),
        "getUTCMinutes" => (ctx.intrinsics.date_get_utc_minutes, Type::I64),
        "getSeconds" => (ctx.intrinsics.date_get_seconds, Type::I64),
        "getUTCSeconds" => (ctx.intrinsics.date_get_utc_seconds, Type::I64),
        "getMilliseconds" => (ctx.intrinsics.date_get_milliseconds, Type::I64),
        "getUTCMilliseconds" => (ctx.intrinsics.date_get_utc_milliseconds, Type::I64),
        "getDay" => (ctx.intrinsics.date_get_day, Type::I64),
        "getUTCDay" => (ctx.intrinsics.date_get_utc_day, Type::I64),
        "getTimezoneOffset" => (ctx.intrinsics.date_get_timezone_offset, Type::I64),
        "setTime" => (ctx.intrinsics.date_set_time, Type::I64),
        "setYear" => (ctx.intrinsics.date_set_year, Type::I64),
        "getYear" => (ctx.intrinsics.date_get_year, Type::I64),
        "toGMTString" | "toUTCString" => (ctx.intrinsics.date_to_gmt_string, Type::Str),
        "toDateString" => (ctx.intrinsics.date_to_date_string, Type::Str),
        "toLocaleString" => (ctx.intrinsics.date_to_locale_string, Type::Str),
        "toLocaleDateString" => (ctx.intrinsics.date_to_locale_date_string, Type::Str),
        "toLocaleTimeString" => (ctx.intrinsics.date_to_locale_time_string, Type::Str),
        "setFullYear" => (ctx.intrinsics.date_set_full_year, Type::I64),
        "setMonth" => (ctx.intrinsics.date_set_month, Type::I64),
        "setDate" => (ctx.intrinsics.date_set_date, Type::I64),
        "setHours" => (ctx.intrinsics.date_set_hours, Type::I64),
        "setMinutes" => (ctx.intrinsics.date_set_minutes, Type::I64),
        "setSeconds" => (ctx.intrinsics.date_set_seconds, Type::I64),
        "setMilliseconds" => (ctx.intrinsics.date_set_milliseconds, Type::I64),
        _ => unreachable!(),
    }
}
