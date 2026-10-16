//! The `__torajs_regex_*` kernel emitters for
//! [`crate::ssa_lower_call_str_regex_methods`]'s dispatch — carved
//! out verbatim (rotation 262 file-size split when the `@@replace`
//! lane grew the parent past 500 prod lines). One fn per method
//! family; every body is exactly the parent's pre-split text.

mod match_len_direct;
pub(crate) mod replace_inline;

use crate::ast::{Expr, ExprId};
use crate::ssa::{IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx, count_capture_groups, intern_arr_layout};
use replace_inline::typed_callback_shape;

/// `s.search(re)` — ES §22.1.3.19 (chunk 800). The runtime helper
/// anchors sticky at 0 / scans from 0 and never touches lastIndex
/// (§22.2.6.12 saves/restores it); returns the UTF-16 match index
/// or -1.
pub(crate) fn emit_search(
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
    // §22.2.7.2 writes lastIndex under `g` / `y`; a frozen one is a
    // recorded TypeError
    ctx.emit_throw_check(None);
    Operand::Value(v)
}

/// The kernels' trailing `want_exec` operand — 0 when
/// `analyze_regex_result_props` proved this call's result has no
/// reader for the §22.2.7.8 exec-shape properties, so the kernel can
/// skip building the arrprops side table (RFC 20260821 attack B;
/// measured 16.6-18.5 ns per match). `callee` is the `Expr::Member`
/// id the pass keyed its answer by, which is exactly what both regex
/// dispatchers already hold.
pub(crate) fn want_exec_shape(ctx: &LowerCtx<'_>, callee: ExprId) -> Operand {
    Operand::ConstI64(i64::from(
        !ctx.ast.regex_result_props_unread.contains(&callee),
    ))
}

pub(crate) fn emit_match(
    ctx: &mut LowerCtx<'_>,
    recv_op: Operand,
    re_op: Operand,
    args: &[ExprId],
    arr_id: crate::ssa::ArrId,
    callee: ExprId,
) -> Operand {
    // S286 — trailing-arg ignore per ES §22.1.3.11: spec reads only `re`,
    // but step()-style side-effect exprs must fire per ES eval-then-discard.
    for &a in args.iter().skip(1) {
        let _ = ctx.lower_expr(a);
    }
    // A result the census proved is only null-tested or asked
    // `m[0].length` is never built: hold the length, +1 so that 0 is
    // null (`ssa_lower_match_len`).
    if ctx.ast.regex_match_len_callees.contains(&callee) {
        return match_len_direct::emit(ctx, recv_op, re_op, args.first().copied());
    }
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.regex_match,
            vec![recv_op, re_op, want_exec_shape(ctx, callee)],
        ),
        Type::Arr(arr_id),
        None,
    );
    // the walk writes lastIndex under `g` / `y`; a frozen one is a
    // recorded TypeError and the kernel answers the miss (null)
    ctx.emit_throw_check(None);
    Operand::Value(v)
}

pub(crate) fn emit_match_all(
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

pub(crate) fn emit_split(
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
        InstKind::ICmp(IPred::Slt, limit_op, Operand::Value(len)),
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

/// The pattern and flags of `s.replace(re, cb)` when the call spells
/// it or names a top-level binding that does.
pub(crate) fn static_replace_pattern(ast: &crate::ast::Ast, arg0: ExprId) -> Option<(&str, &str)> {
    match ast.get_expr(arg0) {
        Expr::Regex { pattern, flags } => Some((pattern.as_str(), flags.as_str())),
        Expr::Ident(name) => crate::ast::toplevel_regex_literal(ast, name),
        _ => None,
    }
}

/// The capture groups of a static replace pattern: how many there are,
/// and how many leading ones every match sets.
#[derive(Clone, Copy)]
pub(crate) struct StaticCaps {
    pub n: usize,
    pub set: usize,
    /// some group is named, so a replacer also receives the groups object
    pub named: bool,
}

pub(crate) fn static_replace_caps(ast: &crate::ast::Ast, arg0: ExprId) -> Option<StaticCaps> {
    static_replace_pattern(ast, arg0).map(|(pattern, flags)| StaticCaps {
        n: count_capture_groups(pattern),
        set: crate::ssa_lower_regex_bake::always_set_groups(pattern, flags),
        named: crate::ssa_lower_free_helpers::has_named_capture(pattern),
    })
}

/// Whether the kernel lane of `s.replace(re, cb)` calls a closure of
/// this signature through the typed callback ABI rather than its boxed
/// entry; `caps` describes the groups of a static pattern. The ABI
/// splices a returned Str as-is, so a callback answering anything else
/// takes the boxed entry, whose any-replacement kernel runs
/// §22.2.6.11 step 14.k ToString. It fixes the argument list at
/// compile time, so a callback naming more than the match against a
/// regex known only at run time takes it too, as does one shaped other
/// than «matched, captures…[, position, string]» (one that names the
/// groups object, or more captures than the pattern has): the boxed
/// entry builds step 14.j's argument list from what the match produced.
/// So does one naming a group some match may leave unset: the typed
/// slot is a string, and step 14.b binds `undefined` there.
pub(crate) fn callback_stays_typed(params: &[Type], ret: Type, caps: Option<StaticCaps>) -> bool {
    ret == Type::Str
        && (caps.is_some() || params.len() <= 1)
        && typed_callback_shape(params, caps.map_or(0, |c| c.n))
            .is_some_and(|(declared, _)| declared <= caps.map_or(0, |c| c.set))
}

pub(crate) fn emit_replace(
    ctx: &mut LowerCtx<'_>,
    recv_op: Operand,
    re_op: Operand,
    method: &str,
    args: &[ExprId],
    callee: ExprId,
) -> Operand {
    let mut repl = ctx.lower_expr(args[1]);
    // S321 — ES §22.1.3.{18,19} silently ignore args past (regex,
    // replacement). Pre-S321 the `debug_assert_eq!(args.len(), 2)` carve-out
    // (release-build assert disabled) skipped lowering args[2..] →
    // step()-style trailing args silent-drop. Mirror S272 idiom by lowering
    // each trailing arg for its side-effects before the regex call. Sister
    // to S286 (match / matchAll) on the same regex-receiver dispatch family.
    for &a in args.iter().skip(2) {
        let _ = ctx.lower_expr(a);
    }
    // a census-admitted callback runs the walk in this function's own
    // code, on views of the subject; anything else is the kernel's
    if let Some(v) = replace_inline::try_lower(ctx, callee, &recv_op, &re_op, args, &repl) {
        return v;
    }
    let mut repl_ty = ctx.operand_ty(&repl);
    let static_caps = static_replace_caps(ctx.ast, args[0]);
    if let Type::Closure(sig_id) = repl_ty {
        let (params, ret) = &ctx.fn_sigs[sig_id.0 as usize];
        if !callback_stays_typed(params, *ret, static_caps)
            || crate::ast::argv_face_callback::replace_callback_needs_boxed(ctx.ast, args[1])
        {
            repl = ctx.box_to_any(repl);
            repl_ty = Type::Any;
        }
    }
    // P9.5-A1 / A1.1 — fn-callback dispatch. Repl is either Str (existing
    // $&/$N expand_repl path) or Closure (env+8 ABI runtime invoke, with
    // N capture Strs built from saves[] per match).
    //
    // For Closure: count regex captures statically from the literal
    // pattern, and check the cb sig can RECEIVE `[Str; N+1]` — it may
    // declare fewer (the spec's extra arguments go nowhere), but never
    // more, and never a differently-typed slot. A cb that would have
    // to read one argument as another's type panics at compile time;
    // never silent-wrong from a cb cast mismatch.
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
            // a regex known only at run time reaches this arm only with
            // a callback that names the match alone
            let n_caps_actual = static_caps.map_or(0, |c| c.n);
            let Some((_, has_off_input)) = typed_callback_shape(&params, n_caps_actual) else {
                unreachable!("a callback the typed ABI cannot serve was boxed above");
            };
            let fid = if m_name == "replace" {
                ctx.intrinsics.regex_replace_fn
            } else {
                ctx.intrinsics.regex_replace_all_fn
            };
            (fid, Some((n_caps_actual, has_off_input)))
        }
        // §22.1.3.19 step 5 is IsCallable, asked at RUN time, and an
        // `any` replacement is the one shape this lane cannot answer
        // statically — it used to refuse the program instead
        // (`s.replace(/b/, getRepl())`, and every `any`-annotated
        // parameter). The kernel asks the question and reaches the
        // same two legs; the call keeps its `Type::Str`, which is why
        // it goes here rather than to the any lane.
        (Type::Any, _) => (ctx.intrinsics.regex_replace_any_repl, None),
        _ => {
            panic!("ssa-lower: s.{method}(re, repl) — repl must be Str or Closure, got {repl_ty:?}")
        }
    };

    let any_repl = matches!(repl_ty, Type::Any);
    let mut call_args = vec![recv_op, re_op, repl];
    if any_repl {
        call_args.push(Operand::ConstI64((method == "replaceAll") as i64));
    }
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
    // Three ways this call leaves a pending throw, and every one of
    // them needs the check right here — otherwise it rides past the
    // `try` the program wrote around the replace and ends the process
    // uncaught:
    //
    // - the any-replacement kernel's ToString(replaceValue);
    // - §22.1.5, replaceAll on a non-global RegExp (mirrors the
    //   matchAll §22.1.3.13 post-call check; `replace` accepts any
    //   regex);
    // - **a callback that threw** — its own `throw`, or a guard the
    //   KERNEL raised inside it, such as a method call on the
    //   `string | undefined` an unmatched capture group binds.
    //
    // The third is why the typed-callback spelling of `replace` was
    // the one shape with no check: it is neither `any_repl` nor
    // `replaceAll`, so `"x".replace(/(y)?x/, (m, p1) => p1.toUpperCase())`
    // exited uncaught with the `try` standing right there. An explicit
    // `throw` in the same callback was caught, which is what made it
    // read as a regex bug rather than a missing check.
    //
    // - and the fourth, on every lane: §22.2.7.2's lastIndex store
    //   under `g` / `y`, which a frozen lastIndex refuses.
    //
    // The string-replacement kernels answer a fresh copy of the
    // subject on their throw paths, so the check owns that copy.
    if any_repl || opt_extras.is_some() {
        ctx.emit_throw_check(None);
    } else {
        ctx.emit_throw_check_owned(None, Operand::Value(v), Type::Str);
    }
    Operand::Value(v)
}

/// `s.replace(x, r)` / `s.replaceAll(x, r)` with `x` in an `any` slot
/// and a replaceValue no typed kernel takes (`any`, number, boolean,
/// nullish, object, or absent). Steps 2 to 6 of §22.1.3.19 are all
/// run-time questions here (is `x` a RegExp, is `r` callable, when is
/// `r` ToString'd), so the call rides the any-receiver dispatcher,
/// which answers each of them on the live values. Without store
/// evidence for a symbol face (the caller's gate) the answer is the
/// string replace produces; its ToString is the unbox.
#[cold]
#[inline(never)]
pub(crate) fn emit_replace_any_dyn(
    ctx: &mut LowerCtx<'_>,
    recv_op: Operand,
    args: &[ExprId],
    name: &str,
) -> Operand {
    let recv_any = ctx.box_to_any(recv_op);
    let out = crate::ssa_lower_any_method_call::emit_any_method_call(
        ctx,
        recv_any,
        name,
        Operand::ConstPtrNull,
        args,
    );
    ctx.emit_throw_check_owned(None, Operand::Value(out), Type::Any);
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.any_to_str_box, vec![Operand::Value(out)]),
        Type::Str,
        None,
    );
    ctx.emit_drop_value(Operand::Value(out), Type::Any);
    Operand::Value(v)
}
