//! Generic `<Str>.<method>(args)` dispatch — fourth (and bulkiest)
//! sub-split carved out of [`ssa_lower_str::try_lower_method_call`]
//! along the `ssa_lower_str/{view,search,transform,structural}.rs`
//! axis.
//!
//! Encapsulates the giant per-method dispatch that fires after the
//! Substr-receiver / charAt-family / Str spec-default short-circuit
//! wedges have rejected a call. Handles every supported String stdlib
//! method on a `Type::Str` receiver — slice / substring / substr /
//! charCodeAt / codePointAt / startsWith / endsWith / includes /
//! indexOf / lastIndexOf / search / split / repeat /
//! toUpper/LowerCase + locale aliases / trim variants / padStart /
//! padEnd / replace / replaceAll / at / localeCompare — including all
//! the undef-slot / Any-decode / trailing-arg ignore / spec-default
//! filler wedges those methods need before reaching their backing
//! `__torajs_str_*` intrinsic.
//!
//! Returns `None` for non-Str receivers or methods outside the dispatch
//! allowlist so the caller can fall through to the remaining branches.

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// Try to lower `<Str>.<method>(args)` through the generic Str
/// dispatch. Returns `Some(value)` when the call was handled; `None`
/// otherwise so the caller keeps trying the remaining branches.
pub(crate) fn try_dispatch(
    ctx: &mut LowerCtx<'_>,
    method: &str,
    args: &[ExprId],
    recv_op: Operand,
    recv_ty: Type,
) -> Option<Operand> {
    if recv_ty == Type::Str
        && matches!(
            method,
            "slice"
                | "substring"
                | "substr"
                | "charCodeAt"
                | "codePointAt"
                | "startsWith"
                | "endsWith"
                | "includes"
                | "indexOf"
                | "split"
                | "repeat"
                | "toUpperCase"
                | "toLowerCase"
                | "toLocaleUpperCase"
                | "toLocaleLowerCase"
                | "trim"
                | "trimStart"
                | "trimEnd"
                | "trimLeft"
                | "trimRight"
                | "padStart"
                | "padEnd"
                | "replace"
                | "replaceAll"
                | "at"
                | "lastIndexOf"
                | "localeCompare"
                | "search"
        )
    {
        let mut argv = Vec::with_capacity(args.len() + 1);
        argv.push(recv_op);
        // Per-arg lowering with the full spec-carve-out tower
        // (S140 drop_args / S207-S338 undef + Any substitutions /
        // trailing-arg ignore). Documented in
        // `ssa_lower_str_str_argv`.
        crate::ssa_lower_str_str_argv::populate_argv(ctx, method, args, recv_op, &mut argv);
        // Append missing positional slots into argv to match each
        // Str method's runtime helper ABI. Spec carve-outs and
        // S-number traces documented in `ssa_lower_str_str_defaults`.
        crate::ssa_lower_str_str_defaults::fill_missing(ctx, method, args, recv_op, &mut argv);
        // V3-18 m1.h.50 — String.indexOf / lastIndexOf
        // with the 2-arg (needle, fromIndex) shape route
        // to the dedicated _from runtime helpers.
        if matches!(method, "indexOf" | "lastIndexOf") && args.len() >= 2 {
            let target = if method == "indexOf" {
                ctx.intrinsics.str_index_of_from
            } else {
                ctx.intrinsics.str_last_index_of_from
            };
            let v = ctx
                .f
                .append_inst(ctx.cur_block, InstKind::Call(target, argv), Type::I64, None);
            return Some(Operand::Value(v));
        }
        // V3-18 m1.h.51 — startsWith / endsWith / includes
        // 2-arg (needle, position) shape: route to
        // dedicated _from helpers.
        if matches!(method, "startsWith" | "endsWith" | "includes") && args.len() >= 2 {
            let target = match method {
                "startsWith" => ctx.intrinsics.str_starts_with_from,
                "endsWith" => ctx.intrinsics.str_ends_with_from,
                _ => ctx.intrinsics.str_includes_from,
            };
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(target, argv),
                Type::Bool,
                None,
            );
            return Some(Operand::Value(v));
        }
        let (target, ret_ty) = match method {
            "slice" => (ctx.intrinsics.str_slice, Type::Str),
            "substring" => (ctx.intrinsics.str_substring, Type::Str),
            "substr" => (ctx.intrinsics.str_substr, Type::Str),
            "repeat" => (ctx.intrinsics.str_repeat, Type::Str),
            "toUpperCase" | "toLocaleUpperCase" => (ctx.intrinsics.str_to_upper, Type::Str),
            "toLowerCase" | "toLocaleLowerCase" => (ctx.intrinsics.str_to_lower, Type::Str),
            "trim" => (ctx.intrinsics.str_trim, Type::Str),
            "trimStart" | "trimLeft" => (ctx.intrinsics.str_trim_start, Type::Str),
            "trimEnd" | "trimRight" => (ctx.intrinsics.str_trim_end, Type::Str),
            "padStart" => (ctx.intrinsics.str_pad_start, Type::Str),
            "padEnd" => (ctx.intrinsics.str_pad_end, Type::Str),
            "replace" => (ctx.intrinsics.str_replace, Type::Str),
            "replaceAll" => (ctx.intrinsics.str_replace_all, Type::Str),
            "at" => (ctx.intrinsics.str_at, Type::Str),
            // P11.3-A1 — `codePointAt` combines surrogate pairs
            // per ES §22.1.3.3; `charCodeAt` returns the lone
            // UTF-16 code unit per §22.1.3.2. Two distinct
            // intrinsics now that P11.1-S1/S5 gave us proper
            // hybrid Latin-1 / UTF-16 storage.
            "charCodeAt" => (ctx.intrinsics.str_char_code_at, Type::I64),
            "codePointAt" => (ctx.intrinsics.str_code_point_at, Type::I64),
            "startsWith" => (ctx.intrinsics.str_starts_with, Type::Bool),
            "endsWith" => (ctx.intrinsics.str_ends_with, Type::Bool),
            "includes" => (ctx.intrinsics.str_includes, Type::Bool),
            "indexOf" => (ctx.intrinsics.str_index_of, Type::I64),
            // V3-18 wedge — String.prototype.search per
            // JS spec §22.1.3.16 with a string arg
            // (the RegExp arg form is a follow-up
            // substrate item alongside Symbol.search
            // dispatch). For a plain string the result
            // is exactly indexOf — first match position
            // or -1 — so route through the same helper.
            "search" => (ctx.intrinsics.str_index_of, Type::I64),
            "lastIndexOf" => (ctx.intrinsics.str_last_index_of, Type::I64),
            "localeCompare" => (ctx.intrinsics.str_locale_compare, Type::I64),
            "split" => {
                return Some(crate::ssa_lower_str_str_split::lower_split(ctx, args, argv));
            }
            _ => unreachable!(),
        };
        let v = ctx
            .f
            .append_inst(ctx.cur_block, InstKind::Call(target, argv), ret_ty, None);
        // ES §22.1.3.16 step 4 — `s.repeat(n < 0)` throws RangeError.
        // Runtime sets a TLS pending throw; emit_throw_check propagates.
        if method == "repeat" {
            ctx.emit_throw_check(None);
        }
        return Some(Operand::Value(v));
    }
    None
}
