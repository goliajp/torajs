//! `xs.{map,filter,reduce,reduceRight,forEach}(fn[, init?])` lowering —
//! first carve-out chunk pulled out of [`crate::ssa_lower::lower_expr_inner`]
//! `Expr::Call` arm along the structural-axis decomposition path the
//! `ssa_lower_str_str_dispatch` 5-chunk decomp validated. Splits with
//! companion module [`crate::ssa_lower_call_arr_ho_loop`] which owns the
//! loop frame + per-method body emit; this file owns the dispatch entry,
//! receiver / callable lower, dst/acc slot prep.
//!
//! The non-Arr receiver still `panic!`s exactly as the inline arm did —
//! Math.* / String.length / console.log are dispatched upstream by the
//! M6.1 / `<recv>.<method>` path so they never enter this arm; a non-Arr
//! receiver here is a hard type error.

use crate::ast::{Expr, ExprId};
use crate::ssa::{FuncId, Operand, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_call_arr_ho_loop::{begin_loop, emit_per_method_body, end_loop_and_produce};
use crate::ssa_lower_call_arr_ho_slots::{
    dst_array_type, prepare_acc_slot, prepare_dst_slot, resolve_acc_ty,
};

/// Try to lower `<recv>.{map|filter|reduce|reduceRight|forEach}(fn[, init?])`.
/// Returns `Some(result)` when the callee matches the Member-of-method
/// shape; `None` lets the caller fall through to the next dispatch arm.
pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let (obj, name) = match ctx.ast.get_expr(callee) {
        Expr::Member { obj, name } => (*obj, name.clone()),
        _ => return None,
    };
    if !matches!(
        name.as_str(),
        "map" | "filter" | "reduce" | "reduceRight" | "forEach"
    ) {
        return None;
    }
    Some(lower_higher_order(ctx, eid, obj, name, args))
}

fn lower_higher_order(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    obj: ExprId,
    method: String,
    args: &[ExprId],
) -> Operand {
    let recv_op = ctx.lower_expr(obj);
    let recv_ty = ctx.operand_ty(&recv_op);
    if !matches!(recv_ty, Type::Arr(_)) {
        panic!("ssa-lower: `.{method}(...)` on non-array receiver type {recv_ty:?}");
    }
    // Rotation 550 — an owned receiver is live across the callback /
    // init lowers and the loop's throw edges; park it.
    let recv_tok = ctx.park_owned_temp(obj, &recv_op);
    let arr_ty = recv_ty;
    let elem_ty = ctx.arr_layouts[match arr_ty {
        Type::Arr(id) => id.0 as usize,
        _ => unreachable!(),
    }];
    let src_arr = match recv_op {
        Operand::Value(v) => v,
        _ => unreachable!("Type::Arr can't be a constant operand"),
    };
    // §23.1.3.19/.8 step 3 ArraySpeciesCreate — map / filter build a
    // species product; reduce / reduceRight / forEach don't (RFC
    // 20260713 blade 3 constructor-face guard).
    if matches!(method.as_str(), "map" | "filter") {
        ctx.emit_arr_species_guard(Operand::Value(src_arr));
    }
    /* Devirt opportunity: if args[0]'s AST is a known
     * `Expr::Closure { fn_name, .. }` (capturing arrow lifted by
     * lift_arrow_fns) or `Expr::Ident(fn_name)` resolving to a top-level
     * FnDecl, the callable's underlying FuncId is statically known. We
     * can skip the env+8 / fn_ptr indirect dispatch and emit a direct
     * `Call(fid, [env_or_args])` per element. Devirt fires for both
     * shapes; non-capturing fns (`xs.map(add1)`) skip env.
     *
     * Big leverage: array-map-1m's `xs.map(closure)` goes from 10M
     * indirect `tail call %fn_ptr(env, x)` to 10M `tail call
     * @__closure_N(env, x)` — LLVM value-prop now sees a constant call
     * target and can inline the closure body when small enough. On
     * `(x: number) => x + k` (3-instr body) the full map loop folds to
     * a vectorized add. */
    let known_fid: Option<FuncId> = match ctx.ast.get_expr(args[0]) {
        Expr::Closure { fn_name, .. } => ctx.fn_table.get(fn_name).copied(),
        Expr::Ident(name) => ctx.fn_table.get(name).copied(),
        _ => None,
    };
    let fn_val = ctx.lower_expr(args[0]);
    let fn_ty = ctx.operand_ty(&fn_val);
    // Knife 4 (RFC 20260717-fnexpr-this-channel) — a promoted fn-expr
    // callback takes the §23.1.3 thisArg (T) as its leading `__this`
    // arg: args[1] lowers into an Any box (undefined when absent).
    // Only map / filter / forEach carry a thisArg — reduce's args[1]
    // is the initialValue.
    // A promoted callback is either the inline Closure itself or a
    // knife-2 variable-routed binding (`fnexpr_recv_locals`) — both
    // native ABIs carry the leading `__this`; missing the Ident
    // shape ran the loop with shifted args (rotation 260).
    let promoted = matches!(ctx.ast.get_expr(args[0]),
        Expr::Closure { fn_name, .. } if ctx.ast.fnexpr_recv_fns.contains(fn_name))
        || matches!(ctx.ast.get_expr(args[0]),
            Expr::Ident(n) if ctx.ast.fnexpr_recv_locals.contains(n));
    let promoted = promoted && matches!(method.as_str(), "map" | "filter" | "forEach");
    let mut this_temp: Option<(ExprId, Operand)> = None;
    let this_arg: Option<Operand> = if promoted {
        if let Some(&t) = args.get(1) {
            let op = ctx.lower_expr(t);
            // box_to_any is a pure encoding (chunk 753) — an
            // owned-shape thisArg temp (inline object literal) keeps
            // its single stake in `op`, and the box borrows it every
            // iteration. Releasing here freed the payload after the
            // first iteration (`this.mul` read garbage); the release
            // rides the after-loop settlement below instead.
            let boxed = ctx.box_to_any_from_expr(t, op.clone());
            this_temp = Some((t, op));
            Some(boxed)
        } else {
            Some(Operand::Value(
                crate::ssa_lower_call_arr_ho_loop::emit_undef_any_box(ctx),
            ))
        }
    } else {
        None
    };
    // S270 — eval-and-drop trailing args (thisArg + extras) so
    // side-effect expressions fire per ES §23.1.3.X trailing-arg ignore.
    // SSA-emit reads only args[0] (cb) for map/filter/forEach; for
    // reduce/reduceRight args[1] is initialValue and lowered later, so
    // skip it here. A promoted callback's thisArg lowered above.
    let skip_n = if matches!(method.as_str(), "reduce" | "reduceRight") || promoted {
        2
    } else {
        1
    };
    for &t in args.iter().skip(skip_n) {
        let _ = ctx.lower_expr(t);
    }
    let dst_arr_ty = dst_array_type(ctx, &method, fn_ty, arr_ty, eid);
    let dst_slot = prepare_dst_slot(ctx, &method, src_arr, dst_arr_ty);
    // Explicit reduce init lowers here (arg order preserved: cb, then
    // initialValue) so its static type can pick the acc lane below.
    let is_reduce_family = matches!(method.as_str(), "reduce" | "reduceRight");
    let reduce_init_op = if is_reduce_family && args.len() >= 2 {
        Some(ctx.lower_expr(args[1]))
    } else {
        None
    };
    let acc_ty = resolve_acc_ty(ctx, fn_ty, elem_ty, is_reduce_family, &reduce_init_op);
    let reduce_no_init = matches!(method.as_str(), "reduce" | "reduceRight") && args.len() == 1;
    let acc_slot = prepare_acc_slot(
        ctx,
        &method,
        args,
        src_arr,
        elem_ty,
        acc_ty,
        reduce_no_init,
        reduce_init_op,
    );
    // Rotation 363 — an argv-face callback (the inline fn-expr
    // whose body reads `arguments` values; collector doc at
    // `arguments_object_collect::collect_hof_anon_argv`) must not
    // take the direct / devirt call: its reshaped sig leads with the
    // synthetic argv pointer. The loop body routes it through the
    // boxed variadic dispatch instead (emit_argv_face_call).
    let argv_face = matches!(ctx.ast.get_expr(args[0]),
        Expr::Closure { fn_name, .. } if ctx.ast.closure_argv_fns.contains(fn_name))
        || matches!(ctx.ast.get_expr(args[0]),
            Expr::Ident(n) if ctx.ast.closure_argv_locals.contains(n));
    let frame = begin_loop(
        ctx,
        &method,
        src_arr,
        dst_slot,
        dst_arr_ty,
        elem_ty,
        reduce_no_init,
    );
    emit_per_method_body(
        ctx,
        frame,
        &method,
        src_arr,
        dst_slot,
        acc_slot,
        dst_arr_ty,
        acc_ty,
        elem_ty,
        known_fid,
        &fn_val,
        fn_ty,
        this_arg.as_ref(),
        argv_face,
    );
    let out = end_loop_and_produce(ctx, frame, &method, dst_slot, acc_slot, dst_arr_ty, acc_ty);
    // RFC 20260705 chunk 550 — release owned-shape temps after the
    // loop consumed them: an inline arrow's minted env in the cb
    // slot and a Call/New-shaped receiver.
    ctx.release_owned_temp(args[0], &fn_val);
    ctx.unpark_owned_temp(recv_tok);
    ctx.release_owned_temp(obj, &recv_op);
    if let Some((t, op)) = this_temp {
        ctx.release_owned_temp(t, &op);
    }
    out
}
