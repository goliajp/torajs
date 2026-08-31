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
use crate::ssa::{InstKind, Operand, Terminator, Type};
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
        } else if matches!(method, "at" | "charCodeAt" | "codePointAt" | "repeat")
            && i == 0
            && !matches!(ctx.expr_types.get(&a), Some(crate::check::Type::Number))
        {
            // S332 — `s.{at,charCodeAt,codePointAt,repeat}(x)`:
            // ToIntegerOrInfinity COERCES its operand, so every shape
            // but Number (which stays on the typed-tier fast path)
            // routes through the runtime's own ToNumber. Rotation 463
            // widened this from an `Any`-only admission — the checker
            // mirror in `check_type_of_call_string_char_any` dropped
            // the matching shape gate, so `'abc'.charCodeAt('1')` is
            // 98 rather than a compile-time refusal.
            argv.push(ctx.lower_to_index_operand(a));
        } else if matches!(method, "slice" | "substring" | "substr")
            && i == 1
            && matches!(ctx.expr_types.get(&a), Some(crate::check::Type::Any))
        {
            // Rotation 543 — §22.1.3.{23,24} step 4 and §B.2.2.1
            // step 5 both read an `undefined` second slot as "to the
            // end of the string", NOT as ToIntegerOrInfinity's 0. The
            // STATIC spelling already answers that (`slice_subs_2arg`
            // above loads the receiver's length); a value that only
            // turns out to be undefined at RUN time fell through to
            // the plain coercion and answered "".
            //
            // Found because rotation 543's mixed-element-type fix made
            // `[...numbers, undefined]` actually hold `undefined`
            // instead of `0`. test262's substr coverage builds its
            // argument matrix out of exactly that array and had been
            // passing only because its own reference implementation
            // read the same wrong `0` — both sides wrong, agreeing.
            //
            // NaN must NOT take this path (`slice(0, NaN)` is ""), so
            // the test is on the any TAG, not on the number.
            let (tag, _, idx) = any_slot_tag_number_index(ctx, a);
            if substring_len_op.is_none() {
                let len = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Load(Type::I64, recv_op.clone(), 8),
                    Type::I64,
                    None,
                );
                substring_len_op = Some(Operand::Value(len));
            }
            let sel = select_on_undef_tag(
                ctx,
                tag,
                substring_len_op.clone().unwrap(),
                idx,
                "__str_end_slot",
            );
            argv.push(sel);
        } else if matches!(method, "slice" | "substring" | "substr")
            && i < 2
            && matches!(ctx.expr_types.get(&a), Some(crate::check::Type::Any))
        {
            // S333 — `s.{slice,substring,substr}(Any, Any)`
            // ToIntegerOrInfinity accepts arbitrary-typed input on
            // both positional slots.
            argv.push(ctx.lower_to_index_operand(a));
        } else if matches!(method, "padStart" | "padEnd")
            && i == 0
            && matches!(ctx.expr_types.get(&a), Some(crate::check::Type::Any))
        {
            // S338 — `s.{padStart,padEnd}(Any [, fillStr])`
            // ToLength accepts arbitrary-typed input.
            argv.push(ctx.lower_to_index_operand(a));
        } else if matches!(method, "endsWith" | "split")
            && i == 1
            && !matches!(ctx.expr_types.get(&a), Some(crate::check::Type::Number))
        {
            // Rotation 544 — §22.1.3.7 step 5 and §22.1.3.23 step 3
            // both test the slot for `undefined` ITSELF, not for the
            // NaN its ToNumber would give: `'abc'.endsWith('c',
            // undefined)` is true and `'abc'.endsWith('c', NaN)` is
            // false; `'a,b,c'.split(',', undefined)` is the whole
            // split and `split(',', NaN)` is `[]`. So the runtime
            // test is on the any TAG, the same shape the slice /
            // substring / substr end slot takes, and the `i64::MAX`
            // sentinel is what the static-undefined arm above
            // already hands the helper for "no limit".
            if matches!(ctx.expr_types.get(&a), Some(crate::check::Type::Any)) {
                let (tag, _, idx) = any_slot_tag_number_index(ctx, a);
                let sel = select_on_undef_tag(
                    ctx,
                    tag,
                    Operand::ConstI64(i64::MAX),
                    idx,
                    "__str_pos_slot",
                );
                argv.push(sel);
            } else {
                // A statically-shaped operand that is not Number is
                // also not `undefined` — that spelling is claimed by
                // the `undef_max_at_arg1` arm above — so there is
                // nothing to test at run time.
                argv.push(ctx.lower_to_index_operand(a));
            }
        } else if method == "lastIndexOf" && i == 1 {
            // Rotation 544 — §22.1.3.10 steps 5-6 read a NaN position
            // as +∞, so `'abcabc'.lastIndexOf('a', NaN)` is 3, not
            // `coerce_to_i64`'s 0 — and so is the `undefined`
            // spelling, by way of the same NaN. The test is on the
            // NUMBER, which is why this slot cannot share the
            // endsWith / split arm above, and it covers EVERY shape:
            // a literal `NaN` is a `Type::Number` and reaches it just
            // as `'zz'` and an any box do.
            argv.push(lower_lastindexof_pos(ctx, a));
        } else if matches!(method, "indexOf" | "includes" | "startsWith")
            && i == 1
            && !matches!(ctx.expr_types.get(&a), Some(crate::check::Type::Number))
        {
            // Rotation 544 — §22.1.3.{8,14,22} read this slot with a
            // plain ToIntegerOrInfinity, whose NaN and whose
            // `undefined` are both 0, so no run-time test is owed and
            // the shared coercion is the whole answer.
            argv.push(ctx.lower_to_index_operand(a));
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

/// S332 / S333 / S338 shared Any → i64 decode chain: lower the arg,
/// route through `any_to_number` → `coerce_to_i64` so the helper's
/// `(Str, i64, …)` ABI sees a clean i64. ToNumber over an object
/// with no primitive conversion records a pending TypeError
/// (§7.1.1 OrdinaryToPrimitive) — propagate it before the NaN
/// placeholder flows into the position slot.
/// Lower an `Any` slot into its three readings at once: the box TAG,
/// the ToNumber of it, and the ToIntegerOrInfinity of that.
///
/// A slot whose spec default for `undefined` is not ToNumber's own
/// `NaN` needs the tag; one whose default is what `NaN` means anyway
/// needs only the number. A user `valueOf` can throw, so the same
/// check `lower_to_number_operand` emits is emitted here.
fn any_slot_tag_number_index(ctx: &mut LowerCtx<'_>, a: ExprId) -> (Operand, Operand, Operand) {
    let raw = ctx.lower_expr(a);
    let cur = ctx.cur_block;
    let tag = ctx.f.append_inst(
        cur,
        InstKind::Call(ctx.intrinsics.any_unbox_tag, vec![raw.clone()]),
        Type::I64,
        None,
    );
    let n = ctx.f.append_inst(
        cur,
        InstKind::Call(ctx.intrinsics.any_to_number, vec![raw]),
        Type::F64,
        None,
    );
    ctx.emit_throw_check(None);
    let idx = ctx.coerce_to_i64(Operand::Value(n));
    (Operand::Value(tag), Operand::Value(n), idx)
}

/// `cond ? when_true : when_false` over two i64 operands, as a slot
/// plus a branch rather than `InstKind::Select` — that one is
/// introduced only after the egraph pass and its elaborator rejects an
/// early one loudly. Both operands are computed before the branch, so
/// neither may carry a side effect the other arm must not see.
fn select_i64(
    ctx: &mut LowerCtx<'_>,
    cond: Operand,
    when_true: Operand,
    when_false: Operand,
    slot_name: &str,
) -> Operand {
    let slot = ctx.alloca(Type::I64, Some(slot_name));
    let then_blk = ctx.f.add_block();
    let else_blk = ctx.f.add_block();
    let join_blk = ctx.f.add_block();
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond,
            then_blk,
            else_blk,
        },
    );
    ctx.cur_block = then_blk;
    ctx.f.append_void(
        then_blk,
        InstKind::Store(when_true, Operand::Value(slot), 0),
    );
    ctx.f.set_term(then_blk, Terminator::Br(join_blk));
    ctx.cur_block = else_blk;
    ctx.f.append_void(
        else_blk,
        InstKind::Store(when_false, Operand::Value(slot), 0),
    );
    ctx.f.set_term(else_blk, Terminator::Br(join_blk));
    ctx.cur_block = join_blk;
    Operand::Value(ctx.f.append_inst(
        join_blk,
        InstKind::Load(Type::I64, Operand::Value(slot), 0),
        Type::I64,
        None,
    ))
}

/// `<tag is undefined> ? default : idx`. Tag 5 is `undefined`.
fn select_on_undef_tag(
    ctx: &mut LowerCtx<'_>,
    tag: Operand,
    default: Operand,
    idx: Operand,
    slot_name: &str,
) -> Operand {
    let is_undef = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(crate::ssa::IPred::Eq, tag, Operand::ConstI64(5)),
        Type::Bool,
        None,
    );
    select_i64(ctx, Operand::Value(is_undef), default, idx, slot_name)
}

/// The `lastIndexOf` position slot: `ToNumber(pos)` is NaN → +∞,
/// otherwise `ToIntegerOrInfinity(pos)` (§22.1.3.10 steps 5-6). The
/// `i64::MAX` sentinel is what the helper already takes for +∞.
///
/// An integer-shaped operand can never be NaN, and a constant one
/// answers at compile time, so only an `F64` value pays for the test.
fn lower_lastindexof_pos(ctx: &mut LowerCtx<'_>, a: ExprId) -> Operand {
    let n = ctx.lower_to_number_operand(a);
    if let Operand::ConstF64(v) = n {
        return if v.is_nan() {
            Operand::ConstI64(i64::MAX)
        } else {
            ctx.coerce_to_i64(n)
        };
    }
    if !matches!(ctx.operand_ty(&n), Type::F64) {
        return ctx.coerce_to_i64(n);
    }
    let idx = ctx.coerce_to_i64(n.clone());
    select_on_nan(ctx, n, Operand::ConstI64(i64::MAX), idx, "__str_pos_slot")
}

/// `<n is NaN> ? default : idx`, the number-side twin of
/// [`select_on_undef_tag`] for a slot whose spec reads NaN itself as
/// the default (`Une` is the unordered `n != n`).
fn select_on_nan(
    ctx: &mut LowerCtx<'_>,
    n: Operand,
    default: Operand,
    idx: Operand,
    slot_name: &str,
) -> Operand {
    let is_nan = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::FCmp(crate::ssa::FPred::Une, n.clone(), n),
        Type::Bool,
        None,
    );
    select_i64(ctx, Operand::Value(is_nan), default, idx, slot_name)
}
