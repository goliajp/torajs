//! Bare-name JS globals dispatch — `parseInt`, `parseFloat`,
//! `isNaN`, `isFinite`, `queueMicrotask` — pulled out of
//! [`crate::ssa_lower::lower_expr_inner`] `Expr::Call` dispatch as chunk-6
//! of the `Expr::Call` god-arm decomp (chunks 1-5 = Arr higher-order +
//! Map dispatch + Set dispatch + Arr.push + Number method dispatch).
//!
//! All five callees share the same `Expr::Call { callee: Expr::Ident(name) }`
//! shape — co-located in one sibling. The two `parseX` paths share an
//! `Any → Str` decode wedge (S336/S337 family) folded into
//! `decode_any_to_str`; everything else stays per-method body for clarity.
//!
//! Returns `Some(result)` when the callee matches one of the bare-name
//! globals; `None` lets the caller fall through to the `Date.parse(undef)`
//! / `JSON.stringify` / `Math.hypot` / generic method dispatch arms below.

use crate::ast::{Expr, ExprId};
use crate::check as check_mod;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// Try to lower a bare-name JS global call. Returns `Some` when dispatched.
pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let name = match ctx.ast.get_expr(callee) {
        Expr::Ident(n) => n.clone(),
        _ => return None,
    };
    match name.as_str() {
        "parseInt" => Some(lower_parse_int(ctx, args)),
        "parseFloat" => Some(lower_parse_float(ctx, args)),
        "isNaN" | "isFinite" => Some(lower_is_nan_or_finite(ctx, name.as_str(), args)),
        "queueMicrotask" => Some(lower_queue_microtask(ctx, args)),
        "encodeURI" => Some(lower_uri(ctx, args, true, false)),
        "encodeURIComponent" => Some(lower_uri(ctx, args, true, true)),
        "decodeURI" => Some(lower_uri(ctx, args, false, false)),
        "decodeURIComponent" => Some(lower_uri(ctx, args, false, true)),
        _ => None,
    }
}

/// §19.2.6 — the four URI globals share one shape: ToString the
/// argument (step 1), run the Encode / Decode kernel, propagate the
/// malformed-URI URIError. The omitted / explicit-undefined argument
/// folds to the literal "undefined" (ToString(undefined)), which
/// both kernels map to itself — no escape, no `%`.
fn lower_uri(ctx: &mut LowerCtx<'_>, args: &[ExprId], encode: bool, component: bool) -> Operand {
    if args.is_empty()
        || matches!(
            ctx.expr_types.get(&args[0]),
            Some(check_mod::Type::Undefined)
        )
    {
        for &a in args.iter().skip(1) {
            let _ = ctx.lower_expr(a);
        }
        let undef_str = ctx.intrinsics.undefined_to_str;
        let cur_block = ctx.cur_block;
        let v = ctx.f.append_inst(
            cur_block,
            InstKind::Call(undef_str, vec![]),
            Type::Str,
            None,
        );
        return Operand::Value(v);
    }
    let raw = ctx.lower_expr(args[0]);
    let s = decode_any_to_str(ctx, raw);
    for &a in args.iter().skip(1) {
        let _ = ctx.lower_expr(a);
    }
    let kernel = if encode {
        ctx.intrinsics.str_uri_encode
    } else {
        ctx.intrinsics.str_uri_decode
    };
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(kernel, vec![s, Operand::ConstI64(component as i64)]),
        Type::Str,
        None,
    );
    ctx.emit_throw_check(None);
    Operand::Value(v)
}

/// S336/S337 — decode an Any operand to a concrete Str via
/// `anyv_to_str_pair` (tag + value) → `any_to_str`. Mirrors the shape the
/// `Number()` ctor and `Number.parseInt` / `Number.parseFloat` namespace
/// paths use. When the input is already Str the raw operand is returned
/// unchanged.
fn decode_any_to_str(ctx: &mut LowerCtx<'_>, raw: Operand) -> Operand {
    let raw_ty = ctx.operand_ty(&raw);
    // Rotation 461 — §19.2.5 step 1 is ToString, which takes any
    // value. A shape that is neither a string slot nor already boxed
    // (a struct an `as any` cast left statically typed, an array)
    // rides the same decode through a box; the box is a pure encode,
    // so the source keeps its own stake.
    let raw = if matches!(raw_ty, Type::Any | Type::Str | Type::Substr) {
        raw
    } else {
        ctx.box_to_any(raw)
    };
    if ctx.operand_ty(&raw) != Type::Any {
        return raw;
    }
    let tag = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.any_unbox_tag, vec![raw.clone()]),
        Type::I64,
        None,
    );
    let val = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.any_unbox_value, vec![raw.clone()]),
        Type::I64,
        None,
    );
    let s_val = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.any_to_str,
            vec![Operand::Value(tag), Operand::Value(val)],
        ),
        Type::Str,
        None,
    );
    // any_to_str only borrowed the pair — reclaim a ShortStr-
    // materialized temp (no-op otherwise).
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.any_unbox_settle,
            vec![raw, Operand::Value(val)],
        ),
    );
    // OrdinaryToPrimitive can record a catchable TypeError (a user
    // toString threw / both hooks answered objects) — propagate to
    // the caller's try/catch instead of leaking the pending throw
    // (0-check audit, rotation 130 L3b).
    ctx.emit_throw_check(None);
    Operand::Value(s_val)
}

pub(crate) fn lower_parse_int(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Operand {
    // S202 — ES §19.2.5 step 1: `string` defaults to undefined when omitted
    // → ToString returns "undefined" which fails to parse as a digit string
    // → NaN. S226 — explicit-undefined arg folds the same way without
    // lowering the arg.
    if args.is_empty()
        || matches!(
            ctx.expr_types.get(&args[0]),
            Some(check_mod::Type::Undefined)
        )
    {
        // S313 — short-circuit path still must evaluate trailing args[1..]
        // (ES spec left-to-right eval), so lower them for side-effects
        // before returning NaN.
        for &a in args.iter().skip(1) {
            let _ = ctx.lower_expr(a);
        }
        return Operand::ConstF64(f64::NAN);
    }
    // S337 — bare-name parseInt accepts Any per check.rs widen: decode Any
    // str via anyv_to_str_pair (tag + value) → any_to_str so the helper's
    // (Str, i64) -> F64 ABI receives a concrete Str. Sister to S336
    // (parseFloat bare-name Any).
    let raw = ctx.lower_expr(args[0]);
    let s = decode_any_to_str(ctx, raw);
    // V3-18 m1.h.25 — when no radix is supplied, pass 0 to trigger the
    // runtime's auto-detect path (10 by default, 16 if "0x" / "0X" prefix).
    // Spec §19.2.5: parseInt without a radix infers from the prefix.
    //
    // S234 — accept explicit-undefined radix per §19.2.5.1 step 2-3:
    // ToInt32(undefined)=0 → same auto-detect sentinel. Skip lowering the
    // undef arg so its ConstPtrNull never reaches the helper's i64 radix
    // slot.
    // S337 — step 2 is ToInt32(radix), settled for both spellings in
    // `ssa_lower_parse_int_radix`.
    let radix_undef = args.len() >= 2
        && matches!(
            ctx.expr_types.get(&args[1]),
            Some(check_mod::Type::Undefined)
        );
    let r = if args.len() >= 2 && !radix_undef {
        let raw_r = ctx.lower_expr(args[1]);
        crate::ssa_lower_parse_int_radix::to_int32(ctx, args[1], raw_r)
    } else {
        Operand::ConstI64(0)
    };
    // S313 — lower-and-drop trailing args[2..] per S272 idiom so step()-style
    // side-effect exprs fire. Same fix the Number.parseInt namespace path
    // got in S302; the global path had been silent-drop until now.
    for &a in args.iter().skip(2) {
        let _ = ctx.lower_expr(a);
    }
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.num_parse_int, vec![s, r]),
        Type::F64,
        None,
    );
    Operand::Value(v)
}

fn lower_parse_float(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Operand {
    // S202 — same default-undefined rule per §19.2.4: missing string → NaN.
    // S226 — explicit-undefined arg folds the same way without lowering the
    // arg.
    if args.is_empty()
        || matches!(
            ctx.expr_types.get(&args[0]),
            Some(check_mod::Type::Undefined)
        )
    {
        // S313 — short-circuit path still must evaluate trailing args[1..]
        // (ES spec left-to-right eval).
        for &a in args.iter().skip(1) {
            let _ = ctx.lower_expr(a);
        }
        return Operand::ConstF64(f64::NAN);
    }
    // S336 — bare-name parseFloat accepts Any per check.rs widen: decode
    // Any via anyv_to_str_pair (tag + value) → any_to_str so the helper's
    // (Str) -> F64 ABI receives a concrete Str. Sister to S330 (Number.
    // parseFloat member method same shape).
    let raw = ctx.lower_expr(args[0]);
    let s = decode_any_to_str(ctx, raw);
    // S313 — lower-and-drop trailing args[1..] per S272 idiom so step()-style
    // side-effect exprs fire. Same fix Number.parseFloat namespace path got
    // in S302.
    for &a in args.iter().skip(1) {
        let _ = ctx.lower_expr(a);
    }
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.num_parse_float, vec![s]),
        Type::F64,
        None,
    );
    Operand::Value(v)
}

fn lower_is_nan_or_finite(ctx: &mut LowerCtx<'_>, name: &str, args: &[ExprId]) -> Operand {
    // S202 — spec §19.2.3 / §19.2.4 step 1 applies ToNumber to the arg
    // before the predicate test; ToNumber(undefined) = NaN so isNaN(NaN)
    // = true and isFinite(NaN) = false are the spec-fixed 0-arg answers.
    // S305 — lower-and-drop trailing args[1..] per S272 idiom so step()-
    // style side-effect exprs fire (check.rs S252 already typecheck-dropped;
    // SSA-emit reads only args[0]).
    for &a in args.iter().skip(1) {
        let _ = ctx.lower_expr(a);
    }
    if args.is_empty() {
        return Operand::ConstBool(name == "isNaN");
    }
    // S279 — explicit `undefined` arg per ES §19.2.3 / §19.2.4: ToNumber
    // (undefined) = NaN, so isNaN → true and isFinite → false. Same
    // short-circuit shape as parseFloat above — without it the Undefined
    // value (i64 sentinel 0) falls into the I64 arm below and dispatches
    // to num_is_nan_i / num_is_finite_i which silently return the
    // *opposite* (false / true), mirroring `isNaN(0)` / `isFinite(0)`
    // rather than `isNaN(NaN)` / `isFinite(NaN)`. Pre-S279 tora was the
    // only silent-wrong path here.
    if matches!(
        ctx.expr_types.get(&args[0]),
        Some(check_mod::Type::Undefined)
    ) {
        return Operand::ConstBool(name == "isNaN");
    }
    let arg_op = ctx.lower_expr(args[0]);
    let arg_ty = ctx.operand_ty(&arg_op);
    // V3-18 wedge — global isNaN / isFinite apply ToNumber per JS spec
    // §19.2.3 / §19.2.4. Reuse the same coerce paths Number(x) ctor takes
    // (m1.h.9 / m1.f). After coercion the arg is always F64 (or i64 for
    // true bools / int literals); dispatch on the post-coerce SSA type.
    let coerced = match arg_ty {
        Type::I64 | Type::F64 | Type::I32 => arg_op,
        Type::Bool => ctx.coerce_bool_to_i64(arg_op),
        Type::Ptr if matches!(arg_op, Operand::ConstPtrNull) => {
            // ToNumber(null) = +0
            Operand::ConstI64(0)
        }
        Type::Any => {
            // S343 — global `isNaN(Any)` / `isFinite(Any)` per ES
            // §19.2.{3,4} step 1: ToNumber accepts arbitrary-typed input.
            // Pre-S343 Any fell into the catch-all and returned
            // ConstBool(name == "isNaN") — silent-wrong vs bun when the Any
            // boxed a real Number (`const a: any = 42; isNaN(a)` → bun:
            // false, tr: true). Route through anyv_to_number — same helper
            // the Number() ctor and S327/S330/S336/S342 family use.
            let f = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.any_to_number, vec![arg_op.clone()]),
                Type::F64,
                None,
            );
            // ES §19.2.{3,4} step 1 "? ToNumber" — an abrupt completion
            // from valueOf/toString on an Any-boxed dynobj (test262
            // `isFinite|isNaN/return-abrupt-from-tonumber-number.js`)
            // must propagate; without this check the pending throw was
            // stashed while a NaN garbage value fed the predicate and
            // downstream consumers, ending in SIGSEGV.
            ctx.emit_throw_check(None);
            if ctx.expr_is_fresh_owned(args[0]) {
                ctx.emit_drop_value(arg_op, Type::Any);
            }
            Operand::Value(f)
        }
        Type::Str | Type::Substr => {
            // Drop responsibility: fresh-owned strings (concat results)
            // dec after the helper reads them. `str_to_number` borrows
            // its arg (reads len + bytes, no rc traffic), so an Ident
            // source keeps its own stake and its normal scope drop —
            // the old consume path orphaned that stake (RFC 20260705
            // ledger #3, 32B/iter probe).
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.str_to_number, vec![arg_op.clone()]),
                Type::F64,
                None,
            );
            if ctx.expr_is_fresh_owned(args[0]) {
                ctx.emit_drop_value(arg_op, arg_ty);
            }
            Operand::Value(v)
        }
        _ => {
            // Object / array / closure — spec §7.1.4 ToNumber invokes
            // OrdinaryToPrimitive → valueOf / toString, and either may
            // throw (test262 `isFinite|isNaN/return-abrupt-from-
            // tonumber-number.js`: `let obj = { valueOf: () => throw
            // Test262Error() }` typechecks as a struct, so it hit this
            // arm and the static ConstBool fold skipped the abrupt
            // completion → assert.throws saw !threw → the harness
            // reported the opposite verdict). Route typed
            // reference-shape args through box_to_any + any_to_number
            // + throw_check so the spec's abrupt path propagates; the
            // common no-valueOf-no-toString struct still resolves to
            // NaN inside any_to_number, so the aggregate answer
            // matches the prior static fold (isFinite = false / isNaN
            // = true).
            if crate::ssa_lower_object_define::is_typed_object(arg_ty) {
                let boxed = ctx.box_to_any_from_expr(args[0], arg_op.clone());
                let f = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Call(ctx.intrinsics.any_to_number, vec![boxed]),
                    Type::F64,
                    None,
                );
                ctx.emit_throw_check(None);
                if arg_ty.is_refcounted() && ctx.expr_is_fresh_owned(args[0]) {
                    ctx.emit_drop_value(arg_op, arg_ty);
                }
                Operand::Value(f)
            } else {
                // Truly non-coercible (Void / other primitives that
                // never reach OrdinaryToPrimitive): keep the static
                // fold, spec answer is NaN.
                if arg_ty.is_refcounted() && ctx.expr_is_fresh_owned(args[0]) {
                    ctx.emit_drop_value(arg_op, arg_ty);
                }
                return Operand::ConstBool(name == "isNaN");
            }
        }
    };
    let coerced_ty = ctx.operand_ty(&coerced);
    let target = match (name, coerced_ty) {
        ("isNaN", Type::F64) => ctx.intrinsics.num_is_nan_f,
        ("isNaN", _) => ctx.intrinsics.num_is_nan_i,
        ("isFinite", Type::F64) => ctx.intrinsics.num_is_finite_f,
        ("isFinite", _) => ctx.intrinsics.num_is_finite_i,
        _ => unreachable!(),
    };
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(target, vec![coerced]),
        Type::Bool,
        None,
    );
    Operand::Value(v)
}

fn lower_queue_microtask(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Operand {
    // P10.1-A1/A1.1 — queueMicrotask(cb). cb is `() => void` typed by
    // check.rs as Type::Function([], Void). At SSA layer it lowers to either
    // Type::Closure (capturing lambda — env+8 fn_addr) or Type::FnSig
    // (named fn declaration — raw fn ptr). Dispatch on cb's static type,
    // mirroring promise_then_{simple,closure}.
    //
    // Return shape: queueMicrotask returns undefined per WHATWG HTML
    // §queueMicrotask; we model Void via ConstI64(0) like other Type::Void
    // call expressions.
    let cb_op = ctx.lower_expr(args[0]);
    // S323 — lower-and-drop trailing args[1..] per S272 idiom so step()-style
    // side-effect exprs fire per Web IDL §3.2.1 over-arity ignore (check.rs
    // S323 already typecheck-dropped).
    for &a in args.iter().skip(1) {
        let _ = ctx.lower_expr(a);
    }
    let cb_ty = ctx.operand_ty(&cb_op);
    let intrinsic = match cb_ty {
        Type::Closure(_) => ctx.intrinsics.microtask_enqueue_closure,
        Type::FnSig(_) => ctx.intrinsics.microtask_enqueue_simple,
        _ => panic!("ssa-lower: queueMicrotask cb expected Closure or FnSig, got {cb_ty:?}"),
    };
    ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(intrinsic, vec![cb_op]),
        Type::Void,
        None,
    );
    Operand::ConstI64(0)
}
