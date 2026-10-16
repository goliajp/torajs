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

pub(crate) mod cb_face;
mod dense_gate;
mod reduce_cast;

pub(crate) use dense_gate::typed_dense_gate;

use crate::ast::{Expr, ExprId};
use crate::ssa::{FuncId, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_arr_hole_gate::TypedGate;
use crate::ssa_lower_call_arr_ho_loop::{begin_loop, emit_per_method_body, end_loop_and_produce};
use crate::ssa_lower_call_arr_ho_slots::{
    dst_array_type, lower_reduce_init, prepare_acc_slot, prepare_dst_slot, resolve_acc_ty,
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
        Expr::Ident(name) if !ctx.locals.contains_key(name) => ctx.fn_table.get(name).copied(),
        _ => None,
    };
    let fn_val = ctx.lower_expr(args[0]);
    // 550-01 — a minted callback env (inline arrow) is ours until the
    // post-loop release; a callback that throws mid-loop drops it.
    let fn_tok = ctx.park_owned_temp(args[0], &fn_val);
    // Rotation 363 — an argv-face callback (one reading `arguments`
    // values, or typed slots fed `any` elements: `cb_face`) takes the
    // boxed variadic dispatch (emit_argv_face_call), never a direct call.
    let fn_ty = ctx.operand_ty(&fn_val);
    let (fn_ty, argv_face) =
        crate::ssa_lower_call_arr_ho::cb_face::cb_face(ctx, args[0], fn_ty, arr_ty, &method, eid);
    // Knife 4 (RFC 20260717-fnexpr-this-channel) — a promoted fn-expr
    // callback takes the §23.1.3 thisArg (T) as its leading `__this`
    // arg: args[1] lowers into an Any box (undefined when absent).
    // Only map / filter / forEach carry a thisArg — reduce's args[1]
    // is the initialValue.
    let is_reduce = matches!(method.as_str(), "reduce" | "reduceRight");
    let promoted = promoted_callback(ctx, args[0], &method);
    let mut this_temp: Option<(ExprId, Operand)> = None;
    let mut this_tok: Option<usize> = None;
    let this_arg: Option<Operand> = if promoted {
        if !is_reduce && let Some(&t) = args.get(1) {
            let op = ctx.lower_expr(t);
            // box_to_any is a pure encoding (chunk 753) — an
            // owned-shape thisArg temp (inline object literal) keeps
            // its single stake in `op`, and the box borrows it every
            // iteration. Releasing here freed the payload after the
            // first iteration (`this.mul` read garbage); the release
            // rides the after-loop settlement below instead.
            let boxed = ctx.box_to_any_from_expr(t, op);
            this_tok = ctx.park_owned_temp(t, &op);
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
        Some(lower_reduce_init(ctx, args[1], fn_ty, promoted))
    } else {
        None
    };
    let acc_ty = resolve_acc_ty(ctx, fn_ty, elem_ty, is_reduce_family, &reduce_init_op);
    let reduce_no_init = matches!(method.as_str(), "reduce" | "reduceRight") && args.len() == 1;
    let (acc_slot, seed_start) = prepare_acc_slot(
        ctx,
        &method,
        args,
        src_arr,
        elem_ty,
        acc_ty,
        reduce_no_init,
        reduce_init_op,
    );
    // 550-01 — the running accumulator (reduce family) is ours until
    // the exit loads it out (seed inc'd / init transferred / each step
    // stores the new value before dropping the old); a callback that
    // throws mid-loop releases it through the slot. Copy-typed
    // accumulators answer no token. Each loop copy parks the dst itself
    // (`emit_hof_loop`), once its pre-reserve shape is known.
    let acc_tok = acc_slot.and_then(|s| ctx.push_throw_typed_slot(s, acc_ty));
    let plan = HofLoop {
        method: &method,
        src_arr,
        dst_slot,
        acc_slot,
        dst_arr_ty,
        acc_ty,
        elem_ty,
        seed_start,
        known_fid,
        fn_val: &fn_val,
        fn_ty,
        this_arg: this_arg.as_ref(),
        argv_face,
        dense_gate: typed_dense_gate(ctx, elem_ty, obj, args[0], if is_reduce { 3 } else { 2 }),
    };
    let out = if elem_ty == Type::Any {
        emit_hof_loop(ctx, &plan, false)
    } else {
        emit_versioned(ctx, &plan)
    };
    ctx.unpark_owned_temp(acc_tok);
    // RFC 20260705 chunk 550 — release owned-shape temps after the
    // loop consumed them: an inline arrow's minted env in the cb
    // slot and a Call/New-shaped receiver.
    ctx.unpark_owned_temp(fn_tok);
    ctx.release_owned_temp(args[0], &fn_val);
    ctx.unpark_owned_temp(recv_tok);
    ctx.release_owned_temp(obj, &recv_op);
    ctx.unpark_owned_temp(this_tok);
    if let Some((t, op)) = this_temp {
        ctx.release_owned_temp(t, &op);
    }
    if is_reduce && args.len() >= 2 {
        return reduce_cast::cast_reduce_product(ctx, eid, args[0], fn_ty, promoted, out);
    }
    out
}

/// Everything one emission of the loop reads — the typed-source path
/// emits it twice ([`emit_versioned`]).
struct HofLoop<'o> {
    method: &'o str,
    src_arr: ValueId,
    dst_slot: Option<ValueId>,
    acc_slot: Option<ValueId>,
    dst_arr_ty: Type,
    acc_ty: Type,
    elem_ty: Type,
    seed_start: Option<ValueId>,
    known_fid: Option<FuncId>,
    fn_val: &'o Operand,
    fn_ty: Type,
    this_arg: Option<&'o Operand>,
    argv_face: bool,
    /// The gate the dense copy runs: `Len` for a typed source the
    /// callback may shorten, `Present` for one it may punch a hole
    /// into (`hof_len_shrink`).
    dense_gate: TypedGate,
}

/// One loop over the source: header, visit, step, product. `holes`
/// picks the copy whose every step asks whether the index is present
/// (a typed source whose header says it can have holes).
fn emit_hof_loop(ctx: &mut LowerCtx<'_>, p: &HofLoop<'_>, holes: bool) -> Operand {
    let mut frame = begin_loop(
        ctx,
        p.method,
        p.src_arr,
        p.dst_slot,
        p.dst_arr_ty,
        p.elem_ty,
        p.seed_start,
    );
    frame.gate = if holes {
        TypedGate::Present
    } else {
        p.dense_gate
    };
    frame.holey = holes;
    // 550-01 — the growing dst (map / filter) is ours until the exit
    // loads it out; a callback that throws mid-loop releases it
    // through the slot (push relocates, so the pointer itself cannot
    // be parked). A deferred-length pre-reserve hands its running
    // count along so the throw path settles the cell first. Each loop
    // copy has its own count, so each parks its own.
    let dst_tok = p.dst_slot.and_then(|s| {
        let deferred = frame
            .dst_fast
            .filter(|st| st.defer_len)
            .map(|st| st.len_slot);
        ctx.push_throw_arr_slot(s, p.dst_arr_ty, deferred)
    });
    emit_per_method_body(
        ctx,
        frame,
        p.method,
        p.src_arr,
        p.acc_slot,
        p.dst_arr_ty,
        p.acc_ty,
        p.elem_ty,
        p.known_fid,
        p.fn_val,
        p.fn_ty,
        p.this_arg,
        p.argv_face,
    );
    let out = end_loop_and_produce(
        ctx,
        frame,
        p.method,
        p.dst_slot,
        p.acc_slot,
        p.dst_arr_ty,
        p.acc_ty,
    );
    ctx.unpark_owned_temp(dst_tok);
    out
}

/// A typed source: read the header once, before either loop, and run
/// today's loop over a dense array — the per-step HasProperty test is
/// what `ssa_lower_arr_hole_gate`'s measurement priced at +34% inside a
/// dense loop — or the gated copy over one that can have holes. Both
/// copies write the same dst / acc slots; the product is taken after
/// they meet.
fn emit_versioned(ctx: &mut LowerCtx<'_>, p: &HofLoop<'_>) -> Operand {
    let out_ty = match p.method {
        "map" | "filter" => Some(p.dst_arr_ty),
        "reduce" | "reduceRight" => Some(p.acc_ty),
        _ => None,
    };
    let out_slot = out_ty.map(|t| ctx.alloca(t, Some("__hof_out")));
    let holey = ctx.emit_hof_sparse_probe(p.src_arr);
    let dense_blk = ctx.f.add_block();
    let holey_blk = ctx.f.add_block();
    let join_blk = ctx.f.add_block();
    let cur = ctx.cur_block;
    ctx.f.set_term(
        cur,
        Terminator::CondBr {
            cond: Operand::Value(holey),
            then_blk: holey_blk,
            else_blk: dense_blk,
        },
    );
    for (blk, holes) in [(dense_blk, false), (holey_blk, true)] {
        ctx.cur_block = blk;
        let out = emit_hof_loop(ctx, p, holes);
        if let Some(slot) = out_slot {
            ctx.f
                .append_void(ctx.cur_block, InstKind::Store(out, Operand::Value(slot), 0));
        }
        let end = ctx.cur_block;
        ctx.f.set_term(end, Terminator::Br(join_blk));
    }
    ctx.cur_block = join_blk;
    match (out_slot, out_ty) {
        (Some(slot), Some(ty)) => Operand::Value(ctx.f.append_inst(
            join_blk,
            InstKind::Load(ty, Operand::Value(slot), 0),
            ty,
            None,
        )),
        _ => Operand::ConstI64(0),
    }
}

/// A promoted callback is either the inline Closure itself or a
/// knife-2 variable-routed binding (`fnexpr_recv_locals`) — both
/// native ABIs carry the leading `__this`; missing the Ident shape
/// ran the loop with shifted args (rotation 260). Only the methods
/// whose loop threads a receiver count: map / filter / forEach take
/// the §23.1.3 thisArg, and reduce / reduceRight (RFC 20260905
/// residue 1) take undefined — §23.1.3.24 calls the reducer with no
/// receiver, but a promoted reducer still owns the leading slot, and
/// leaving it out fed the accumulator into it.
fn promoted_callback(ctx: &LowerCtx<'_>, cb: ExprId, method: &str) -> bool {
    let promoted = matches!(ctx.ast.get_expr(cb),
        Expr::Closure { fn_name, .. } if ctx.ast.fnexpr_recv_fns.contains(fn_name))
        || ctx.ast.is_fnexpr_recv_use(cb);
    promoted
        && matches!(
            method,
            "map" | "filter" | "forEach" | "reduce" | "reduceRight"
        )
}
