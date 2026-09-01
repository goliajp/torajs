//! Final `<Str>.<method>` runtime-helper dispatch — fifth carve-out
//! chunk pulled out of [`ssa_lower_str_str_dispatch::try_dispatch`].
//!
//! Assumes the caller has produced an `argv` that already includes
//! the receiver at slot 0 plus every positional arg in helper-ABI
//! shape (undef-substituted, Any-decoded, trailing-dropped, default-
//! filled). Routes through three paths:
//!
//! - 2-arg `(needle, fromIndex)` `indexOf` / `lastIndexOf` route to
//!   the dedicated `str_*_index_of_from` helpers (V3-18 m1.h.50).
//! - 2-arg `(needle, position)` `startsWith` / `endsWith` /
//!   `includes` route to the dedicated `str_*_from` helpers
//!   (V3-18 m1.h.51).
//! - `split` is delegated to [`super::ssa_lower_str_str_split`]
//!   (chunk #1 of this decomposition) for its multi-shape inline
//!   emit.
//! - Everything else dispatches through a per-method intrinsic
//!   lookup, emits the `Call(target, argv)` once, and post-runs
//!   `repeat`'s pending-throw check (ES §22.1.3.16 step 4 — n<0
//!   throws RangeError).

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// Dispatch the prepared `argv` through the appropriate runtime
/// helper for `method`. Returns the resulting `Operand` (caller wraps
/// in `Some` for the outer `try_dispatch` Option contract).
pub(crate) fn dispatch_intrinsic(
    ctx: &mut LowerCtx<'_>,
    method: &str,
    args: &[ExprId],
    argv: Vec<Operand>,
    owned_base: usize,
) -> Operand {
    // V3-18 m1.h.50 — indexOf / lastIndexOf 2-arg (needle, fromIndex)
    if matches!(method, "indexOf" | "lastIndexOf") && args.len() >= 2 {
        let target = if method == "indexOf" {
            ctx.intrinsics.str_index_of_from
        } else {
            ctx.intrinsics.str_last_index_of_from
        };
        let v = ctx
            .f
            .append_inst(ctx.cur_block, InstKind::Call(target, argv), Type::I64, None);
        drain_owned_temps(ctx, owned_base);
        return Operand::Value(v);
    }
    // V3-18 m1.h.51 — startsWith / endsWith / includes 2-arg
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
        drain_owned_temps(ctx, owned_base);
        return Operand::Value(v);
    }
    // split has its own inline emit (multi-block CondBr / arr_slice
    // limit-clamp) — delegate to chunk #1; the drop drain lands in
    // the join block lower_split leaves as cur_block.
    if method == "split" {
        let r = crate::ssa_lower_str_str_split::lower_split(ctx, args, argv);
        drain_owned_temps(ctx, owned_base);
        return r;
    }
    // Final per-method intrinsic + return-type lookup.
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
        // P11.3-A1 — codePointAt combines surrogate pairs per ES
        // §22.1.3.3; charCodeAt returns the lone UTF-16 code unit
        // per §22.1.3.2. Two distinct intrinsics now that
        // P11.1-S1/S5 gave us proper hybrid Latin-1 / UTF-16 storage.
        "charCodeAt" => (ctx.intrinsics.str_char_code_at, Type::F64),
        "codePointAt" => (ctx.intrinsics.str_code_point_at, Type::I64),
        "startsWith" => (ctx.intrinsics.str_starts_with, Type::Bool),
        "endsWith" => (ctx.intrinsics.str_ends_with, Type::Bool),
        "includes" => (ctx.intrinsics.str_includes, Type::Bool),
        "indexOf" => (ctx.intrinsics.str_index_of, Type::I64),
        // V3-18 wedge — String.prototype.search with a string arg
        // (RegExp arg form is a follow-up substrate item alongside
        // Symbol.search dispatch). For a plain string the result is
        // exactly indexOf — first match position or -1 — so route
        // through the same helper.
        "search" => (ctx.intrinsics.str_index_of, Type::I64),
        "lastIndexOf" => (ctx.intrinsics.str_last_index_of, Type::I64),
        "localeCompare" => (ctx.intrinsics.str_locale_compare, Type::I64),
        _ => unreachable!(),
    };
    let v = ctx
        .f
        .append_inst(ctx.cur_block, InstKind::Call(target, argv), ret_ty, None);
    // ES §22.1.3.16 step 4 — s.repeat(n < 0) throws RangeError.
    // Runtime sets a TLS pending throw; emit_throw_check propagates.
    if method == "repeat" {
        ctx.emit_throw_check(None);
    }
    drain_owned_temps(ctx, owned_base);
    Operand::Value(v)
}

/// RFC 20260712 chunk B — drop the fresh-owned argv operands parked
/// since `owned_base` (ToString-coerced replace args, fresh str
/// temps) now that the helper call has consumed them. The base
/// index keeps a nested str-method lowering (an arg expression that
/// is itself a str method call) from draining its caller's parks.
fn drain_owned_temps(ctx: &mut LowerCtx<'_>, owned_base: usize) {
    let temps: Vec<_> = ctx.temps.argv_owned.split_off(owned_base);
    for (op, ty, tok) in temps {
        ctx.pop_throw_temp(tok);
        ctx.emit_drop_value(op, ty);
    }
}
