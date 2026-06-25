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
        // S140 — locale-variant case methods drop any locales arg; the
        // runtime helper is 1-arg only (en-US default, see check.rs).
        let drop_args = matches!(method, "toLocaleLowerCase" | "toLocaleUpperCase");
        if !drop_args {
            // S207 — for replace / replaceAll the helper requires
            // Str operands; an explicit-undefined arg's lower path
            // would emit a non-Str value and SEGV the helper.
            // Replace the operand inline with the interned
            // "undefined" literal — same idiom S206 uses for
            // Array.join's undefined sep.
            //
            // S209 — for repeat the count arg is a number; per
            // spec §22.1.3.17 step 1 ToIntegerOrInfinity(undefined)
            // = 0, so an explicit-undefined arg lowers to
            // ConstI64(0) and the helper returns "".
            // S211 — localeCompare(undefined) per §22.1.3.10 step 4
            // takes the same ToString(undefined) = "undefined" path.
            let undef_to_str_repl = matches!(method, "replace" | "replaceAll" | "localeCompare");
            let undef_to_zero = method == "repeat";
            // S235 — String.{indexOf,lastIndexOf,includes,startsWith,endsWith,
            // search}(undefined) per ES §22.1.3.{8,10,5,21,7,16} step 1-3:
            // ToString(undefined) = "undefined". For the search-string slot
            // only (arg 0); the fromIndex/position slot keeps its
            // numeric undef carve-out (S214 / S216 / S224). Replace the
            // undef operand with the interned "undefined" literal so
            // the helper's (Str, Str) ABI receives a concrete pointer.
            let undef_to_str_at_arg0 = matches!(
                method,
                "indexOf" | "lastIndexOf" | "includes" | "startsWith" | "endsWith" | "search"
            );
            // S214 — String.indexOf(needle, undefined) per ES
            // §22.1.3.8 step 4: ToIntegerOrInfinity(undefined)=0,
            // so fromIndex=undefined lowers to ConstI64(0). Only
            // applies to args[1] (fromIndex); args[0] (needle)
            // keeps generic lower (0-arg-undef-needle covered by
            // §22.1.3.8 "undefined" literal default block below).
            //
            // S224 — extend the same zero-default to startsWith /
            // includes per ES §22.1.3.{21,5}:
            // `ToIntegerOrInfinity(undefined)=0`, so position=undef
            // matches "search from index 0". (endsWith uses the
            // length-default branch — see undef_max_at_arg1 below.)
            let undef_zero_at_arg1 =
                matches!(method, "indexOf" | "startsWith" | "includes") && args.len() == 2;
            // S215 — String.split(sep, undefined) per ES §22.1.3.21
            // step 2: limit undefined → 2^32-1 (effectively no
            // truncation). Lower limit=undefined to ConstI64(i64::MAX);
            // the take-min branch downstream Slt(MAX, len) is false →
            // take_slot = len, matching spec "no truncation".
            //
            // S216 — String.lastIndexOf(needle, undefined) per ES
            // §22.1.3.10 step 5: ToNumber(undefined)=NaN, NaN→pos=+∞.
            // Effective fromIndex=length; lower to ConstI64(i64::MAX)
            // and the str_last_index_of_from helper clamps `from > len`
            // to len (see torajs-str lookup.rs::last_index_of_from).
            //
            // S224 — extend the same length-default to endsWith per
            // ES §22.1.3.7: endPosition=undef → len(O). The
            // `str_ends_with_from` helper clamps `end > s.len()`
            // back to `s.len()` (lookup.rs::ends_with_from).
            let undef_max_at_arg1 =
                matches!(method, "split" | "lastIndexOf" | "endsWith") && args.len() == 2;
            // S221 — String.substring(undefined [, undefined]) per ES
            // §22.1.3.22 step 4/5: start=undef → 0, end=undef → length.
            // Replace each undef slot before lower so the str_substring
            // helper's (Str, I64, I64) ABI never sees an Undefined
            // operand. recv.length is emitted lazily — only if arg[1]
            // is actually undef.
            //
            // S232 — extend the same undef-slot substitution to `slice`
            // 2-arg per ES §22.1.3.20 step 3/4: start undef → 0, end
            // undef → length. The 1-arg-undef shape for slice/substring
            // is handled by the existing V3-18 m1.h.36 fallthrough
            // below — once the loop substitutes the lone undef arg
            // with ConstI64(0), the `args.len() < 2` path appends the
            // recv length, yielding `[0, len]`.
            // S241 widens the 2-arg detection to 3-arg so a trailing-arg
            // shape (slice/substring(a, b, trailing)) still substitutes
            // undef slots; the trailing operand is dropped by the loop
            // break-at-i>1 guard below.
            let slice_subs_2arg =
                matches!(method, "substring" | "slice") && (args.len() == 2 || args.len() == 3);
            let mut substring_len_op: Option<Operand> = None;
            let slice_subs_1arg_undef = matches!(method, "substring" | "slice") && args.len() == 1;
            // S222 — String.{at,charAt,charCodeAt,codePointAt}(undefined)
            // per ES §22.1.3.{1,2,3,4} step 2-3:
            // ToIntegerOrInfinity(undefined)=0. Replace the single undef
            // index slot with ConstI64(0) — equivalent to the 0-arg
            // shape these methods already accept.
            let undef_zero_at_arg0_idx =
                matches!(method, "at" | "charAt" | "charCodeAt" | "codePointAt") && args.len() == 1;
            // S223 — String.{padStart,padEnd}(undefined [, fillStr]) per
            // ES §22.1.3.{16,17} step 1: ToLength(undefined)=0, and step
            // 2 short-circuits because `0 <= S.length`, so the helper
            // returns S unchanged. Replace the undef maxLength slot with
            // ConstI64(0); the V3-18 1-arg fallthrough below still
            // supplies the default fill " " when arg 1 is omitted.
            let undef_zero_at_arg0_pad =
                matches!(method, "padStart" | "padEnd") && (1..=3).contains(&args.len());
            // S236 — String.{padStart,padEnd}(N, undefined) per ES
            // §22.1.3.{16,17} step 6.a: fillString undef → " ". Replace
            // the undef fillStr slot with the interned " " literal so
            // the helper's (Str, I64, Str) ABI never sees a ConstPtrNull;
            // matches the V3-18 m1.h.45 1-arg default fill.
            //
            // S241 widens the 2-arg detection to 3-arg so a padStart/padEnd
            // (N, undef, trailing) shape still substitutes the " " literal;
            // the trailing operand is dropped by the loop break-at-i>1
            // guard below.
            let undef_space_at_arg1_pad =
                matches!(method, "padStart" | "padEnd") && (args.len() == 2 || args.len() == 3);
            for (i, &a) in args.iter().enumerate() {
                let arg_undef =
                    matches!(ctx.expr_types.get(&a), Some(crate::check::Type::Undefined));
                // Per-method trailing-arg-ignore — drop args beyond the
                // helper ABI arity (lower-and-drop so step()-style
                // side-effect exprs still fire per S272 idiom). Spec
                // carve-outs (S238/S239/S240/S281/S241/S282) are
                // documented in `ssa_lower_str_str_trailing`.
                if crate::ssa_lower_str_str_trailing::should_drop(method, i) {
                    let _ = ctx.lower_expr(a);
                    continue;
                }
                if undef_to_str_repl && arg_undef {
                    let u = ctx.intern_string_literal("undefined");
                    argv.push(Operand::Value(u));
                } else if undef_to_str_at_arg0 && arg_undef && i == 0 {
                    let u = ctx.intern_string_literal("undefined");
                    argv.push(Operand::Value(u));
                } else if undef_to_zero && arg_undef {
                    argv.push(Operand::ConstI64(0));
                } else if undef_zero_at_arg1 && arg_undef && i == 1 {
                    argv.push(Operand::ConstI64(0));
                } else if undef_max_at_arg1 && arg_undef && i == 1 {
                    argv.push(Operand::ConstI64(i64::MAX));
                } else if undef_zero_at_arg0_idx && arg_undef && i == 0 {
                    argv.push(Operand::ConstI64(0));
                } else if undef_zero_at_arg0_pad && arg_undef && i == 0 {
                    argv.push(Operand::ConstI64(0));
                } else if undef_space_at_arg1_pad && arg_undef && i == 1 {
                    let space = ctx.intern_string_literal(" ");
                    argv.push(Operand::Value(space));
                } else if slice_subs_2arg && arg_undef && i == 0 {
                    argv.push(Operand::ConstI64(0));
                } else if slice_subs_2arg && arg_undef && i == 1 {
                    if substring_len_op.is_none() {
                        let len = ctx.f.append_inst(
                            ctx.cur_block,
                            InstKind::Load(Type::I64, recv_op, 8),
                            Type::I64,
                            None,
                        );
                        substring_len_op = Some(Operand::Value(len));
                    }
                    argv.push(substring_len_op.clone().unwrap());
                } else if slice_subs_1arg_undef && arg_undef && i == 0 {
                    argv.push(Operand::ConstI64(0));
                } else if matches!(method, "at" | "charCodeAt" | "codePointAt" | "repeat")
                    && i == 0
                    && matches!(ctx.expr_types.get(&a), Some(crate::check::Type::Any))
                {
                    // S332 — `s.{at,charCodeAt,codePointAt,repeat}(Any)`
                    // per ES §22.1.3.{1,3,4,17}: ToIntegerOrInfinity
                    // accepts arbitrary-typed input. Decode Any via
                    // anyv_to_number → coerce_to_i64 so the helper's
                    // (Str, i64) ABI sees a clean i64. Sister to
                    // S329 (fromCharCode Any). charAt has its own
                    // early path (above) so isn't reached here.
                    let raw = ctx.lower_expr(a);
                    let f = ctx.f.append_inst(
                        ctx.cur_block,
                        InstKind::Call(ctx.intrinsics.any_to_number, vec![raw]),
                        Type::F64,
                        None,
                    );
                    argv.push(ctx.coerce_to_i64(Operand::Value(f)));
                } else if matches!(method, "slice" | "substring" | "substr")
                    && i < 2
                    && matches!(ctx.expr_types.get(&a), Some(crate::check::Type::Any))
                {
                    // S333 — `s.{slice,substring,substr}(Any, Any)` per
                    // ES §22.1.3.{20,22,23}: ToIntegerOrInfinity accepts
                    // arbitrary-typed input on both positional slots.
                    // Decode Any via anyv_to_number → coerce_to_i64 so
                    // the helper's (Str, i64, i64) ABI sees clean i64s.
                    // Sister to S332.
                    let raw = ctx.lower_expr(a);
                    let f = ctx.f.append_inst(
                        ctx.cur_block,
                        InstKind::Call(ctx.intrinsics.any_to_number, vec![raw]),
                        Type::F64,
                        None,
                    );
                    argv.push(ctx.coerce_to_i64(Operand::Value(f)));
                } else if matches!(method, "padStart" | "padEnd")
                    && i == 0
                    && matches!(ctx.expr_types.get(&a), Some(crate::check::Type::Any))
                {
                    // S338 — `s.{padStart,padEnd}(Any [, fillStr])` per
                    // ES §22.1.3.{16,17} step 1: ToLength accepts
                    // arbitrary-typed input. Decode Any via
                    // anyv_to_number → coerce_to_i64 so the helper's
                    // (Str, i64, Str) ABI sees a clean i64.
                    // Sister to S332/S333.
                    let raw = ctx.lower_expr(a);
                    let f = ctx.f.append_inst(
                        ctx.cur_block,
                        InstKind::Call(ctx.intrinsics.any_to_number, vec![raw]),
                        Type::F64,
                        None,
                    );
                    argv.push(ctx.coerce_to_i64(Operand::Value(f)));
                } else {
                    argv.push(ctx.lower_expr(a));
                }
            }
        }
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
