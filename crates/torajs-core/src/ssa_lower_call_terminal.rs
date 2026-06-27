//! Terminal direct-call emit pulled out of
//! [`crate::ssa_lower::lower_expr_inner`]'s `Expr::Call` arm as
//! chunk-84a of the decomp. The 37+ specialized `try_lower`
//! sibling cascade (see [`crate::ssa_lower::lower_expr_inner`])
//! claims its case-shape; whatever falls through lands here and
//! emits the generic SSA `Call` instruction.
//!
//! Pipeline:
//!
//! 1. **M3 generic call retarget** — `call_retargets[eid]` (recorded
//!    by the typechecker for monomorphized generics) overrides
//!    `resolve_callee`'s ident lookup.
//! 2. **Math.sumPrecise dispatch swap** — default `math_sum_precise`
//!    is the F64-array helper; swap to `math_sum_precise_i64` when
//!    the single arg is statically `Type::Arr<I64>` (literal-int
//!    sources lower that way).
//! 3. **T-28 trailing-Any pad** — `arity_pad_count[eid]` (when all
//!    trailing missing params were `Type::Any`) → push one
//!    ANY_UNDEF Any-box per missing slot to align argv with the
//!    callee's static signature.
//! 4. **Per-call-site consume bitmap** — `ast.consuming_params`
//!    transfers ownership from caller's binding to callee. Mirrors
//!    `check.rs`. `__new_*` ctors consume every arg.
//! 5. **Argument coercion** — Math.* unary/binary takes `f64` (with
//!    `Any → any_to_number` for S342); generic fn_sigs walk widens
//!    `I64/Bool → F64`, truncates `F64/Bool → I64` (matches JS
//!    `ToInt32` / `ToUint32` prefix for indexes / codepoints / bit
//!    positions); `Any` param + concrete arg routes through
//!    `box_to_any_from_expr` (P0.9 + S126-3 — `undefined`/`null`
//!    literals keep `ANY_UNDEF`/`ANY_NULL` tags instead of
//!    collapsing to ConstPtrNull).
//! 6. **Emit `Call`** with `f_ret_type_hint(target)` + `emit_throw_check`.

use crate::ast::{Expr, ExprId};
use crate::ssa::{FuncId, InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn emit(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    callee: ExprId,
    args: &[ExprId],
) -> Operand {
    let mut target = resolve_target(ctx, eid, callee);
    let mut argv: Vec<Operand> = args.iter().map(|a| ctx.lower_expr(*a)).collect();
    target = maybe_swap_math_sum_precise(ctx, target, &argv);
    pad_trailing_undef(ctx, eid, &mut argv);
    apply_consume_bitmap(ctx, callee, args);
    coerce_args(ctx, target, args, &mut argv);
    let ret_ty = ctx.f_ret_type_hint(target);
    let cur_block = ctx.cur_block;
    let v = ctx
        .f
        .append_inst(cur_block, InstKind::Call(target, argv), ret_ty, None);
    ctx.emit_throw_check(Some(target));
    Operand::Value(v)
}

fn resolve_target(ctx: &LowerCtx<'_>, eid: ExprId, callee: ExprId) -> FuncId {
    // M3 — generic call retarget. If the typechecker recorded a
    // (mono fn name) for this call's ExprId, look up the specialized
    // FuncId by name and use it instead of the generic ident's
    // resolve.
    if let Some(mono_name) = ctx.call_retargets.get(&eid).cloned() {
        *ctx.fn_table.get(&mono_name).unwrap_or_else(|| {
            panic!("ssa-lower: monomorphized fn `{mono_name}` missing from fn_table")
        })
    } else {
        ctx.resolve_callee(callee)
    }
}

fn maybe_swap_math_sum_precise(ctx: &LowerCtx<'_>, target: FuncId, argv: &[Operand]) -> FuncId {
    if target == ctx.intrinsics.math_sum_precise
        && argv.len() == 1
        && let Type::Arr(arr_id) = ctx.operand_ty(&argv[0])
    {
        let elem_ty = ctx.arr_layouts[arr_id.0 as usize];
        if matches!(elem_ty, Type::I64) {
            return ctx.intrinsics.math_sum_precise_i64;
        }
    }
    target
}

fn pad_trailing_undef(ctx: &mut LowerCtx<'_>, eid: ExprId, argv: &mut Vec<Operand>) {
    // T-28 — pad trailing missing Type::Any params with ANY_UNDEF
    // Any-box operands. check.rs's arity_pad_count recorded the
    // missing count for this Call ExprId iff all the trailing
    // missing params were Type::Any (the typed-tier strict-arity
    // check rejected mixed-typed missing).
    let pad_n = match ctx.arity_pad_count.get(&eid).copied() {
        Some(n) if n > 0 => n,
        _ => return,
    };
    for _ in 0..pad_n {
        let cur_block = ctx.cur_block;
        let undef_box = ctx.f.append_inst(
            cur_block,
            InstKind::Call(
                ctx.intrinsics.any_box,
                vec![Operand::ConstI64(5), Operand::ConstI64(0)],
            ),
            Type::Any,
            None,
        );
        argv.push(Operand::Value(undef_box));
    }
}

fn apply_consume_bitmap(ctx: &mut LowerCtx<'_>, callee: ExprId, args: &[ExprId]) {
    // Per-call-site consume bitmap from `ast.consuming_params`.
    // Mirrors the check.rs pass. A consuming arg position transfers
    // ownership from the caller's binding to the callee — without
    // this, both the caller's local and the callee's slot would own
    // the same heap and both drop.
    let consume_bitmap: Vec<bool> = match ctx.ast.get_expr(callee) {
        Expr::Ident(callee_name) => {
            if let Some(bm) = ctx.ast.consuming_params.get(callee_name) {
                bm.clone()
            } else if callee_name.starts_with("__new_") {
                vec![true; args.len()]
            } else {
                vec![false; args.len()]
            }
        }
        _ => vec![false; args.len()],
    };
    for (i, a) in args.iter().enumerate() {
        if consume_bitmap.get(i).copied().unwrap_or(false) {
            ctx.consume_if_ident(*a);
        }
    }
}

fn coerce_args(ctx: &mut LowerCtx<'_>, target: FuncId, args: &[ExprId], argv: &mut [Operand]) {
    if ctx.is_math_unary(target) {
        debug_assert_eq!(argv.len(), 1, "Math.* unary takes 1 arg");
        argv[0] = coerce_to_f64_or_any_to_number(ctx, argv[0]);
        return;
    }
    if ctx.is_math_binary(target) {
        debug_assert_eq!(argv.len(), 2, "Math.* binary takes 2 args");
        for i in 0..2 {
            argv[i] = coerce_to_f64_or_any_to_number(ctx, argv[i]);
        }
        return;
    }
    let Some(sig_id) = ctx.fn_sig_ids.get(&target).copied() else {
        return;
    };
    // Width-aware coercion for monomorphized generic calls AND
    // direct intrinsics. Coerce both directions:
    //   expected F64 + actual I64 → SiToFp (widen)
    //   expected I64 + actual F64 → FpToSi (truncate; matches JS
    //     ToInt32 / ToUint32 prefix for indexes/codepoints/bit
    //     positions)
    // P0.9 — global FnDecl with Any param + concrete arg: box the
    // concrete value into Any at the call boundary. S126-3
    // `box_to_any_from_expr` reads `args[i]` expr_types so
    // undefined/null literals keep ANY_UNDEF/ANY_NULL tags.
    let param_tys = ctx.fn_sigs[sig_id.0 as usize].0.clone();
    for (i, expected) in param_tys.iter().enumerate() {
        if i >= argv.len() {
            break;
        }
        let got = ctx.operand_ty(&argv[i]);
        match (expected, got) {
            (Type::F64, Type::I64) | (Type::F64, Type::Bool) => {
                argv[i] = ctx.coerce_to_f64(argv[i]);
            }
            (Type::I64, Type::F64) | (Type::I64, Type::Bool) => {
                argv[i] = ctx.coerce_to_i64(argv[i]);
            }
            (Type::Any, got_ty) if got_ty != Type::Any => {
                argv[i] = ctx.box_to_any_from_expr(args[i], argv[i]);
            }
            _ => {}
        }
    }
}

fn coerce_to_f64_or_any_to_number(ctx: &mut LowerCtx<'_>, op: Operand) -> Operand {
    // S342 — Any-typed arg goes through any_to_number (ToNumber per
    // spec §21.3.2.*) so the F64 ABI sees a clean f64.
    // coerce_to_f64 wouldn't handle Any.
    if ctx.operand_ty(&op) == Type::Any {
        let cur_block = ctx.cur_block;
        let f = ctx.f.append_inst(
            cur_block,
            InstKind::Call(ctx.intrinsics.any_to_number, vec![op]),
            Type::F64,
            None,
        );
        Operand::Value(f)
    } else {
        ctx.coerce_to_f64(op)
    }
}
