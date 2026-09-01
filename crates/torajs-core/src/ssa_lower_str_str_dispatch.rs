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
use crate::ssa::{Operand, Type};
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
        // RC-4 replace A1_T4 — `s.{replace|replaceAll}(pat, cb)`
        // functional-replaceValue lane claims the call before the
        // (Str, Str, Str) argv tower below misroutes the closure
        // into a Str param slot.
        if let Some(op) = crate::ssa_lower_str_replace_fn::try_lower(ctx, method, args, recv_op) {
            return Some(op);
        }
        let mut argv = Vec::with_capacity(args.len() + 1);
        argv.push(recv_op);
        // RFC 20260712 chunk B — fresh-owned argv operands parked
        // past this watermark drop after the helper call (nested
        // str-method args keep their own window).
        let owned_base = ctx.temps.argv_owned.len();
        // Per-arg lowering with the full spec-carve-out tower
        // (S140 drop_args / S207-S338 undef + Any substitutions /
        // trailing-arg ignore). Documented in
        // `ssa_lower_str_str_argv`.
        crate::ssa_lower_str_str_argv::populate_argv(ctx, method, args, recv_op, &mut argv);
        // Append missing positional slots into argv to match each
        // Str method's runtime helper ABI. Spec carve-outs and
        // S-number traces documented in `ssa_lower_str_str_defaults`.
        crate::ssa_lower_str_str_defaults::fill_missing(ctx, method, args, recv_op, &mut argv);
        // Final runtime-helper dispatch (2-arg from-routing for
        // indexOf-family / split delegation / per-method intrinsic
        // lookup + repeat throw check). Documented in
        // `ssa_lower_str_str_intrinsic`.
        Some(crate::ssa_lower_str_str_intrinsic::dispatch_intrinsic(
            ctx, method, args, argv, owned_base,
        ))
    } else {
        None
    }
}
