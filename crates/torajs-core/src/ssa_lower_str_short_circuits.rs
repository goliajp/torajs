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
    // `s.toLocaleUpperCase(locale?)` routes to the 2-arg tailored
    // runtime kernel (tr/az/lt SpecialCasing; everything else takes
    // the default fold inside). Missing / undefined locale rides as
    // the interned empty string (host default). A non-Str locale
    // arg (e.g. an Arr of identifiers) falls through to the generic
    // dispatch (CanonicalizeLocaleList walk is the follow-up cut).
    if method == "toLocaleUpperCase" || method == "toLocaleLowerCase" {
        if let Some(locale_op) = lower_locale_arg(ctx, args) {
            let target = if method == "toLocaleUpperCase" {
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
            for &a in args.iter().skip(1) {
                let _ = ctx.lower_expr(a);
            }
            return Some(Operand::Value(v));
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

/// Lower the `toLocale{Upper,Lower}Case` locale argument. Answers:
/// - `Some(interned "")` for the 0-arg / explicit-undefined shape
///   (host default),
/// - `Some(lowered)` when the static type of `args[0]` is `Str`,
/// - `None` for anything else (Arr / Any locale lists) — the caller
///   falls through to the generic dispatch until the
///   CanonicalizeLocaleList cut lands.
///
/// Shared with the Substr-receiver dispatch in
/// [`crate::ssa_lower_str_substr_dispatch`].
pub(crate) fn lower_locale_arg(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Option<Operand> {
    let undef0 = !args.is_empty()
        && matches!(
            ctx.expr_types.get(&args[0]),
            Some(crate::check::Type::Undefined)
        );
    if args.is_empty() || undef0 {
        return Some(Operand::Value(ctx.intern_string_literal("")));
    }
    if matches!(
        ctx.expr_types.get(&args[0]),
        Some(crate::check::Type::String)
    ) {
        return Some(ctx.lower_expr(args[0]));
    }
    None
}
