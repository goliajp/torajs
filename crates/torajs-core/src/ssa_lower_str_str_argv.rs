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
//! - **the numeric slots** — every position whose spec step is
//!   `ToIntegerOrInfinity` / `ToLength` / `ToUint32`, delegated whole
//!   to [`crate::ssa_lower_str_str_argv_numslot`]; they are one
//!   family and their three readings of `undefined` are its subject.
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
    // S221 / S232 / S241 — slice / substring / substr 2-3 arg undef
    // slot substitution; recv.length emitted lazily only when arg[1]
    // is actually undef. substr joined in rotation 545 when its
    // checker gate stopped excluding static undefined: §B.2.2.1
    // step 5 reads an undefined length as "to the end", and
    // recv.length is the same value the any lane's runtime tag test
    // hands the helper (clamped downstream), so the static spelling
    // substitutes what the dynamic one selects.
    let slice_subs_2arg =
        matches!(method, "substring" | "slice" | "substr") && (args.len() == 2 || args.len() == 3);
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
        // RC-4 replace A1_T4 — a null literal takes the same
        // ToString substitution as undefined on the String-coercing
        // slots ("null" per §7.1.17): before this it lowered to a
        // null ptr in the helper's Str param and the runtime
        // deref'd it.
        let arg_null = matches!(ctx.expr_types.get(&a), Some(crate::check::Type::Null));
        let str_sub = |ctx: &mut LowerCtx<'_>| {
            let v = ctx.intern_string_literal(if arg_null { "null" } else { "undefined" });
            Operand::Value(v)
        };
        // Per-method trailing-arg-ignore — drop args beyond the
        // helper ABI arity (lower-and-drop so step()-style
        // side-effect exprs still fire per S272 idiom).
        if crate::ssa_lower_str_str_trailing::should_drop(method, i) {
            let _ = ctx.lower_expr(a);
            continue;
        }
        if undef_to_str_repl && (arg_undef || arg_null) {
            let u = str_sub(ctx);
            argv.push(u);
        } else if undef_to_str_at_arg0 && (arg_undef || arg_null) && i == 0 {
            // Chunk 616 — the checker's search-undef arm now admits
            // Null needles too (§7.1.17 ToString(null) = "null");
            // str_sub picks the matching literal. Shipped as a pair
            // with that admit per the 605-era note here.
            let u = str_sub(ctx);
            argv.push(u);
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
                    InstKind::Load(Type::I64, recv_op.clone(), 8),
                    Type::I64,
                    None,
                );
                substring_len_op = Some(Operand::Value(len));
            }
            argv.push(substring_len_op.unwrap());
        } else if slice_subs_1arg_undef && arg_undef && i == 0 {
            argv.push(Operand::ConstI64(0));
        } else if let Some(op) = crate::ssa_lower_str_str_argv_numslot::try_lower(
            ctx,
            method,
            i,
            a,
            args,
            &recv_op,
            &mut substring_len_op,
        ) {
            // Every position whose spec step is ToIntegerOrInfinity /
            // ToLength / ToUint32 — see that module for why they are
            // one family and where their three readings of
            // `undefined` part ways. It sits here, after the
            // substitution arms, because those claim the
            // statically-`undefined` spellings and answer the same
            // defaults as constants.
            argv.push(op);
        } else if matches!(method, "replace" | "replaceAll") && i < 2 {
            // RFC 20260712 chunk B — non-regex searchValue and
            // non-fn replaceValue are ToString-coerced per
            // §22.1.3.19 steps 3/6 (a throwing user toString
            // propagates; argv order keeps searchValue first). A
            // str-shaped arg passes through; anything else routes
            // via coerce_to_str + throw check. Fresh-owned
            // operands park in argv_owned_temps so the drop lands
            // AFTER the helper call.
            let raw = ctx.lower_expr(a);
            let raw_ty = ctx.operand_ty(&raw);
            if matches!(raw_ty, Type::Str | Type::Substr) {
                if ctx.expr_is_fresh_owned(a) {
                    ctx.argv_owned_temps.push((raw.clone(), raw_ty));
                }
                argv.push(raw);
            } else {
                let s = ctx.coerce_to_str(raw.clone(), raw_ty);
                ctx.emit_throw_check(None);
                if raw_ty.is_refcounted() && ctx.expr_is_fresh_owned(a) {
                    ctx.emit_drop_value(raw, raw_ty);
                }
                ctx.argv_owned_temps.push((s.clone(), Type::Str));
                argv.push(s);
            }
        } else {
            // Fresh-owned str temps (`s.replace(n.slice(0,1), x)`)
            // park for the post-call drop — pre-chunk-B they
            // leaked one cell per call.
            let v = ctx.lower_expr(a);
            let v_ty = ctx.operand_ty(&v);
            if matches!(v_ty, Type::Str | Type::Substr) && ctx.expr_is_fresh_owned(a) {
                ctx.argv_owned_temps.push((v.clone(), v_ty));
            }
            argv.push(v);
        }
    }
}
