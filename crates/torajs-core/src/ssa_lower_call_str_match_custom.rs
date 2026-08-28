//! §22.1.3.13 / §22.1.3.20 String.prototype.{match,search} step 3 —
//! the custom `@@match` / `@@search` branch for an Any-typed pattern
//! argument (rotation 262).
//!
//! `"".match(obj)` where `obj[Symbol.match] = fn` (and the search
//! twin): step 3.a runs GetMethod(regexp, @@sym); when it answers a
//! method the matcher runs with the pattern as `this` and the
//! receiver string as sole argument (step 3.c), otherwise the step-4
//! `RegExpCreate(ToString(P), "")` coerce lane answers through the
//! per-method regex kernel. Both steps live in one torajs-anyvalue
//! call, `str_match_custom`'s `__torajs_any_str_symbol_try` — it was
//! a presence probe plus a second walk, which an ACCESSOR-shaped
//! matcher would have run the getter twice for, and which could not
//! reach the "the getter answered nullish, use the coerce lane"
//! verdict at all. The SSA branch still keys off a plain I64, now the
//! single GetMethod's verdict, with the result handed back through
//! the Any result slot (the alloca-store-load idiom every
//! branch-shaped lowering here uses).
//!
//! The checker mirrors this exact gate
//! ([`crate::check_type_of_call_string_match`]): a single-arg
//! `s.match(x)` / `s.search(x)` with `x: any` and store evidence
//! types `any`, so the custom lane's arbitrary user return and the
//! coerce lane's boxed result agree with the static face.

use crate::ast::ExprId;
use crate::ssa::{IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;

/// `undefined` as an Any operand — the `extra` slot the «S» faces
/// pass and the callee never reads (ANY_UNDEF = 5, payload ignored).
fn undef_any(ctx: &mut LowerCtx<'_>) -> Operand {
    let u = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.any_box,
            vec![Operand::ConstI64(5), Operand::ConstI64(0)],
        ),
        Type::Any,
        None,
    );
    Operand::Value(u)
}

/// Which symbol-dispatch method face is lowering — picks the
/// well-known index operand and the coerce lane's regex kernel.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum SymbolDispatchKind {
    /// `@@match`, wk idx 6; coerce answers `regex_match` (Arr).
    Match,
    /// `@@search`, wk idx 9; coerce answers `regex_search` (I64).
    Search,
}

/// Lower `s.match(x)` / `s.search(x)` with an Any-typed `x`.
/// `recv_op` is the (already Substr-materialized) Str receiver; the
/// caller handles its view-drop after this returns. Answers a
/// `Type::Any` operand.
pub(crate) fn lower_symbol_dispatch_pattern(
    ctx: &mut LowerCtx<'_>,
    recv_op: Operand,
    args: &[ExprId],
    kind: SymbolDispatchKind,
) -> Operand {
    let wk_idx = match kind {
        SymbolDispatchKind::Match => 6,
        SymbolDispatchKind::Search => 9,
    };
    // Argument evaluation in source order: the pattern first, then
    // trailing args per the S286 eval-then-discard idiom (spec reads
    // only the pattern; step()-style side effects still fire).
    let raw_arg = ctx.lower_expr(args[0]);
    for &a in args.iter().skip(1) {
        let _ = ctx.lower_expr(a);
    }
    let result_slot = ctx.alloca(Type::Any, Some("__symdisp_result"));
    let extra_any = undef_any(ctx);
    let handled = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.any_str_symbol_try,
            vec![
                recv_op.clone(),
                raw_arg.clone(),
                extra_any,
                Operand::ConstI64(1),
                Operand::ConstI64(wk_idx),
                Operand::Value(result_slot),
            ],
        ),
        Type::I64,
        None,
    );
    let hit = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Ne, Operand::Value(handled), Operand::ConstI64(0)),
        Type::Bool,
        None,
    );
    let custom_blk = ctx.f.add_block();
    let coerce_blk = ctx.f.add_block();
    let after_blk = ctx.f.add_block();
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(hit),
            then_blk: custom_blk,
            else_blk: coerce_blk,
        },
    );
    // Step 3.c already ran inside the call above and wrote the result
    // slot; a user throw (or the GetMethod not-callable TypeError)
    // surfaces here.
    ctx.cur_block = custom_blk;
    ctx.emit_throw_check(None);
    ctx.f.set_term(ctx.cur_block, Terminator::Br(after_blk));
    // Step 4 — RegExpCreate(ToString(pattern), "") then the
    // per-method regex kernel, boxed to Any so both lanes agree at
    // the join.
    ctx.cur_block = coerce_blk;
    let arg_ty = ctx.operand_ty(&raw_arg);
    let pat_op = crate::ssa_lower_call_coercion::emit_to_string(
        ctx,
        args[0],
        raw_arg.clone(),
        arg_ty,
        false,
    );
    let flags_v = ctx.intern_string_literal("");
    let re_v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.regex_compile,
            vec![pat_op.clone(), Operand::Value(flags_v)],
        ),
        Type::RegExp,
        None,
    );
    ctx.emit_drop_value(pat_op, Type::Str);
    let coerced = match kind {
        SymbolDispatchKind::Match => {
            let arr_id = crate::ssa_lower::intern_arr_layout(ctx.arr_layouts, Type::Str);
            let arr_v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.regex_match,
                    // The @@match coercion lane mints its RegExp at
                    // runtime from a value the caller may hand on
                    // anywhere; no static reader picture, so it keeps
                    // the full §22.2.7.8 exec shape.
                    vec![recv_op.clone(), Operand::Value(re_v), Operand::ConstI64(1)],
                ),
                Type::Arr(arr_id),
                None,
            );
            Operand::Value(arr_v)
        }
        SymbolDispatchKind::Search => {
            let idx_v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.regex_search,
                    vec![recv_op.clone(), Operand::Value(re_v)],
                ),
                Type::I64,
                None,
            );
            Operand::Value(idx_v)
        }
    };
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.regex_drop, vec![Operand::Value(re_v)]),
    );
    let coerced_any = ctx.box_to_any(coerced);
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(coerced_any, Operand::Value(result_slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(after_blk));
    ctx.cur_block = after_blk;
    let out = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Any, Operand::Value(result_slot), 0),
        Type::Any,
        None,
    );
    Operand::Value(out)
}

/// Lower `s.replace(x, r)` with an Any-typed `x` — §22.1.3.19
/// step 3: probe GetMethod(searchValue, @@replace); when present the
/// replacer runs as Call(replacer, searchValue, «O, replaceValue»),
/// otherwise ToString(searchValue) takes the LITERAL substring
/// replace kernels (`str_replace` / `str_replace_fn` — replace's
/// step 4 never mints a RegExp, unlike match/search). The caller
/// gated `r` to a checker String or Function shape so both lanes
/// can emit. Answers a `Type::Any` operand.
pub(crate) fn lower_replace_any_pattern(
    ctx: &mut LowerCtx<'_>,
    recv_op: Operand,
    args: &[ExprId],
) -> Operand {
    let raw_arg = ctx.lower_expr(args[0]);
    // A single-arg `s.replace(x)` has replaceValue = undefined
    // (the get-err poison shape reads @@replace before anything
    // touches the replacer).
    let repl_op = args.get(1).map(|&a| ctx.lower_expr(a));
    let repl_ty = repl_op.as_ref().map(|op| ctx.operand_ty(op));
    // ES §22.1.3.19 silently ignores args past (pattern,
    // replacement) — eval-then-discard.
    for &a in args.iter().skip(2) {
        let _ = ctx.lower_expr(a);
    }
    let result_slot = ctx.alloca(Type::Any, Some("__symdisp_repl_result"));
    // Step 3.c's replaceValue — the box is borrowed (box_to_any is
    // rc-neutral, the operand's stake stays with this frame until the
    // join's release), so building it ahead of the branch is free.
    let extra_any = match &repl_op {
        Some(op) => ctx.box_to_any(op.clone()),
        None => undef_any(ctx),
    };
    let handled = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.any_str_symbol_try,
            vec![
                recv_op.clone(),
                raw_arg.clone(),
                extra_any,
                Operand::ConstI64(2),
                Operand::ConstI64(8),
                Operand::Value(result_slot),
            ],
        ),
        Type::I64,
        None,
    );
    let hit = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Ne, Operand::Value(handled), Operand::ConstI64(0)),
        Type::Bool,
        None,
    );
    let custom_blk = ctx.f.add_block();
    let coerce_blk = ctx.f.add_block();
    let after_blk = ctx.f.add_block();
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(hit),
            then_blk: custom_blk,
            else_blk: coerce_blk,
        },
    );
    // Step 3.c — Call(replacer, searchValue, «O, replaceValue») ran
    // inside the call above and wrote the result slot.
    ctx.cur_block = custom_blk;
    ctx.emit_throw_check(None);
    ctx.f.set_term(ctx.cur_block, Terminator::Br(after_blk));
    // Step 4-onward fallback — ToString(searchValue), then the
    // literal substring kernels the string-pattern spelling uses.
    ctx.cur_block = coerce_blk;
    let arg_ty = ctx.operand_ty(&raw_arg);
    let pat_op = crate::ssa_lower_call_coercion::emit_to_string(
        ctx,
        args[0],
        raw_arg.clone(),
        arg_ty,
        false,
    );
    let (fid, repl_arg) = match (&repl_op, &repl_ty) {
        (Some(op), Some(Type::Closure(_))) => (ctx.intrinsics.str_replace_fn, op.clone()),
        (Some(op), _) => (ctx.intrinsics.str_replace, op.clone()),
        // ToString(undefined) — the absent replaceValue substitutes
        // the literal text per the string-replacer path.
        (None, _) => {
            let lit = ctx.intern_string_literal("undefined");
            (ctx.intrinsics.str_replace, Operand::Value(lit))
        }
    };
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(fid, vec![recv_op.clone(), pat_op.clone(), repl_arg]),
        Type::Str,
        None,
    );
    // The fn replacer can throw — propagate the pending throw.
    ctx.emit_throw_check(None);
    ctx.emit_drop_value(pat_op, Type::Str);
    let v_any = ctx.box_to_any(Operand::Value(v));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(v_any, Operand::Value(result_slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(after_blk));
    ctx.cur_block = after_blk;
    // Release an inline closure literal's minted env after whichever
    // lane consumed it (the str_replace_fn mirror; both lanes route
    // through this join).
    if let (Some(&a1), Some(op)) = (args.get(1), &repl_op) {
        ctx.release_owned_temp(a1, op);
    }
    let out = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Any, Operand::Value(result_slot), 0),
        Type::Any,
        None,
    );
    Operand::Value(out)
}

/// Lower `s.split(x, limit?)` with an Any-typed `x` carrying store
/// evidence — §22.1.3.23 step 2: probe GetMethod(separator,
/// @@split); when present the splitter runs as Call(splitter,
/// separator, «O, limit») with the limit passed through RAW (step 2
/// precedes step 4's ToUint32), otherwise the existing any-separator
/// three-way kernel answers with the step 4-onward behavior (limit
/// clamp included). The caller (`ssa_lower_str_str_split`) hands the
/// already-lowered `argv = [recv, sep, limit?]`. Answers a
/// `Type::Any` operand.
pub(crate) fn lower_split_any_pattern(ctx: &mut LowerCtx<'_>, argv: Vec<Operand>) -> Operand {
    let result_slot = ctx.alloca(Type::Any, Some("__symdisp_split_result"));
    // Both lanes consume the limit as an AnyValue: the splitter gets
    // it RAW (step 2 precedes step 4's ToUint32), and the fallback
    // kernel runs the ToUint32 itself. Absent = undefined (ANY_UNDEF
    // = 5, payload ignored).
    let limit_any = match argv.get(2) {
        Some(op) => ctx.box_to_any(op.clone()),
        None => {
            let u = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.any_box,
                    vec![Operand::ConstI64(5), Operand::ConstI64(0)],
                ),
                Type::Any,
                None,
            );
            Operand::Value(u)
        }
    };
    let handled = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.any_str_symbol_try,
            vec![
                argv[0].clone(),
                argv[1].clone(),
                limit_any.clone(),
                Operand::ConstI64(2),
                Operand::ConstI64(11),
                Operand::Value(result_slot),
            ],
        ),
        Type::I64,
        None,
    );
    let hit = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Ne, Operand::Value(handled), Operand::ConstI64(0)),
        Type::Bool,
        None,
    );
    let custom_blk = ctx.f.add_block();
    let coerce_blk = ctx.f.add_block();
    let after_blk = ctx.f.add_block();
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(hit),
            then_blk: custom_blk,
            else_blk: coerce_blk,
        },
    );
    // Step 2.b — Call(splitter, separator, «O, limit») ran inside the
    // call above and wrote the result slot.
    ctx.cur_block = custom_blk;
    ctx.emit_throw_check(None);
    ctx.f.set_term(ctx.cur_block, Terminator::Br(after_blk));
    // Step 4-onward fallback — the limit-carrying any-separator
    // kernel (undefined → [S] / RegExp cell → regex split / else
    // ToString; the limit's ToUint32 runs inside, per step 4's
    // placement AFTER the splitter probe). Answers an AnyValue box
    // so both lanes agree at the join.
    ctx.cur_block = coerce_blk;
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.str_split_any_sep_lim,
            vec![argv[0].clone(), argv[1].clone(), limit_any],
        ),
        Type::Any,
        None,
    );
    ctx.emit_throw_check(None);
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(v), Operand::Value(result_slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(after_blk));
    ctx.cur_block = after_blk;
    let out = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Any, Operand::Value(result_slot), 0),
        Type::Any,
        None,
    );
    Operand::Value(out)
}
