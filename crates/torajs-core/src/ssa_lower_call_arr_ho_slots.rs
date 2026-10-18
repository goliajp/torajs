//! dst / acc slot preparation for the array higher-order lanes
//! (`map` / `filter` / `reduce` / `reduceRight` / `forEach`) — the
//! "slot prep" half of [`crate::ssa_lower_call_arr_ho`], moved out as
//! its own sibling (rotation 550, pure motion): that file keeps the
//! dispatch entry and the receiver / callable lower, this one answers
//! what the loop writes into (the pre-sized dst array for map / filter,
//! the typed accumulator slot for the reduce family) and the dst
//! element type the callback's return shape dictates.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type, ValueId};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx, intern_arr_layout};

/// `map`/`filter`: pre-size dst to src.len so per-iter
/// `arr_push_unchecked` stays within cap (S135 — bare `arr_alloc(0)`
/// stomped past pool blocks on multi-elem inputs, surfaced as SIGSEGV in
/// chained `xs.filter(p).map(f)` where map's loop read filter dst's
/// garbage len).
pub(crate) fn prepare_dst_slot(
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
pub(crate) fn resolve_acc_ty(
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

/// The explicit reduce init. An empty array literal (under any `as`)
/// is contextually typed by the accumulator the callback declares —
/// TS infers the reducer's `U` from it — so it is minted in that
/// array lane, as under a `const seed: T[] = []` annotation. Minted
/// as `Arr<Any>` it differed from the callback's return and pushed
/// the accumulator into the boxed lane, whose box the typed callback
/// then read as its own array layout. The lane is taken only when the
/// accumulator parameter (after a promoted reducer's leading `__this`)
/// is that same array type: an `any[]` parameter handed a typed array
/// would find its element kind locked.
pub(crate) fn lower_reduce_init(
    ctx: &mut LowerCtx<'_>,
    init: ExprId,
    fn_ty: Type,
    promoted: bool,
) -> Operand {
    let mut lit = init;
    while let Expr::As { expr, .. } = ctx.ast.get_expr(lit) {
        lit = *expr;
    }
    if let Expr::Array(els) = ctx.ast.get_expr(lit)
        && els.is_empty()
        && let Some(acc @ Type::Arr(id)) = ctx.callback_ret_ty(fn_ty)
        && ctx.arr_layouts[id.0 as usize] != Type::Any
        && ctx
            .sig_param_tys(fn_ty)
            .and_then(|p| p.get(usize::from(promoted)).copied())
            == Some(acc)
    {
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.arr_alloc, vec![Operand::ConstI64(0)]),
            acc,
            None,
        );
        return Operand::Value(v);
    }
    ctx.lower_expr(init)
}

/// `reduce`/`reduceRight`: allocate `__iter_acc`, seed from `init_op`
/// (2-arg form — lowered by the caller so the acc lane could read its
/// type) or the first present element in visiting order (1-arg form,
/// TypeError for an array with none — §23.1.3.24/25 step 4 and 8).
#[allow(clippy::too_many_arguments)]
pub(crate) fn prepare_acc_slot(
    ctx: &mut LowerCtx<'_>,
    method: &str,
    args: &[ExprId],
    src_arr: ValueId,
    elem_ty: Type,
    acc_ty: Type,
    reduce_no_init: bool,
    init_op: Option<Operand>,
) -> (Option<ValueId>, Option<ValueId>) {
    if !matches!(method, "reduce" | "reduceRight") {
        return (None, None);
    }
    // No initial value: the seed is the first PRESENT element — a
    // typed source can hold holes too — found by the runtime, which
    // also raises the TypeError for an array with none (see
    // `emit_present_seed`).
    let mut seed_start = None;
    let init_v = if reduce_no_init {
        let (k, v) = emit_present_seed(ctx, method, src_arr, elem_ty);
        seed_start = Some(k);
        v
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
        // The typed slot owns its accumulator the same way: a borrowed
        // explicit init (`xs.reduce(cb, seed)`) takes a +1 so the
        // binding keeps its own stake past the first step's old-acc
        // drop, and an owned temp init releases the ref that +1
        // duplicated.
        _ if !reduce_no_init && init_ty.is_refcounted() => {
            ctx.emit_rc_inc(init_v);
            ctx.release_owned_temp(args[1], &init_v);
            init_v
        }
        _ => init_v,
    };
    let slot = ctx.alloca(acc_ty, Some("__iter_acc"));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(init_v, Operand::Value(slot), 0),
    );
    (Some(slot), seed_start)
}

/// §23.1.3.24 / §23.1.3.25 step 8: the runtime answers the first
/// present index in visiting order (or records the TypeError for an
/// array with none). Over a boxed-element source the seed is that
/// element's [[Get]] — owned, since a getter may have produced it; over
/// a typed source it is the slot, and a refcounted one is bumped so the
/// post-loop accumulator drop balances. The loop starts one step past
/// it (`begin_loop`'s `seed_start`).
fn emit_present_seed(
    ctx: &mut LowerCtx<'_>,
    method: &str,
    src_arr: ValueId,
    elem_ty: Type,
) -> (ValueId, Operand) {
    let right = (method == "reduceRight") as i64;
    let k = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.arr_reduce_seed_index,
            vec![Operand::Value(src_arr), Operand::ConstI64(right)],
        ),
        Type::I64,
        None,
    );
    ctx.emit_throw_check(None);
    if elem_ty != Type::Any {
        let (off_base, off) =
            ctx.emit_arr_slot_byte_offset(Operand::Value(src_arr), Operand::Value(k), 8, false);
        let seed = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::LoadDyn(elem_ty, off_base, off),
            elem_ty,
            None,
        );
        if elem_ty.is_refcounted() {
            ctx.emit_rc_inc(Operand::Value(seed));
        }
        return (k, Operand::Value(seed));
    }
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.arr_index_get,
            vec![Operand::Value(src_arr), Operand::Value(k)],
        ),
        Type::Any,
        None,
    );
    ctx.emit_throw_check_owned(None, Operand::Value(v), Type::Any);
    (k, Operand::Value(v))
}

/// The product array type of `map` / `filter`: map takes the closure's
/// return (void → any; i64 widened by the width analysis; a view answer
/// materialized to Str), filter copies the receiver (a view-typed
/// receiver copied out as owned strings — rotation 468). Carved out of
/// [`lower_higher_order`] when the filter arm pushed it past the
/// 200-line function limit.
pub(crate) fn dst_array_type(
    ctx: &mut LowerCtx<'_>,
    method: &str,
    fn_ty: Type,
    arr_ty: Type,
    eid: ExprId,
) -> Type {
    if method == "map"
        && let Some(sig_id) = match fn_ty {
            Type::FnSig(s) | Type::Closure(s) => Some(s),
            _ => None,
        }
    {
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
        } else if ret == Type::Substr {
            // A view answer (`xs.map(x => x)` over a split product) is
            // pushed as an owned copy — a view does not leave its split
            // block (rotation 468); `emit_map` materializes.
            Type::Str
        } else {
            ret
        };
        let arr_id = intern_arr_layout(ctx.arr_layouts, ret);
        Type::Arr(arr_id)
    } else if method == "filter"
        && let Type::Arr(src_id) = arr_ty
    {
        // filter copies kept elements out of the receiver: out of an
        // `Arr<Substr>` they are owned copies (rotation 468).
        Type::Arr(ctx.copied_arr_layout(src_id))
    } else {
        arr_ty
    }
}
