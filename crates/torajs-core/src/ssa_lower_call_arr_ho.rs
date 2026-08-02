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
use crate::ssa::{BinOp as SsaBinOp, FuncId, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx, intern_arr_layout};
use crate::ssa_lower_call_arr_ho_loop::{begin_loop, emit_per_method_body, end_loop_and_produce};

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
    // dst array element type: filter = src; map = closure ret
    // (Arr<Substr>→Arr<Str> on materialize boundary).
    let dst_arr_ty = if method == "map"
        && let Some(sig_id) = match fn_ty {
            Type::FnSig(s) | Type::Closure(s) => Some(s),
            _ => None,
        } {
        let ret = ctx.fn_sigs[sig_id.0 as usize].1;
        // RC-1 (RFC 20260706-test262-bug-corpus) — a Void-ret callback
        // maps every element to `undefined`; the dst holds Any boxes.
        let ret = if ret == Type::Void { Type::Any } else { ret };
        // RFC 20260726-array-elem-width knife 1 — the callback's ret is
        // only ONE source of the product's elements, so it cannot decide
        // their width alone. The analysis keys this product by its call
        // origin and joins that class with every slot the product flows
        // into (`container_result_key.rs`, the "map" arm); a fractional
        // value reaching any of them widens the class while the ret edge
        // stays narrow — that edge is directional on purpose (reduce's
        // accumulator rides it, see acc_ty below). Ask the class, the way
        // an array literal already does (`ssa_lower_array::compute_elem_ty`).
        // Skipping the ask stored I64 bits behind an F64-typed slot and
        // every later read reinterpreted them — silently, exit 0.
        let ret = if ret == Type::I64
            && ctx
                .num_f64_slots
                .elem_is_f64(&crate::num_width::SlotKey::Anon(eid.0))
        {
            Type::F64
        } else {
            ret
        };
        let arr_id = intern_arr_layout(ctx.arr_layouts, ret);
        Type::Arr(arr_id)
    } else {
        arr_ty
    };
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
    let frame = begin_loop(ctx, &method, src_arr, dst_slot, dst_arr_ty, reduce_no_init);
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
    );
    let out = end_loop_and_produce(ctx, frame, &method, dst_slot, acc_slot, dst_arr_ty, acc_ty);
    // RFC 20260705 chunk 550 — release owned-shape temps after the
    // loop consumed them: an inline arrow's minted env in the cb
    // slot and a Call/New-shaped receiver.
    ctx.release_owned_temp(args[0], &fn_val);
    ctx.release_owned_temp(obj, &recv_op);
    if let Some((t, op)) = this_temp {
        ctx.release_owned_temp(t, &op);
    }
    out
}

/// `map`/`filter`: pre-size dst to src.len so per-iter
/// `arr_push_unchecked` stays within cap (S135 — bare `arr_alloc(0)`
/// stomped past pool blocks on multi-elem inputs, surfaced as SIGSEGV in
/// chained `xs.filter(p).map(f)` where map's loop read filter dst's
/// garbage len).
fn prepare_dst_slot(
    ctx: &mut LowerCtx<'_>,
    method: &str,
    src_arr: ValueId,
    dst_arr_ty: Type,
) -> Option<ValueId> {
    if !matches!(method, "map" | "filter") {
        return None;
    }
    let src_len_for_cap = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(src_arr), ARR_LEN_OFF),
        Type::I64,
        None,
    );
    // Any-elem dst allocates through arr_alloc_any so the block
    // carries FLAG_ARR_ANY — the plain alloc leaves the flag clear
    // and runtime flag-dispatch walkers (cycle collector,
    // value_drop_heap) go blind on the product (the lowering-side
    // static drop still walked it via arr_drop_any, so this was a
    // walker-visibility hole, not a leak).
    let dst_is_any = matches!(dst_arr_ty, Type::Arr(id)
        if matches!(ctx.arr_layouts[id.0 as usize], Type::Any));
    let alloc_fid = if dst_is_any {
        ctx.intrinsics.arr_alloc_any
    } else {
        ctx.intrinsics.arr_alloc
    };
    let dst_arr = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(alloc_fid, vec![Operand::Value(src_len_for_cap)]),
        dst_arr_ty,
        None,
    );
    let slot = ctx.alloca(dst_arr_ty, Some("__iter_dst"));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(dst_arr), Operand::Value(slot), 0),
    );
    Some(slot)
}

/// W4 — reduce acc width follows callback ret (acc is fed back from
/// it every iter), not receiver's elem: i64 elems + f64-widened
/// callback left the acc slot narrow → GPR/FPR mismatch (array-007).
/// RC-1 — a Void-ret callback feeds back the boxed `undefined`, so
/// the slot holds an Any.
///
/// rotation 285 — a HETERO reduce seed (explicit init or the no-init
/// elem seed whose type differs from the cb ret) forces the Any acc
/// lane: §23.1.3.24's acc is `init` before the first call and the cb
/// ret after, so a typed slot can hold neither union member without
/// silent bit reinterpretation (an empty array + typed slot answered
/// the seed's raw bits under the ret's type). The F64←I64 widening
/// keeps its coerce lane (array-007), and an Any ret is already the
/// boxed lane.
fn resolve_acc_ty(
    ctx: &LowerCtx<'_>,
    fn_ty: Type,
    elem_ty: Type,
    is_reduce_family: bool,
    reduce_init_op: &Option<Operand>,
) -> Type {
    let acc_ty = match fn_ty {
        Type::FnSig(s) | Type::Closure(s) if ctx.fn_sigs[s.0 as usize].1 == Type::Void => Type::Any,
        Type::FnSig(s) | Type::Closure(s) => ctx.fn_sigs[s.0 as usize].1,
        _ => elem_ty,
    };
    if is_reduce_family && acc_ty != Type::Any {
        let seed_ty = reduce_init_op
            .as_ref()
            .map(|op| ctx.operand_ty(op))
            .unwrap_or(elem_ty);
        if seed_ty != acc_ty && !(acc_ty == Type::F64 && seed_ty == Type::I64) {
            return Type::Any;
        }
    }
    acc_ty
}

/// `reduce`/`reduceRight`: allocate `__iter_acc`, seed from `init_op`
/// (2-arg form — lowered by the caller so the acc lane could read its
/// type) or arr[0] / arr[len-1] (1-arg form, throws on empty arr per
/// spec §22.1.3.21/22 step 3).
#[allow(clippy::too_many_arguments)]
fn prepare_acc_slot(
    ctx: &mut LowerCtx<'_>,
    method: &str,
    args: &[ExprId],
    src_arr: ValueId,
    elem_ty: Type,
    acc_ty: Type,
    reduce_no_init: bool,
    init_op: Option<Operand>,
) -> Option<ValueId> {
    if !matches!(method, "reduce" | "reduceRight") {
        return None;
    }
    if reduce_no_init {
        // Empty-arr guard: branch to throw_blk which calls the helper +
        // emit_throw_check; fall-through Br'd to continue_blk so the IR
        // validates (helper always sets throw_active → fall unreachable
        // at runtime).
        let len_chk = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(Type::I64, Operand::Value(src_arr), ARR_LEN_OFF),
            Type::I64,
            None,
        );
        let is_empty = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::ICmp(IPred::Eq, Operand::Value(len_chk), Operand::ConstI64(0)),
            Type::Bool,
            None,
        );
        let throw_blk = ctx.f.add_block();
        let continue_blk = ctx.f.add_block();
        let cb = ctx.cur_block;
        ctx.f.set_term(
            cb,
            Terminator::CondBr {
                cond: Operand::Value(is_empty),
                then_blk: throw_blk,
                else_blk: continue_blk,
            },
        );
        ctx.cur_block = throw_blk;
        let throw_fid = if method == "reduceRight" {
            ctx.intrinsics.arr_throw_reduce_right_empty
        } else {
            ctx.intrinsics.arr_throw_reduce_empty
        };
        ctx.f
            .append_void(ctx.cur_block, InstKind::Call(throw_fid, vec![]));
        ctx.emit_throw_check(None);
        let cb2 = ctx.cur_block;
        ctx.f.set_term(cb2, Terminator::Br(continue_blk));
        ctx.cur_block = continue_blk;
    }
    let init_v = if reduce_no_init {
        let len_for_seed = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(Type::I64, Operand::Value(src_arr), ARR_LEN_OFF),
            Type::I64,
            None,
        );
        let seed_idx = if method == "reduceRight" {
            ctx.f.append_inst(
                ctx.cur_block,
                InstKind::BinOp(
                    SsaBinOp::Sub,
                    Operand::Value(len_for_seed),
                    Operand::ConstI64(1),
                ),
                Type::I64,
                None,
            )
        } else {
            ctx.f.append_inst(
                ctx.cur_block,
                InstKind::BinOp(SsaBinOp::Add, Operand::ConstI64(0), Operand::ConstI64(0)),
                Type::I64,
                None,
            )
        };
        let (off_base, off) = ctx.emit_arr_slot_byte_offset(
            Operand::Value(src_arr),
            Operand::Value(seed_idx),
            3,
            false,
        );
        let seed = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::LoadDyn(elem_ty, off_base.clone(), off),
            elem_ty,
            None,
        );
        // Refcounted seed is a borrowed slot read — bump RC so the
        // post-loop acc drop balances.
        if elem_ty.is_refcounted() {
            ctx.emit_rc_inc(Operand::Value(seed));
        }
        Operand::Value(seed)
    } else {
        init_op.expect("reduce 2-arg form lowered its init at the call site")
    };
    let init_ty = ctx.operand_ty(&init_v);
    let init_v = match (acc_ty, init_ty) {
        (Type::F64, Type::I64) => ctx.coerce_to_f64(init_v),
        // W-J — `Array<Any>.reduce` with an Any-returning callback gets
        // acc_ty = Type::Any, but the user's literal init lowers raw bits.
        // Pre-fix wrote raw 100 into the 8-byte AnyValue slot → next
        // `Load any, slot` decoded garbage NaN-box → SIGSEGV.
        //
        // Rotation 185 stake audit — a refcounted EXPLICIT init
        // (`arr.reduce(cb, someObjVar)`) boxes rc-neutral into the
        // owned acc slot (post-loop drop / returned), so a borrow
        // source needs the same compensating inc the seed branch
        // already emits; an owned temp init then releases its own
        // ref (chunk-733 inc + release idiom — net transfer). The
        // seed branch inc'd before this match, so it must not take
        // the extra inc again.
        (Type::Any, src) if src != Type::Any => {
            if !reduce_no_init && init_ty.is_refcounted() {
                ctx.emit_rc_inc(init_v);
                let boxed = ctx.box_to_any(init_v);
                ctx.release_owned_temp(args[1], &init_v);
                boxed
            } else {
                ctx.box_to_any(init_v)
            }
        }
        _ => init_v,
    };
    let slot = ctx.alloca(acc_ty, Some("__iter_acc"));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(init_v, Operand::Value(slot), 0),
    );
    Some(slot)
}
