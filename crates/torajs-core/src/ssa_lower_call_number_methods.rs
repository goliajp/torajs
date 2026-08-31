//! `<recv>.{toFixed|toString|toLocaleString|toExponential|toPrecision}`
//! lowering pulled out of [`crate::ssa_lower::lower_expr_inner`]
//! `Expr::Call` dispatch as chunk-5 of the `Expr::Call` god-arm decomp
//! (chunks 1-4 = Arr higher-order + Map dispatch + Set dispatch +
//! Arr.push).
//!
//! 5 method names dispatched against 6 receiver-type wedges that fold
//! into 4 helpers:
//! - BigInt + toString (no-arg / radix / undef-radix per ES §6.1.6.2.13).
//! - Symbol / Bool + toString|toLocaleString (1-arg intrinsic returning
//!   Str — shared `emit_str_intrinsic`).
//! - Str / Substr + toString|toLocaleString (identity, just lower-and-drop
//!   trailing).
//! - I64 / F64 — the big numeric path: toString with radix (i64/f64
//!   variants routing to num_to_string_radix_{i,f}), toFixed / toExp /
//!   toPrecision / toLocaleString routing to the matching
//!   num_to_{fixed,exp,precision,locale}_{i,f} intrinsic with default-arg
//!   handling per spec §21.1.3.{3,5,6} and post-call throw_check for the
//!   RangeError path.
//!
//! Returns `Some(result)` when the callee matches the method-name set;
//! `None` lets the caller fall through to the next dispatch arm
//! (`Array.isArray(...)` and below).

use crate::ast::{Expr, ExprId};
use crate::check as check_mod;
use crate::ssa::{FuncId, InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// Try to lower `<recv>.{toFixed|toString|toLocaleString|toExponential
/// |toPrecision}(...)`. Returns `Some` when dispatched.
pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let (recv_id, m_name) = match ctx.ast.get_expr(callee) {
        Expr::Member { obj, name } => (*obj, name.clone()),
        _ => return None,
    };
    if !matches!(
        m_name.as_str(),
        "toFixed" | "toString" | "toLocaleString" | "toExponential" | "toPrecision"
    ) {
        return None;
    }
    // RFC 20260719-fn-tostring-source B6 — `Math.max.toString()`:
    // the namespace-static builtin fn member has no value form, so
    // the eager receiver lower below would reject it. Bail first;
    // the fn-tostring wedge downstream folds the JSC named native
    // form.
    if crate::ssa_lower_call_fn_tostring::namespace_static_native_form(ctx, recv_id).is_some() {
        return None;
    }
    let recv_op = ctx.lower_expr(recv_id);
    let recv_ty = ctx.operand_ty(&recv_op);
    // S229 — explicit-undefined 1-arg shape folds to the 0-arg defaults
    // per ES §21.1.3.{3,5,6,7}. Detect here so each downstream branch
    // (radix dispatch for toString, default args for toFixed / toExp /
    // toPrec) can short-circuit without lowering the ConstPtrNull undef
    // into a non-Ptr ABI. S244 widens the 1-arg arg0_is_undef gate to
    // >=1 so trailing-arg shapes still short-circuit the undef radix.
    let arg0_is_undef = !args.is_empty()
        && matches!(
            ctx.expr_types.get(&args[0]),
            Some(check_mod::Type::Undefined)
        );
    if recv_ty == Type::BigInt && m_name == "toString" {
        return Some(emit_bigint_to_string(ctx, recv_op, args));
    }
    // §Intl fallback — default-locale grouped decimal (the
    // `num_to_locale_i` posture); the checker admits the 0-arg form
    // only, so no locales/options lowering here.
    if recv_ty == Type::BigInt && m_name == "toLocaleString" {
        let cur_block = ctx.cur_block;
        let v = ctx.f.append_inst(
            cur_block,
            InstKind::Call(ctx.intrinsics.bigint_to_locale_string, vec![recv_op]),
            Type::Str,
            None,
        );
        return Some(Operand::Value(v));
    }
    if recv_ty == Type::Symbol && (m_name == "toString" || m_name == "toLocaleString") {
        return Some(emit_str_intrinsic(
            ctx,
            recv_op,
            args,
            ctx.intrinsics.symbol_to_str,
        ));
    }
    if recv_ty == Type::Bool && (m_name == "toString" || m_name == "toLocaleString") {
        return Some(emit_str_intrinsic(
            ctx,
            recv_op,
            args,
            ctx.intrinsics.bool_to_str,
        ));
    }
    if matches!(recv_ty, Type::Str | Type::Substr)
        && (m_name == "toString" || m_name == "toLocaleString")
    {
        // V3-18 wedge — String.prototype.toString / toLocaleString just
        // return the receiver per spec §22.1.3.27. Identity at the SSA
        // layer; lower-and-drop trailing args first so step()-style
        // side-effect exprs fire (S293 idiom).
        for &a in args.iter() {
            let _ = ctx.lower_expr(a);
        }
        // RFC 20260705 owned-result invariant: identity result carries
        // its own ref.
        ctx.emit_owned_result_inc(recv_op.clone(), recv_ty);
        return Some(recv_op);
    }
    if recv_ty == Type::I64 || recv_ty == Type::F64 {
        return Some(emit_numeric_method(
            ctx,
            &m_name,
            recv_op,
            recv_ty,
            args,
            arg0_is_undef,
        ));
    }
    // RFC 20260705 chunk 555 — the receiver is already lowered; park
    // the operand for the next cascade arm (single-eval).
    ctx.redispatch_lowered = Some((recv_id, recv_op));
    None
}

/// BigInt.prototype.toString — V3-18 m1.h.27 + P12.4 radix support per ES
/// §6.1.6.2.13. 0-arg / undef-radix → decimal `bigint_to_string`; explicit
/// radix → `bigint_to_string_radix` after FpToSi-coercing f64 radix to
/// i64 (caller is responsible for clamping; out-of-range RangeError throw
/// is a follow-up tracked in L3b).
fn emit_bigint_to_string(ctx: &mut LowerCtx<'_>, recv_op: Operand, args: &[ExprId]) -> Operand {
    if args.is_empty() {
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.bigint_to_string, vec![recv_op]),
            Type::Str,
            None,
        );
        return Operand::Value(v);
    }
    // S247 — explicit-undefined radix routes to the 0-arg default decimal
    // helper. Trailing args lower-and-drop after (S272 idiom).
    if matches!(
        ctx.expr_types.get(&args[0]),
        Some(check_mod::Type::Undefined)
    ) {
        for &a in args.iter().skip(1) {
            let _ = ctx.lower_expr(a);
        }
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.bigint_to_string, vec![recv_op]),
            Type::Str,
            None,
        );
        return Operand::Value(v);
    }
    let radix_op = ctx.lower_expr(args[0]);
    let radix_ty = ctx.operand_ty(&radix_op);
    let radix_i64 = match radix_ty {
        Type::I64 => radix_op,
        Type::F64 => Operand::Value(ctx.f.append_inst(
            ctx.cur_block,
            InstKind::FpToSi(radix_op),
            Type::I64,
            None,
        )),
        _ => radix_op,
    };
    // S294 — lower-and-drop trailing args past the 1 useful radix slot
    // (S272 idiom). check.rs S247 typecheck-and-drops args[1..].
    for &a in args.iter().skip(1) {
        let _ = ctx.lower_expr(a);
    }
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.bigint_to_string_radix,
            vec![recv_op, radix_i64],
        ),
        Type::Str,
        None,
    );
    Operand::Value(v)
}

/// Symbol / Bool + (toString|toLocaleString) shared shape: lower-and-drop
/// all trailing args (S293 idiom) then 1-arg call to a Str-returning
/// intrinsic (`symbol_to_str` / `bool_to_str`).
fn emit_str_intrinsic(
    ctx: &mut LowerCtx<'_>,
    recv_op: Operand,
    args: &[ExprId],
    intrinsic: FuncId,
) -> Operand {
    for &a in args.iter() {
        let _ = ctx.lower_expr(a);
    }
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(intrinsic, vec![recv_op]),
        Type::Str,
        None,
    );
    Operand::Value(v)
}

/// I64 / F64 numeric method dispatch. Splits into `emit_numeric_radix`
/// (toString with explicit radix) and the post-radix `emit_numeric_digits`
/// (toFixed / toExponential / toPrecision / toLocaleString + bare
/// `n.toString()` no-arg fast-path). throw_check fires post-call for
/// toFixed/toExp/toPrec per spec §21.1.3.{3,5,6} RangeError gate.
fn emit_numeric_method(
    ctx: &mut LowerCtx<'_>,
    m_name: &str,
    recv_op: Operand,
    recv_ty: Type,
    args: &[ExprId],
    arg0_is_undef: bool,
) -> Operand {
    let is_f64 = recv_ty == Type::F64;
    // S244 widens the 1-arg gate to >=1 so trailing-arg shapes still
    // route radix into num_to_string_radix_{i,f}.
    if m_name == "toString" && !args.is_empty() && !arg0_is_undef {
        return emit_numeric_radix(ctx, recv_op, args, is_f64);
    }
    emit_numeric_digits(ctx, m_name, recv_op, args, is_f64, arg0_is_undef)
}

/// `n.toString(radix)` for I64 (`num_to_string_radix_i`) / F64
/// (`num_to_string_radix_f`) per JS spec §21.1.3.6 / §6.1.6.1.13. Runtime
/// helper records RangeError via TLS if radix is outside `[2, 36]`;
/// `emit_throw_check` propagates here.
fn emit_numeric_radix(
    ctx: &mut LowerCtx<'_>,
    recv_op: Operand,
    args: &[ExprId],
    is_f64: bool,
) -> Operand {
    // §21.1.3.6 step 2 ToIntegerOrInfinity(radix) — the helper's
    // radix param is i64, and a raw `lower_expr` handed it whatever
    // the operand was. `undefined` is not this coercion's NaN: it
    // means radix 10 (`(255).toString(undefined)` is "255") where
    // `toString(NaN)` is a RangeError, so an any box is asked for its
    // tag. The statically-undefined spelling never arrives — the
    // `arg0_is_undef` short-circuit above routes it to the 0-arg
    // decimal path.
    let radix = ctx.lower_to_index_or_undef_default(args[0], Operand::ConstI64(10), "__radix_slot");
    // S294 — lower-and-drop trailing args past the 1 useful radix slot
    // (S272 idiom). check.rs S244 typecheck-and-drops args[1..].
    for &a in args.iter().skip(1) {
        let _ = ctx.lower_expr(a);
    }
    let intrinsic = if is_f64 {
        ctx.intrinsics.num_to_string_radix_f
    } else {
        ctx.intrinsics.num_to_string_radix_i
    };
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(intrinsic, vec![recv_op, radix]),
        Type::Str,
        None,
    );
    ctx.emit_throw_check(None);
    Operand::Value(v)
}

/// toFixed / toExponential / toPrecision / toLocaleString / bare-toString:
/// pick the matching i/f intrinsic, build argv with default-arg handling
/// per spec §21.1.3.{3,5,6}, fire post-call throw_check for the
/// digits-RangeError methods.
fn emit_numeric_digits(
    ctx: &mut LowerCtx<'_>,
    m_name: &str,
    recv_op: Operand,
    args: &[ExprId],
    is_f64: bool,
    arg0_is_undef: bool,
) -> Operand {
    let target = pick_digits_intrinsic(ctx, m_name, is_f64);
    let mut argv = vec![recv_op];
    // S139 — toLocaleString ignores any locale/options arg in tr's subset
    // (always en-US). Drop user args so the runtime helper's 1-arg
    // signature matches; the spec allows ignoring locale args outside
    // Intl-enabled hosts.
    let pass_args = !matches!(m_name, "toLocaleString");
    if pass_args && !arg0_is_undef {
        // S254 — toFixed / toExponential / toPrecision read only args[0]
        // per ES §21.1.3.{3,5,6}. Trailing args dropped at lower-time
        // (check.rs's carve-out already type_of'd them). toString's radix
        // path uses the dedicated pre-dispatch above so it never reaches
        // here with >1 args.
        if let Some(&a0) = args.first() {
            // §21.1.3.{3,5,6} step 1 is `ToIntegerOrInfinity(digits)`,
            // so the operand is COERCED, not shape-checked: a Str
            // routes through the runtime's ToNumber, and `coerce_to_i64`
            // performs the ToInteger fold the helpers' i64 `digits`
            // parameter needs — including `toFixed(1.1)`, which used to
            // hand an f64 straight to an i64 ABI.
            let n = ctx.lower_to_number_operand(a0);
            let d = ctx.coerce_to_i64(n);
            argv.push(d);
        }
    }
    // S294 — lower-and-drop trailing args past the first slot routed
    // into argv per ES trailing-arg ignore (S272 idiom). For
    // toLocaleString (pass_args=false) drop ALL user args; for
    // toFixed/toExp/toPrec (pass_args=true) drop args[1..] since args[0]
    // was already routed into argv.
    let drop_start = if pass_args && !arg0_is_undef && !args.is_empty() {
        1
    } else {
        0
    };
    for &a in args.iter().skip(drop_start) {
        let _ = ctx.lower_expr(a);
    }
    // V3-18 m1.h.46 — toFixed / toExponential / toPrecision with no arg.
    // toFixed defaults digits to 0; toPrecision routes through ToString
    // (toPrecision(0) is outside §21.1.3.6's `1 ≤ p ≤ 100` range and
    // produces a bogus result for no-arg); toExponential passes the
    // i64::MIN sentinel (unreachable as user literal — TS won't accept
    // it, and Number.MIN_SAFE_INTEGER truncates differently via FpToSi)
    // so the runtime distinguishes no-arg "shortest" from
    // toExponential(0)'s 1-sig-digit mantissa.
    if m_name == "toFixed" && (args.is_empty() || arg0_is_undef) {
        argv.push(Operand::ConstI64(0));
    }
    if m_name == "toPrecision" && (args.is_empty() || arg0_is_undef) {
        let to_str_intrinsic = if is_f64 {
            ctx.intrinsics.f64_to_str
        } else {
            ctx.intrinsics.i64_to_str
        };
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(to_str_intrinsic, vec![recv_op]),
            Type::Str,
            None,
        );
        return Operand::Value(v);
    }
    if m_name == "toExponential" && (args.is_empty() || arg0_is_undef) {
        argv.push(Operand::ConstI64(i64::MIN));
    }
    let v = ctx
        .f
        .append_inst(ctx.cur_block, InstKind::Call(target, argv), Type::Str, None);
    // ES §22.1.3.{5,32} step 3 — toFixed(digits) / toExponential(digits)
    // / toPrecision(digits) throw RangeError when digits is outside the
    // spec range. The runtime helpers record the throw via TLS; propagate
    // here.
    if matches!(m_name, "toFixed" | "toExponential" | "toPrecision") {
        ctx.emit_throw_check(None);
    }
    Operand::Value(v)
}

/// Pick the i / f intrinsic for the given method name. toLocaleString
/// matches the canonical bun output on macOS without an Intl substrate
/// (S139); locale-arg variants are a separate Intl item. Bare toString
/// routes to the same formatters powering Number-to-String coercion in
/// `+` (i64_to_str / f64_to_str).
fn pick_digits_intrinsic(ctx: &LowerCtx<'_>, m_name: &str, is_f64: bool) -> FuncId {
    match m_name {
        "toFixed" if is_f64 => ctx.intrinsics.num_to_fixed_f,
        "toFixed" => ctx.intrinsics.num_to_fixed_i,
        "toExponential" if is_f64 => ctx.intrinsics.num_to_exp_f,
        "toExponential" => ctx.intrinsics.num_to_exp_i,
        "toPrecision" if is_f64 => ctx.intrinsics.num_to_precision_f,
        "toPrecision" => ctx.intrinsics.num_to_precision_i,
        "toLocaleString" if is_f64 => ctx.intrinsics.num_to_locale_f,
        "toLocaleString" => ctx.intrinsics.num_to_locale_i,
        // bare toString — i64_to_str / f64_to_str.
        _ if is_f64 => ctx.intrinsics.f64_to_str,
        _ => ctx.intrinsics.i64_to_str,
    }
}
