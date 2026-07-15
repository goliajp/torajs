//! v0.2 #1 Phase 1b — `<Str>.{replace|replaceAll|split|match|matchAll}`
//! regex-receiver intercept pulled out of
//! [`crate::ssa_lower::lower_expr_inner`] `Expr::Call` dispatch as chunk-7
//! of the `Expr::Call` god-arm decomp (chunks 1-6 = Arr higher-order +
//! Map dispatch + Set dispatch + Arr.push + Number methods + bare-name
//! globals).
//!
//! Six methods (search joined at chunk 800) share
//! `Expr::Member { obj, name }` + first-arg-is-RegExp
//! peek; when both gates pass, dispatch routes to `__torajs_regex_*`
//! intrinsics. The non-regex `(Str, Str)` path is owned by
//! `ssa_lower_str::try_lower_method_call` (the M6.1 sidekick) downstream
//! — this block intercepts only when the first arg is statically a
//! regex (literal `/.../flags` or Ident with tracked Type::RegExp).
//! Substr receivers materialize through `substr_to_owned` before the
//! runtime call (chunk 800 — the byte reader misreads view blocks).
//!
//! Returns `Some(result)` when the regex intercept dispatches; `None`
//! lets the caller fall through to the M6.1 String/Substr/Array stdlib
//! sidekick + the subsequent Arr predicate-iterator family.

use crate::ast::{Expr, ExprId};
use crate::ssa::{IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx, count_capture_groups, intern_arr_layout};

/// Try to lower `<Str>.{replace|replaceAll|split|match|matchAll}` when the
/// first arg is a RegExp. Returns `Some` when dispatched, `None` otherwise.
pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let (obj, name) = match ctx.ast.get_expr(callee) {
        Expr::Member { obj, name } => (*obj, name.clone()),
        _ => return None,
    };
    if !matches!(
        name.as_str(),
        "replace" | "replaceAll" | "split" | "match" | "matchAll" | "search"
    ) {
        return None;
    }
    if args.is_empty() {
        return None;
    }
    // Detection: peek the AST without lowering to avoid double side-effects
    // (re-evaluating the receiver if we were to fall through). Recognized
    // regex args:
    //   - Expr::Regex { ... } — literal `/.../flags`
    //   - Expr::Ident(name) — a local whose tracked SSA type is Type::RegExp
    // Anything else (incl. computed RegExp from a function call) falls
    // through to the existing string path, which currently rejects RegExp
    // args via Type::Any signature. A v0.2 #1.c follow-up can broaden the
    // detection — for now the literal + ident forms cover the dominant
    // idioms and all the test262 cases at hand.
    let arg0_is_regex = match ctx.ast.get_expr(args[0]) {
        Expr::Regex { .. } => true,
        Expr::Ident(n) => ctx
            .locals
            .get(n)
            .map(|info| info.ty == Type::RegExp)
            .unwrap_or(false),
        _ => false,
    };
    // RFC 20260716 刀 9 — `match` / `matchAll` accept a non-RegExp
    // arg via ES §22.1.3.{11,12} step 4.c: `RegExpCreate(ToString(P),
    // flags)`. match uses `""` flags; matchAll uses `"g"` (matchAll
    // kernel throws TypeError on non-global — the coerced RegExp
    // must be global implicitly). Emit the coerce inline below and
    // fall through to the shared dispatch block.
    let coerce_match_lane = !arg0_is_regex && matches!(name.as_str(), "match" | "matchAll");
    if !arg0_is_regex && !coerce_match_lane {
        return None;
    }
    let raw_recv = ctx.lower_expr(obj);
    // Chunk 800 — a Substr receiver (charAt view, for-of-str char,
    // exec capture) is a 16-byte parent-pointer block the runtime
    // `str_slice` reader would misread as an owned Str header
    // (probe: `ch.match(/o/)` answered null on a matching char,
    // `ch.split(/x/)` answered mojibake). Materialize through
    // `substr_to_owned` — the chunk-699 test/exec haystack dance —
    // and drop the fresh temp after the call. Owned-Str receivers
    // pass through.
    let recv_is_view = ctx.operand_ty(&raw_recv) == Type::Substr;
    let recv_op = if recv_is_view {
        let owned = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.substr_to_owned, vec![raw_recv]),
            Type::Str,
            None,
        );
        Operand::Value(owned)
    } else {
        raw_recv
    };
    let (re_op, minted_regex) = if coerce_match_lane {
        // Lower the arg, coerce to Str (or borrow if already Str),
        // mint a fresh RegExp via `regex_compile`. `regex_compile`
        // takes ownership of neither arg — the caller drops both
        // Str temps if they were freshly minted (coerce_to_str
        // materializes ownership per its contract). matchAll gets
        // the "g" flag per spec §22.1.3.12 step 4.c.
        let raw_arg = ctx.lower_expr(args[0]);
        let arg_ty = ctx.operand_ty(&raw_arg);
        let pat_op = ctx.coerce_to_str(raw_arg, arg_ty);
        let flags_bytes = if name == "matchAll" { "g" } else { "" };
        let flags_v = ctx.intern_string_literal(flags_bytes);
        let re_v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.regex_compile,
                vec![pat_op.clone(), Operand::Value(flags_v)],
            ),
            Type::RegExp,
            None,
        );
        // Drop the coerced Str temp — the compile call read the
        // bytes and holds no borrow. `intern_string_literal`
        // returns a static-lifetime Str with rc_inc-ed usage so no
        // drop needed for the flags literal.
        if arg_ty != Type::Str {
            ctx.emit_drop_value(pat_op, Type::Str);
        }
        (Operand::Value(re_v), true)
    } else {
        (ctx.lower_expr(args[0]), false)
    };
    let arr_id = intern_arr_layout(ctx.arr_layouts, Type::Str);
    let result = match name.as_str() {
        "match" => emit_match(ctx, recv_op.clone(), re_op.clone(), args, arr_id),
        "matchAll" => emit_match_all(ctx, recv_op.clone(), re_op.clone(), args, arr_id),
        "split" => emit_split(ctx, recv_op.clone(), re_op, args, arr_id),
        "replace" | "replaceAll" => emit_replace(ctx, recv_op.clone(), re_op, &name, args),
        "search" => emit_search(ctx, recv_op.clone(), re_op, args),
        _ => unreachable!(),
    };
    if minted_regex {
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.regex_drop, vec![re_op]),
        );
    }
    if recv_is_view {
        ctx.emit_drop_value(recv_op, Type::Str);
    }
    Some(result)
}

/// `s.search(re)` — ES §22.1.3.19 (chunk 800). The runtime helper
/// anchors sticky at 0 / scans from 0 and never touches lastIndex
/// (§22.2.6.12 saves/restores it); returns the UTF-16 match index
/// or -1.
fn emit_search(
    ctx: &mut LowerCtx<'_>,
    recv_op: Operand,
    re_op: Operand,
    args: &[ExprId],
) -> Operand {
    // Trailing-arg ignore per the S286 family idiom: spec reads
    // only `re`, step()-style side-effect exprs still fire.
    for &a in args.iter().skip(1) {
        let _ = ctx.lower_expr(a);
    }
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.regex_search, vec![recv_op, re_op]),
        Type::I64,
        None,
    );
    Operand::Value(v)
}

fn emit_match(
    ctx: &mut LowerCtx<'_>,
    recv_op: Operand,
    re_op: Operand,
    args: &[ExprId],
    arr_id: crate::ssa::ArrId,
) -> Operand {
    // S286 — trailing-arg ignore per ES §22.1.3.11: spec reads only `re`,
    // but step()-style side-effect exprs must fire per ES eval-then-discard.
    for &a in args.iter().skip(1) {
        let _ = ctx.lower_expr(a);
    }
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.regex_match, vec![recv_op, re_op]),
        Type::Arr(arr_id),
        None,
    );
    Operand::Value(v)
}

fn emit_match_all(
    ctx: &mut LowerCtx<'_>,
    recv_op: Operand,
    re_op: Operand,
    args: &[ExprId],
    arr_id: crate::ssa::ArrId,
) -> Operand {
    // S286 — trailing-arg ignore per ES §22.1.3.13: same as match arm.
    for &a in args.iter().skip(1) {
        let _ = ctx.lower_expr(a);
    }
    // outer = Array<Array<Str>>, inner arr_id = Array<Str> from caller.
    let outer_id = intern_arr_layout(ctx.arr_layouts, Type::Arr(arr_id));
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.regex_match_all, vec![recv_op, re_op]),
        Type::Arr(outer_id),
        None,
    );
    // P9.4 follow-up — matchAll throws TypeError per ES §22.1.3.13 when re
    // lacks `g`. The runtime helper sets the catchable throw slot; emit
    // the post-call check here (intrinsic fast-path skips it by default).
    // Mirrors the bigint_op_may_throw pattern.
    ctx.emit_throw_check(None);
    Operand::Value(v)
}

fn emit_split(
    ctx: &mut LowerCtx<'_>,
    recv_op: Operand,
    re_op: Operand,
    args: &[ExprId],
    arr_id: crate::ssa::ArrId,
) -> Operand {
    // S326 — regex-receiver `s.split(re, limit, ...trailing)` per ES
    // §22.1.3.21. Mirror of S282 string path in ssa_lower_str.rs:1084.
    // Pre-S326 the regex_split call ignored args[1..] entirely → two bugs
    // in one arm: (1) `s.split(/,/, 3)` silently returned the full split
    // instead of limited; (2) any step()-style trailing arg got silent-
    // dropped at lower-time.
    let split_v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.regex_split, vec![recv_op, re_op]),
        Type::Arr(arr_id),
        None,
    );
    if args.len() < 2 {
        return Operand::Value(split_v);
    }
    // S326 — lower args[2..] for side effects before slicing (S272/S321
    // idiom), then drop their values.
    for &a in args.iter().skip(2) {
        let _ = ctx.lower_expr(a);
    }
    // S326 — ES §22.1.3.21 step 6: limit ToUint32 then clamp to resulting
    // length. Match the S282 string-path slice shape.
    let limit_raw = ctx.lower_expr(args[1]);
    let limit_op = ctx.coerce_to_i64(limit_raw);
    let len = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(split_v), ARR_LEN_OFF),
        Type::I64,
        None,
    );
    let take_slot = ctx.alloca(Type::I64, Some("__regex_split_take"));
    let lt = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Slt, limit_op.clone(), Operand::Value(len)),
        Type::Bool,
        None,
    );
    let then_blk = ctx.f.add_block();
    let else_blk = ctx.f.add_block();
    let after_blk = ctx.f.add_block();
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(lt),
            then_blk,
            else_blk,
        },
    );
    ctx.cur_block = then_blk;
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(limit_op, Operand::Value(take_slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(after_blk));
    ctx.cur_block = else_blk;
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(len), Operand::Value(take_slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(after_blk));
    ctx.cur_block = after_blk;
    let take = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(take_slot), 0),
        Type::I64,
        None,
    );
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.arr_slice,
            vec![
                Operand::Value(split_v),
                Operand::ConstI64(0),
                Operand::Value(take),
            ],
        ),
        Type::Arr(arr_id),
        None,
    );
    Operand::Value(v)
}

fn emit_replace(
    ctx: &mut LowerCtx<'_>,
    recv_op: Operand,
    re_op: Operand,
    method: &str,
    args: &[ExprId],
) -> Operand {
    let repl = ctx.lower_expr(args[1]);
    // S321 — ES §22.1.3.{18,19} silently ignore args past (regex,
    // replacement). Pre-S321 the `debug_assert_eq!(args.len(), 2)` carve-out
    // (release-build assert disabled) skipped lowering args[2..] →
    // step()-style trailing args silent-drop. Mirror S272 idiom by lowering
    // each trailing arg for its side-effects before the regex call. Sister
    // to S286 (match / matchAll) on the same regex-receiver dispatch family.
    for &a in args.iter().skip(2) {
        let _ = ctx.lower_expr(a);
    }
    let repl_ty = ctx.operand_ty(&repl);
    // P9.5-A1 / A1.1 — fn-callback dispatch. Repl is either Str (existing
    // $&/$N expand_repl path) or Closure (env+8 ABI runtime invoke, with
    // N capture Strs built from saves[] per match).
    //
    // For Closure: count regex captures statically from the literal pattern
    // and validate cb sig is exactly `[Str; N+1] -> Str`. Mismatches +
    // N > 9 panic at compile-time; never silent-wrong from cb cast
    // mismatch.
    // Returns (target_fid, Some((n_caps, has_off_input))) for Closure paths,
    // or (target_fid, None) for the Str path.
    let (target, opt_extras) = match (&repl_ty, method) {
        (Type::Str, "replace") => (ctx.intrinsics.regex_replace, None),
        (Type::Str, "replaceAll") => (ctx.intrinsics.regex_replace_all, None),
        (Type::Closure(sig_id), m_name) => {
            let (params, ret_ty) = ctx.fn_sigs[sig_id.0 as usize].clone();
            if ret_ty != Type::Str {
                panic!("ssa-lower: P9.5 s.{m_name}(re, fn) — fn ret must be Str, got {ret_ty:?}");
            }
            // Detect cb shape:
            //  - A1.1: [Str; N+1]            → has_off_input=0
            //  - A1.2: [Str; N+1, I64, Str]  → has_off_input=1
            // (A1.2 trailing slots are (offset:number, input:string) per
            //  ES spec.)
            let (n_caps_expected, has_off_input) = if params.len() >= 3
                && params[params.len() - 2] == Type::I64
                && params[params.len() - 1] == Type::Str
                && params[..params.len() - 2].iter().all(|t| t == &Type::Str)
            {
                (params.len() - 3, true)
            } else if !params.is_empty() && params.iter().all(|t| t == &Type::Str) {
                (params.len() - 1, false)
            } else {
                panic!(
                    "ssa-lower: P9.5 s.{m_name}(re, fn) — fn must have shape \
                         `(Str, ..., Str) => Str` (A1.1) or `(Str, ..., Str, number, \
                         string) => Str` (A1.2), got params={params:?} ret={ret_ty:?}"
                );
            };
            if n_caps_expected > 9 {
                panic!(
                    "ssa-lower: P9.5-A1.1 s.{m_name}(re, fn) — max 9 capture-group \
                         cb args, got {n_caps_expected}. Use the Str-repl form for this case."
                );
            }
            // For ident-bound regex tora can't count captures statically;
            // A1.1 narrow scope = inline regex literal for N ≥ 1.
            let n_caps_actual = match ctx.ast.get_expr(args[0]) {
                Expr::Regex { pattern, .. } => count_capture_groups(pattern),
                _ => 0,
            };
            if n_caps_actual != n_caps_expected {
                panic!(
                    "ssa-lower: P9.5-A1.1 s.{m_name}(re, fn) — regex has \
                         {n_caps_actual} capture group(s) but cb takes \
                         {n_caps_expected} capture arg(s). Note: ident-bound regex \
                         always assumed 0 captures; inline the regex literal to use \
                         captures."
                );
            }
            let fid = if m_name == "replace" {
                ctx.intrinsics.regex_replace_fn
            } else {
                ctx.intrinsics.regex_replace_all_fn
            };
            (fid, Some((n_caps_actual, has_off_input)))
        }
        _ => {
            panic!("ssa-lower: s.{method}(re, repl) — repl must be Str or Closure, got {repl_ty:?}")
        }
    };

    let mut call_args = vec![recv_op, re_op, repl];
    if let Some((n_caps, has_off_input)) = opt_extras {
        call_args.push(Operand::ConstI64(n_caps as i64));
        call_args.push(Operand::ConstI64(has_off_input as i64));
    }
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(target, call_args),
        Type::Str,
        None,
    );
    // RFC 20260705 chunk 552 — release an inline arrow's minted env
    // in the replacement slot after the runtime call consumed it
    // (the Str-repl shape is a borrow and stays untouched).
    ctx.release_owned_temp(args[1], &repl);
    Operand::Value(v)
}
