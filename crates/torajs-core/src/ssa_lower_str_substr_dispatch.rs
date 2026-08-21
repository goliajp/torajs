//! `<Substr>.<method>(args)` dispatch — first sub-split carved out
//! of [`ssa_lower_str::try_lower_method_call`] along the
//! `ssa_lower_str/{view,search,transform,structural}.rs` axis
//! anticipated in that file's module doc.
//!
//! Encapsulates every Substr-receiver path:
//! - `charAt` 0-arg / N-arg wedges (length-1 view slice without
//!   materializing).
//! - Substr-trim stack-write fast path (delegates to
//!   [`crate::ssa_lower_substr_trim_into::try_emit`]).
//! - View-aware fast paths backed by `substr_*` intrinsics
//!   (`charCodeAt`, `codePointAt`, `startsWith`, `endsWith`,
//!   `includes`, `indexOf`, `slice`, `substring`, `trim`/`trimStart`/
//!   `trimEnd`).
//! - Materialize-then-route fallback that calls `substr_to_owned`
//!   and replays the resulting Str through the matching
//!   `__torajs_str_*` intrinsic + the standard `str_drop` follow-up
//!   so the temporary owned Str is freed at the call site.
//!
//! Returns `None` when the receiver is not [`Type::Substr`] so the
//! caller can fall through to the Str-path dispatch.

use crate::ast::ExprId;
use crate::ssa::{BinOp as SsaBinOp, InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// Dispatch `<Substr>.<method>(args)` to the appropriate
/// runtime intrinsic / fast path. Returns `Some(value)` when the
/// receiver was a [`Type::Substr`] and the call was lowered;
/// `None` otherwise so the caller can keep trying the Str path.
pub(crate) fn try_dispatch(
    ctx: &mut LowerCtx<'_>,
    method: &str,
    args: &[ExprId],
    recv_op: Operand,
    recv_ty: Type,
) -> Option<Operand> {
    if recv_ty != Type::Substr {
        return None;
    }
    // S240 widens the 1-arg detection to 2-arg so the
    // charAt(idx, trailing) shape still routes through this fast
    // path; args[1] is never lowered (trailing-arg ignore).
    if method == "charAt" && !args.is_empty() {
        // charAt on Substr: substr_slice(v, i, i+1).
        // S272 — widen `== 1 || == 2` to `>= 1`; trailing exprs eval-
        // and-drop below per ES §22.1.3.2 trailing-arg ignore.
        let idx_val = ctx.lower_expr(args[0]);
        let end = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::BinOp(SsaBinOp::Add, idx_val, Operand::ConstI64(1)),
            Type::I64,
            None,
        );
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.substr_slice,
                vec![recv_op, idx_val, Operand::Value(end)],
            ),
            Type::Substr,
            None,
        );
        for &a in &args[1..] {
            let _ = ctx.lower_expr(a);
        }
        return Some(Operand::Value(v));
    }
    // V3-18 wedge — Substr.charAt() 0-arg variant —
    // hoisted above the generic Substr dispatch
    // because Substr.charAt has no view-aware
    // fast path and would otherwise hit the
    // 'unsupported Substr method' panic.
    if method == "charAt" && args.is_empty() {
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.substr_slice,
                vec![recv_op, Operand::ConstI64(0), Operand::ConstI64(1)],
            ),
            Type::Substr,
            None,
        );
        return Some(Operand::Value(v));
    }
    // Step 13-a: stack-write trim fast-path (sibling).
    if let Some(v) =
        crate::ssa_lower_substr_trim_into::try_emit(ctx, method, args.len(), recv_op, recv_ty)
    {
        return Some(v);
    }
    // View-aware fast paths — read bytes from
    // parent + offset directly, no per-call malloc.
    let view_aware = match method {
        "charCodeAt" => Some((ctx.intrinsics.substr_char_code_at, Type::F64)),
        "codePointAt" => Some((ctx.intrinsics.substr_code_point_at, Type::I64)),
        "startsWith" => Some((ctx.intrinsics.substr_starts_with, Type::Bool)),
        "endsWith" => Some((ctx.intrinsics.substr_ends_with, Type::Bool)),
        "includes" => Some((ctx.intrinsics.substr_includes, Type::Bool)),
        "indexOf" => Some((ctx.intrinsics.substr_index_of, Type::I64)),
        "slice" => Some((ctx.intrinsics.substr_slice, Type::Substr)),
        "substring" => Some((ctx.intrinsics.substr_substring, Type::Substr)),
        "trim" => Some((ctx.intrinsics.substr_trim, Type::Substr)),
        "trimStart" | "trimLeft" => Some((ctx.intrinsics.substr_trim_start, Type::Substr)),
        "trimEnd" | "trimRight" => Some((ctx.intrinsics.substr_trim_end, Type::Substr)),
        _ => None,
    };
    if let Some((target, ret_ty)) = view_aware {
        let mut argv = Vec::with_capacity(args.len() + 1);
        argv.push(recv_op);
        for a in args {
            argv.push(ctx.lower_expr(*a));
        }
        // V3-18 m1.h.36 — Substr.slice / substring
        // also accept 0/1 args; fill defaults the
        // same way as the Str path. Substr len is
        // at offset 8 of the Substr layout.
        if matches!(method, "slice" | "substring") && args.len() < 2 {
            if args.is_empty() {
                argv.push(Operand::ConstI64(0));
            }
            let len = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(Type::I64, recv_op, 8),
                Type::I64,
                None,
            );
            argv.push(Operand::Value(len));
        }
        // V3-18 wedge — Substr.charCodeAt /
        // codePointAt 0-arg defaults pos to 0.
        if matches!(method, "charCodeAt" | "codePointAt") && args.is_empty() {
            argv.push(Operand::ConstI64(0));
        }
        let v = ctx
            .f
            .append_inst(ctx.cur_block, InstKind::Call(target, argv), ret_ty, None);
        return Some(Operand::Value(v));
    }
    match method {
        // Unreachable now — view_aware above covers
        // these — but keep the explicit no-op match
        // arm in case the dispatch table later splits.
        "charCodeAt" | "codePointAt" => unreachable!(),
        // Methods producing new strings — materialize
        // first then route through the OWNED Str path.
        _ => {
            let owned = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.substr_to_owned, vec![recv_op]),
                Type::Str,
                None,
            );
            if matches!(method, "toLocaleUpperCase" | "toLocaleLowerCase") {
                if let Some(v) = lower_owned_to_locale_case(ctx, method, args, owned) {
                    return Some(v);
                }
            }
            let mut argv = Vec::with_capacity(args.len() + 1);
            argv.push(Operand::Value(owned));
            for a in args {
                argv.push(ctx.lower_expr(*a));
            }
            let (target, ret_ty) = match method {
                "slice" => (ctx.intrinsics.str_slice, Type::Str),
                "substring" => (ctx.intrinsics.str_substring, Type::Str),
                // S140 — Locale variants alias to the non-locale
                // form (en-US default host locale, matches bun).
                "toUpperCase" | "toLocaleUpperCase" => (ctx.intrinsics.str_to_upper, Type::Str),
                "toLowerCase" | "toLocaleLowerCase" => (ctx.intrinsics.str_to_lower, Type::Str),
                "trim" => (ctx.intrinsics.str_trim, Type::Str),
                "trimStart" | "trimLeft" => (ctx.intrinsics.str_trim_start, Type::Str),
                "trimEnd" | "trimRight" => (ctx.intrinsics.str_trim_end, Type::Str),
                "padStart" => (ctx.intrinsics.str_pad_start, Type::Str),
                "padEnd" => (ctx.intrinsics.str_pad_end, Type::Str),
                "startsWith" => (ctx.intrinsics.str_starts_with, Type::Bool),
                "endsWith" => (ctx.intrinsics.str_ends_with, Type::Bool),
                "includes" => (ctx.intrinsics.str_includes, Type::Bool),
                "indexOf" => (ctx.intrinsics.str_index_of, Type::I64),
                "lastIndexOf" => (ctx.intrinsics.str_last_index_of, Type::I64),
                "localeCompare" => (ctx.intrinsics.str_locale_compare, Type::I64),
                // V3-18 wedge — Substr.search routes
                // through str_index_of after
                // materializing (same as Str.search):
                // first match position or -1.
                "search" => (ctx.intrinsics.str_index_of, Type::I64),
                "at" => (ctx.intrinsics.str_at, Type::Str),
                "repeat" => (ctx.intrinsics.str_repeat, Type::Str),
                "replace" => (ctx.intrinsics.str_replace, Type::Str),
                "replaceAll" => (ctx.intrinsics.str_replace_all, Type::Str),
                other => {
                    panic!("ssa-lower: unsupported Substr method `{other}`")
                }
            };
            let v = ctx
                .f
                .append_inst(ctx.cur_block, InstKind::Call(target, argv), ret_ty, None);
            // owned is consumed by the call (our Str
            // intrinsics are read-only on Str args, so
            // owned still needs scope-end drop — but
            // it's not bound to a local. Insert an
            // explicit drop so it doesn't leak).
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.str_drop, vec![Operand::Value(owned)]),
            );
            // ES §22.1.3.16 step 4 — `s.repeat(n < 0)` throws
            // RangeError. Runtime sets a TLS pending throw;
            // emit_throw_check propagates here.
            if method == "repeat" {
                ctx.emit_throw_check(None);
            }
            Some(Operand::Value(v))
        }
    }
}

/// ES402 — Substr.toLocale{Upper,Lower}Case routes the materialized
/// receiver through the tailored kernels (same wedge shape as the
/// Str path in [`crate::ssa_lower_str_short_circuits`], including
/// the Arr<Str> CanonicalizeLocaleList walk). Answers `None` for an
/// unroutable locale type — the caller falls through to the legacy
/// locale-dropping remap.
fn lower_owned_to_locale_case(
    ctx: &mut LowerCtx<'_>,
    method: &str,
    args: &[ExprId],
    owned: crate::ssa::ValueId,
) -> Option<Operand> {
    let upper = method == "toLocaleUpperCase";
    let v = crate::ssa_lower_str_short_circuits::lower_locale_case(
        ctx,
        args,
        Operand::Value(owned),
        upper,
    )?;
    for &a in args.iter().skip(1) {
        let _ = ctx.lower_expr(a);
    }
    ctx.emit_throw_check(None);
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.str_drop, vec![Operand::Value(owned)]),
    );
    Some(v)
}
