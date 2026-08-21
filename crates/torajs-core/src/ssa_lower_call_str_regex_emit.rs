//! The `__torajs_regex_*` kernel emitters for
//! [`crate::ssa_lower_call_str_regex_methods`]'s dispatch — carved
//! out verbatim (rotation 262 file-size split when the `@@replace`
//! lane grew the parent past 500 prod lines). One fn per method
//! family; every body is exactly the parent's pre-split text.

use crate::ast::{Expr, ExprId};
use crate::ssa::{IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx, count_capture_groups, intern_arr_layout};

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
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.regex_match,
            vec![recv_op, re_op, want_exec_shape(ctx, callee)],
        ),
        Type::Arr(arr_id),
        None,
    );
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

pub(crate) fn emit_replace(
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
            // Detect cb shape:
            //  - A1.1: [Str; N+1]            → has_off_input=0
            //  - A1.2: [Str; N+1, I64, Str]  → has_off_input=1
            // (A1.2 trailing slots are (offset:number, input:string) per
            //  ES spec.)
            // §22.2.6.11 step 14.j calls the replacer with
            // `Call(replaceValue, undefined, «matched, captures…,
            // position, string»)`, and a function that declares fewer
            // parameters than that simply never sees the rest. So how
            // many arguments to build is the REGEX's to decide; what
            // the callback declares only has to be no MORE than that.
            let (declared, has_off_input) = if params.len() >= 3
                && params[params.len() - 2] == Type::I64
                && params[params.len() - 1] == Type::Str
                && params[..params.len() - 2].iter().all(|t| t == &Type::Str)
            {
                (params.len() - 3, true)
            } else if params.iter().all(|t| t == &Type::Str) {
                // An empty list lands here on purpose: `function () {
                // … }` declares nothing at all, which is legal and is
                // how t262 spells a replacer that only needs a
                // constant.
                (params.len().saturating_sub(1), false)
            } else {
                panic!(
                    "ssa-lower: P9.5 s.{m_name}(re, fn) — fn must have shape \
                         `(Str, ..., Str) => Str` (A1.1) or `(Str, ..., Str, number, \
                         string) => Str` (A1.2), got params={params:?} ret={ret_ty:?}"
                );
            };
            // For ident-bound regex tora can't count captures statically;
            // A1.1 narrow scope = inline regex literal for N ≥ 1.
            let n_caps_actual = match ctx.ast.get_expr(args[0]) {
                Expr::Regex { pattern, .. } => count_capture_groups(pattern),
                _ => 0,
            };
            if n_caps_actual > 9 {
                panic!(
                    "ssa-lower: P9.5-A1.1 s.{m_name}(re, fn) — max 9 capture-group \
                         cb args, regex has {n_caps_actual}. Use the Str-repl form \
                         for this case."
                );
            }
            // Naming the offset / input tail pins every position, so a
            // callback that names them while declaring fewer captures
            // than the regex has would read a capture Str as the
            // number — that one keeps the loud reject rather than
            // being handed the wrong argument.
            let short = if has_off_input {
                declared != n_caps_actual
            } else {
                declared > n_caps_actual
            };
            if short {
                panic!(
                    "ssa-lower: P9.5-A1.1 s.{m_name}(re, fn) — regex has \
                         {n_caps_actual} capture group(s) but cb takes \
                         {declared} capture arg(s). A cb may take FEWER than the \
                         regex has, but not more, and the (number, string) tail \
                         has to name every capture before it. Note: ident-bound \
                         regex always assumed 0 captures; inline the regex \
                         literal to use captures."
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
    // §22.1.5 — replaceAll throws a TypeError on a non-global RegExp; the
    // kernel records the pending throw, so propagate it here (mirrors the
    // matchAll §22.1.3.13 post-call check). `replace` accepts any regex.
    if method == "replaceAll" {
        ctx.emit_throw_check(None);
    }
    Operand::Value(v)
}
