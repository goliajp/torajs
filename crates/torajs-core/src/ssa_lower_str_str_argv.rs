//! Per-arg lowering + spec-default substitution for the generic
//! `<Str>.<method>(args)` dispatch — fourth carve-out chunk pulled
//! out of [`ssa_lower_str_str_dispatch::try_dispatch`].
//!
//! Walks `args`, lowers each into `argv`, but first applies a tower
//! of spec carve-outs:
//!
//! - **S140** — `toLocale{Upper,Lower}Case` skip the entire loop
//!   (`drop_args = true`), since the runtime helper is 1-arg only
//!   (en-US default; locale arg dropped silently).
//! - **S239 / S238 / S240 / S281 / S241 / S282** — trailing-arg
//!   ignore (delegated to [`super::ssa_lower_str_str_trailing`]).
//! - **S207 / S211** — `replace` / `replaceAll` / `localeCompare`
//!   undef → "undefined" literal substitution.
//! - **S209** — `repeat` undef → ConstI64(0).
//! - **S235** — `indexOf` / `lastIndexOf` / `includes` /
//!   `startsWith` / `endsWith` / `search` arg-0 undef → "undefined".
//! - **S214 / S224** — `indexOf` / `startsWith` / `includes`
//!   2-arg fromIndex undef → ConstI64(0).
//! - **S215 / S216 / S224** — `split` / `lastIndexOf` / `endsWith`
//!   2-arg limit/fromIndex/endPosition undef → ConstI64(MAX).
//! - **S221 / S232 / S241** — `slice` / `substring` 2-3-arg
//!   undef slots → (0, recv.length) with lazy length emit.
//! - **V3-18 m1.h.36 m1.0** — `slice` / `substring` 1-arg undef →
//!   ConstI64(0) (the missing-end-arg default comes from
//!   `fill_missing` below).
//! - **S222** — `at` / `charAt` / `charCodeAt` / `codePointAt`
//!   1-arg undef → ConstI64(0).
//! - **S223 / S236 / S241** — `padStart` / `padEnd` undef slot
//!   substitution (maxLen → 0, fillStr → " ").
//! - **S332 / S333 / S338** — `at` / `charCodeAt` / `codePointAt`
//!   / `repeat` / `slice` / `substring` / `substr` / `padStart`
//!   / `padEnd` first-arg `Type::Any` → decode via `any_to_number`
//!   → `coerce_to_i64` so the helper's i64 ABI sees a clean i64.
//! - **fallback** — `argv.push(ctx.lower_expr(a))`.

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// Walk `args` and append each lowered (or spec-substituted) operand
/// into `argv`. `recv_op` is needed for the `slice` / `substring`
/// undef-end-slot path that lazily emits a Load of the receiver's
/// length. Mutates `argv` in place.
pub(crate) fn populate_argv(
    ctx: &mut LowerCtx<'_>,
    method: &str,
    args: &[ExprId],
    recv_op: Operand,
    argv: &mut Vec<Operand>,
) {
    // S140 — locale-variant case methods drop any locales arg; the
    // runtime helper is 1-arg only (en-US default, see check.rs).
    let drop_args = matches!(method, "toLocaleLowerCase" | "toLocaleUpperCase");
    if drop_args {
        return;
    }
    // S207 — for replace / replaceAll the helper requires Str
    // operands; an explicit-undefined arg's lower path would emit a
    // non-Str value and SEGV the helper. Replace inline with the
    // interned "undefined" literal — same idiom S206 uses for
    // Array.join's undefined sep.
    //
    // S209 — for repeat the count arg is a number; per spec
    // §22.1.3.17 step 1 ToIntegerOrInfinity(undefined) = 0, so an
    // explicit-undefined arg lowers to ConstI64(0) and the helper
    // returns "".
    // S211 — localeCompare(undefined) per §22.1.3.10 step 4 takes
    // the same ToString(undefined) = "undefined" path.
    let undef_to_str_repl = matches!(method, "replace" | "replaceAll" | "localeCompare");
    let undef_to_zero = method == "repeat";
    // S235 — indexOf/lastIndexOf/includes/startsWith/endsWith/search
    // arg-0 undef → "undefined" literal (per ES §22.1.3.{8,10,5,21,
    // 7,16} step 1-3). For the search-string slot only; fromIndex /
    // position slot keeps its numeric undef carve-out (S214 / S216 /
    // S224).
    let undef_to_str_at_arg0 = matches!(
        method,
        "indexOf" | "lastIndexOf" | "includes" | "startsWith" | "endsWith" | "search"
    );
    // S214 + S224 — indexOf/startsWith/includes 2-arg fromIndex undef
    // → ConstI64(0) per ToIntegerOrInfinity(undefined)=0.
    let undef_zero_at_arg1 =
        matches!(method, "indexOf" | "startsWith" | "includes") && args.len() == 2;
    // S215 / S216 / S224 — split/lastIndexOf/endsWith 2-arg
    // limit/fromIndex/endPosition undef → ConstI64(MAX). Downstream
    // clamp picks len.
    let undef_max_at_arg1 =
        matches!(method, "split" | "lastIndexOf" | "endsWith") && args.len() == 2;
    // S221 / S232 / S241 — slice / substring 2-3 arg undef slot
    // substitution; recv.length emitted lazily only when arg[1] is
    // actually undef.
    let slice_subs_2arg =
        matches!(method, "substring" | "slice") && (args.len() == 2 || args.len() == 3);
    let mut substring_len_op: Option<Operand> = None;
    let slice_subs_1arg_undef = matches!(method, "substring" | "slice") && args.len() == 1;
    // S222 — at/charAt/charCodeAt/codePointAt 1-arg undef →
    // ConstI64(0) per ToIntegerOrInfinity(undefined)=0.
    let undef_zero_at_arg0_idx =
        matches!(method, "at" | "charAt" | "charCodeAt" | "codePointAt") && args.len() == 1;
    // S223 — padStart/padEnd arg-0 undef → ConstI64(0) (the
    // V3-18 m1.h.45 1-arg fallthrough still supplies the default
    // fill " " when arg 1 is omitted).
    let undef_zero_at_arg0_pad =
        matches!(method, "padStart" | "padEnd") && (1..=3).contains(&args.len());
    // S236 / S241 — padStart/padEnd arg-1 fillStr undef → " "
    // literal (helper's (Str, I64, Str) ABI never sees a
    // ConstPtrNull).
    let undef_space_at_arg1_pad =
        matches!(method, "padStart" | "padEnd") && (args.len() == 2 || args.len() == 3);
    for (i, &a) in args.iter().enumerate() {
        let arg_undef = matches!(ctx.expr_types.get(&a), Some(crate::check::Type::Undefined));
        // Per-method trailing-arg-ignore — drop args beyond the
        // helper ABI arity (lower-and-drop so step()-style
        // side-effect exprs still fire per S272 idiom).
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
            argv.push(substring_len_op.unwrap());
        } else if slice_subs_1arg_undef && arg_undef && i == 0 {
            argv.push(Operand::ConstI64(0));
        } else if matches!(method, "at" | "charCodeAt" | "codePointAt" | "repeat")
            && i == 0
            && matches!(ctx.expr_types.get(&a), Some(crate::check::Type::Any))
        {
            // S332 — `s.{at,charCodeAt,codePointAt,repeat}(Any)`
            // ToIntegerOrInfinity accepts arbitrary-typed input.
            argv.push(any_arg_to_i64(ctx, a));
        } else if matches!(method, "slice" | "substring" | "substr")
            && i < 2
            && matches!(ctx.expr_types.get(&a), Some(crate::check::Type::Any))
        {
            // S333 — `s.{slice,substring,substr}(Any, Any)`
            // ToIntegerOrInfinity accepts arbitrary-typed input on
            // both positional slots.
            argv.push(any_arg_to_i64(ctx, a));
        } else if matches!(method, "padStart" | "padEnd")
            && i == 0
            && matches!(ctx.expr_types.get(&a), Some(crate::check::Type::Any))
        {
            // S338 — `s.{padStart,padEnd}(Any [, fillStr])`
            // ToLength accepts arbitrary-typed input.
            argv.push(any_arg_to_i64(ctx, a));
        } else {
            argv.push(ctx.lower_expr(a));
        }
    }
}

/// S332 / S333 / S338 shared Any → i64 decode chain: lower the arg,
/// route through `any_to_number` → `coerce_to_i64` so the helper's
/// `(Str, i64, …)` ABI sees a clean i64.
fn any_arg_to_i64(ctx: &mut LowerCtx<'_>, a: ExprId) -> Operand {
    let raw = ctx.lower_expr(a);
    let f = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.any_to_number, vec![raw]),
        Type::F64,
        None,
    );
    ctx.coerce_to_i64(Operand::Value(f))
}
