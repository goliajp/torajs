//! §22.1.3.13 / §22.1.3.20 String.prototype.{match,search} step 3 —
//! the custom `@@match` / `@@search` branch for an Any-typed pattern
//! argument (rotation 262).
//!
//! `"".match(obj)` where `obj[Symbol.match] = fn` (and the search
//! twin): step 3.a probes GetMethod(regexp, @@sym); when present the
//! matcher runs with the pattern as `this` and the receiver string
//! as sole argument (step 3.c), otherwise the step-4
//! `RegExpCreate(ToString(P), "")` coerce lane answers through the
//! per-method regex kernel. The probe/invoke pair lives in
//! torajs-anyvalue `str_match_custom`; the split keeps the SSA
//! branch keyed off a plain I64 (join through an Any slot — the
//! alloca-store-load idiom every branch-shaped lowering here uses).
//!
//! The checker mirrors this exact gate
//! ([`crate::check_type_of_call_string_match`]): a single-arg
//! `s.match(x)` / `s.search(x)` with `x: any` and store evidence
//! types `any`, so the custom lane's arbitrary user return and the
//! coerce lane's boxed result agree with the static face.

use crate::ast::ExprId;
use crate::ssa::{IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;

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
    let probe = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.any_str_symbol_probe,
            vec![raw_arg.clone(), Operand::ConstI64(wk_idx)],
        ),
        Type::I64,
        None,
    );
    let hit = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Ne, Operand::Value(probe), Operand::ConstI64(0)),
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
    // Step 3.c — Call(matcher, pattern, «S»); a user throw (or the
    // GetMethod not-callable TypeError) propagates before the store.
    ctx.cur_block = custom_blk;
    let r = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.any_str_symbol_invoke,
            vec![recv_op.clone(), raw_arg.clone(), Operand::ConstI64(wk_idx)],
        ),
        Type::Any,
        None,
    );
    ctx.emit_throw_check(None);
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(r), Operand::Value(result_slot), 0),
    );
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
                    vec![recv_op.clone(), Operand::Value(re_v)],
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
