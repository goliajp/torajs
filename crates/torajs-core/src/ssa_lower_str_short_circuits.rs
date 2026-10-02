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
//! - `isWellFormed()` / `toWellFormed()` (ES2024 §22.1.3.10 / 30) —
//!   runtime scans for lone surrogates (a UTF-16 payload can hold
//!   them), after eval-and-dropping the trailing args.
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
        // Rotation 678 — §22.1.3.14 step 3 is ToString on the form,
        // so this slot takes every value: `'a'.normalize(null)` is
        // the kernel's RangeError ("null" is not a valid form) and
        // `'a'.normalize({toString(){return 'NFC'}})` normalizes. The
        // raw `lower_expr` that stood here handed a non-Str operand
        // to a helper whose ABI declares `Str`, so the checker gate
        // was its only protection. This short-circuit sits OUTSIDE
        // the argv walk's owned watermark, so the minted Str is
        // dropped here rather than parked.
        let mut form_owned: Option<(Operand, usize)> = None;
        // §7.1.17 ToString(null) is "null" -- a form the kernel then
        // rejects with a RangeError, which is what every engine
        // answers. It takes the literal rather than `coerce_to_str`,
        // which panics on a `Ptr` operand (it cannot tell null from
        // undefined there; the static type can).
        let null_form = !args.is_empty()
            && matches!(ctx.expr_types.get(&args[0]), Some(crate::check::Type::Null));
        let form_op = if args.is_empty() || undef_form {
            Operand::Value(ctx.intern_string_literal("NFC"))
        } else if null_form {
            Operand::Value(ctx.intern_string_literal("null"))
        } else {
            let raw = ctx.lower_expr(args[0]);
            let raw_ty = ctx.operand_ty(&raw);
            if matches!(raw_ty, Type::Str | Type::Substr) {
                raw
            } else {
                let coerced = ctx.coerce_to_str(raw, raw_ty);
                let raw_owned = raw_ty.is_refcounted() && ctx.expr_is_fresh_owned(args[0]);
                let raw_tok = raw_owned.then(|| ctx.push_throw_temp(raw, raw_ty));
                // The coerced Str is ours whether or not the user
                // toString threw (kernel placeholder contract), so it
                // rides the throw edge too.
                // It stays parked across the kernel call as well:
                // `normalize(null)` reaches the kernel and the kernel
                // RAISES (invalid form), so the throw edge after the
                // call has to drop it too.
                let coerced_tok = ctx.push_throw_temp(coerced, Type::Str);
                ctx.emit_throw_check(None);
                ctx.unpark_owned_temp(raw_tok);
                if raw_owned {
                    ctx.emit_drop_value(raw, raw_ty);
                }
                form_owned = Some((coerced, coerced_tok));
                coerced
            }
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
        if let Some((op, tok)) = form_owned {
            ctx.pop_throw_temp(tok);
            ctx.emit_drop_value(op, Type::Str);
        }
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
    // A UTF-16 payload can hold lone surrogates (`'a' + '\uD83D'`), so
    // both are runtime scans (torajs-str `well_formed`); trailing
    // arguments are evaluated and dropped first (S281).
    if method == "isWellFormed" || method == "toWellFormed" {
        for &a in args {
            let _ = ctx.lower_expr(a);
        }
        if method == "toWellFormed" {
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.str_to_well_formed, vec![recv_op]),
                Type::Str,
                None,
            );
            return Some(Operand::Value(v));
        }
        let r = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.str_is_well_formed, vec![recv_op]),
            Type::I64,
            None,
        );
        let b = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::ICmp(
                crate::ssa::IPred::Ne,
                Operand::Value(r),
                Operand::ConstI64(0),
            ),
            Type::Bool,
            None,
        );
        return Some(Operand::Value(b));
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
/// - anything else (an `any`, `null`, an object, a primitive) → the
///   same walk over the boxed value (`str_locale_case_any`, §9.2.1:
///   `null` throws, a primitive has no elements, an object is an
///   array-like).
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
        _ => {
            let op = ctx.lower_expr(args[0]);
            let boxed = ctx.box_to_any_from_expr(args[0], op);
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.str_locale_case_any,
                    vec![recv_op, boxed, Operand::ConstI64(upper as i64)],
                ),
                Type::Str,
                None,
            );
            ctx.release_owned_temp(args[0], &boxed);
            Some(Operand::Value(v))
        }
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
