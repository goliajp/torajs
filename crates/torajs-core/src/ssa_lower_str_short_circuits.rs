//! Str-receiver spec-default short-circuits — third sub-split
//! carved out of [`ssa_lower_str::try_lower_method_call`] along the
//! `ssa_lower_str/{view,search,transform,structural}.rs` axis.
//!
//! Encapsulates four Str-only pre-dispatch wedges that fold the
//! generic dispatch to a fast-path constant (or a single helper
//! call) before the giant per-method match-table fires:
//! - `search()` / `search(undefined)` (ES §22.1.3.20:
//!   `RegExpCreate(undefined, undefined)` matches at index 0) →
//!   `ConstI64(0)`, never enters `str_index_of` which would fold
//!   the undef operand to the literal `"undefined"`.
//! - `normalize(form?)` (ES §22.1.3.13: undefined form defaults to
//!   `"NFC"`) — calls `__torajs_str_normalize` with the interned
//!   default, drops any trailing args, and propagates the runtime
//!   throw on invalid forms.
//! - `isWellFormed()` (ES2024 §22.1.3.10) — torajs Str is internally
//!   UTF-8 + every reachable Str is well-formed by construction, so
//!   the answer is constant `true` after eval-and-dropping the
//!   trailing args for spec-compliant side-effect order.
//! - `toWellFormed()` (ES2024 §22.1.3.30) — identity on torajs Str
//!   (rc-inc + return self), same trailing-arg discipline.
//!
//! Returns `None` for non-Str receivers or unrelated methods so the
//! caller can keep trying the generic Str-path dispatch.

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// Try to lower `<Str>.<method>(args)` through the four spec-default
/// short-circuit wedges. Returns `Some(value)` when one fired;
/// `None` otherwise.
pub(crate) fn try_dispatch(
    ctx: &mut LowerCtx<'_>,
    method: &str,
    args: &[ExprId],
    recv_op: Operand,
    recv_ty: Type,
) -> Option<Operand> {
    if recv_ty != Type::Str {
        return None;
    }
    // P11.6-S4 — `s.normalize(form?)` per ES §22.1.3.13. Routes to
    // `__torajs_str_normalize(s, form)`. Default `form` is the
    // interned `"NFC"` literal (per ES step 2: form coerces to
    // "NFC" when undefined). The runtime intrinsic parses the form
    // string, raises `RangeError` via the cross-TU throw stub on
    // invalid forms, and otherwise returns a freshly-allocated Str
    // holding the normalized result. The per-call `emit_throw_check`
    // propagates the throw to the user's `try/catch` before the
    // result flows downstream.
    // S210 — `s.search()` / `s.search(undefined)` per ES §22.1.3.20:
    // RegExpCreate(undefined, undefined) yields an empty regex which
    // matches at index 0 in any string. Short-circuit before the
    // generic dispatch which routes search through str_index_of —
    // the indexOf path would default the missing arg to the literal
    // "undefined" and return -1 / first occurrence of "undefined",
    // not the spec'd 0.
    if method == "search"
        && (args.is_empty()
            || (args.len() == 1
                && matches!(
                    ctx.expr_types.get(&args[0]),
                    Some(crate::check::Type::Undefined)
                )))
    {
        return Some(Operand::ConstI64(0));
    }
    if method == "normalize" {
        // S208 — explicit `undefined` form follows the same
        // default-undefined rule per spec §22.1.3.13 step 1:
        // when `form` is undefined, default to "NFC". Detect
        // the typed-Undefined arg (same idiom S206/S207 use)
        // and route to the 0-arg path so the operand lower is
        // skipped (the literal has no side effects).
        // S240 widens the 1-arg detection to 2-arg so a trailing-undef
        // shape (normalize(undef, trailing)) still folds to "NFC" without
        // lowering the undef operand into the helper's Str slot.
        //
        // S272 — widen further to `args.len() >= 1` (any trailing count);
        // detect typed-Undefined at args[0] regardless of arg count, then
        // eval-and-drop args[1..] so trailing side-effect exprs fire.
        let undef_form = !args.is_empty()
            && matches!(
                ctx.expr_types.get(&args[0]),
                Some(crate::check::Type::Undefined)
            );
        let form_op = if args.is_empty() || undef_form {
            Operand::Value(ctx.intern_string_literal("NFC"))
        } else {
            ctx.lower_expr(args[0])
        };
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.str_normalize, vec![recv_op, form_op]),
            Type::Str,
            None,
        );
        for &a in args.iter().skip(1) {
            let _ = ctx.lower_expr(a);
        }
        ctx.emit_throw_check(None);
        return Some(Operand::Value(v));
    }
    // ES402 sup-string.prototype.tolocale{upper,lower}case —
    // `s.toLocaleUpperCase(locale?)` routes to the tailored runtime
    // kernels (tr/az/lt SpecialCasing; everything else takes the
    // default fold inside). Missing / undefined locale rides as the
    // interned "und" (valid tag, host-default tailoring, skips no
    // validation semantics — CanonicalizeLocaleList(undefined) is
    // the empty list). A Str locale is validated by the kernel
    // (RangeError on a structurally invalid tag); an Arr<Str>
    // locales list walks CanonicalizeLocaleList on the anyvalue
    // side. Any other locale type falls through to the generic
    // dispatch (number/bool coerce to a length-0 array-like =
    // host default there).
    if method == "toLocaleUpperCase" || method == "toLocaleLowerCase" {
        let upper = method == "toLocaleUpperCase";
        if let Some(v) = lower_locale_case(ctx, args, recv_op, upper) {
            for &a in args.iter().skip(1) {
                let _ = ctx.lower_expr(a);
            }
            ctx.emit_throw_check(None);
            return Some(v);
        }
        return None;
    }
    // ES2024 §22.1.3.10 / §22.1.3.30 — `isWellFormed()` / `toWellFormed()`.
    // torajs strings are internally UTF-8 so lone surrogates can't be
    // encoded at all: every reachable Str is well-formed by construction.
    // `isWellFormed` returns true; `toWellFormed` is the identity. Both
    // arms drop the receiver dependency at the caller per existing
    // method-call ownership rules.
    if method == "isWellFormed" {
        // S281 — lower-and-drop any trailing args for ES eval-then-
        // discard semantics (the early-return below skips the args
        // loop further down, so this site is the only opportunity).
        for &a in args {
            let _ = ctx.lower_expr(a);
        }
        return Some(Operand::ConstBool(true));
    }
    if method == "toWellFormed" {
        // S281 — same idiom as isWellFormed above.
        for &a in args {
            let _ = ctx.lower_expr(a);
        }
        ctx.emit_rc_inc(recv_op.clone());
        return Some(recv_op);
    }
    None
}

/// Lower a `toLocale{Upper,Lower}Case` call against an owned-Str
/// receiver operand. Dispatches on the locale argument's static
/// type:
/// - 0-arg / explicit-undefined → 2-arg kernel with the interned
///   `"und"` (valid tag whose tailoring is the host default),
/// - `Str` → 2-arg kernel (the kernel validates and raises
///   RangeError on a structurally invalid tag),
/// - `Arr<_>` → the anyvalue-side CanonicalizeLocaleList walk
///   (`str_locale_case_arr`),
/// - anything else → `None`; the caller falls through to the
///   generic locale-dropping dispatch (number/bool locales are a
///   length-0 array-like = host default).
///
/// The caller owns trailing-arg eval-and-drop plus the
/// `emit_throw_check`. Shared with the Substr-receiver dispatch in
/// [`crate::ssa_lower_str_substr_dispatch`].
pub(crate) fn lower_locale_case(
    ctx: &mut LowerCtx<'_>,
    args: &[ExprId],
    recv_op: Operand,
    upper: bool,
) -> Option<Operand> {
    let undef0 = !args.is_empty()
        && matches!(
            ctx.expr_types.get(&args[0]),
            Some(crate::check::Type::Undefined)
        );
    if args.is_empty() || undef0 {
        let und = Operand::Value(ctx.intern_string_literal("und"));
        return Some(emit_locale_call(ctx, recv_op, und, upper));
    }
    match ctx.expr_types.get(&args[0]) {
        Some(crate::check::Type::String) => {
            let locale_op = ctx.lower_expr(args[0]);
            Some(emit_locale_call(ctx, recv_op, locale_op, upper))
        }
        Some(crate::check::Type::Array(_)) => {
            let arr_op = ctx.lower_expr(args[0]);
            // The walk reads elements through the kind-aware anyv
            // path — an unescaped typed Arr must carry its
            // ElementsKind stamp or every slot reads as UNSET.
            ctx.emit_arr_mark_kind(&arr_op);
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.str_locale_case_arr,
                    vec![recv_op, arr_op, Operand::ConstI64(upper as i64)],
                ),
                Type::Str,
                None,
            );
            Some(Operand::Value(v))
        }
        _ => None,
    }
}

/// Append the 2-arg tailored-kernel call.
fn emit_locale_call(
    ctx: &mut LowerCtx<'_>,
    recv_op: Operand,
    locale_op: Operand,
    upper: bool,
) -> Operand {
    let target = if upper {
        ctx.intrinsics.str_to_locale_upper
    } else {
        ctx.intrinsics.str_to_locale_lower
    };
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(target, vec![recv_op, locale_op]),
        Type::Str,
        None,
    );
    Operand::Value(v)
}
