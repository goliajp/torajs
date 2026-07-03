//! Array-receiver `<Arr>.sort | toSorted (cmp?)` dispatch — sixth
//! sub-split carved out of [`ssa_lower_str::try_lower_method_call`].
//! Lowers in-place insertion-sort (`sort`) and clone-then-sort
//! (`toSorted`) over typed elements. The body emits the comparator
//! inline: when a user cmp is supplied it's invoked through the
//! callee Operand and the return widened to a Bool predicate;
//! without one, an element-type-aware `prev > cur` pred is built
//! directly (ICmp/FCmp for primitive layouts, the runtime helper
//! `__torajs_str_locale_compare` for Str/Substr, branchless XOR for
//! Bool). The pred drives the inner-shift loop that walks the
//! growing prefix backward to find the insertion point.
//!
//! Split: [`helper`] holds the `__torajs_arr_sort_cb` O(n log n)
//! fast-path gate + emission; [`pred`] holds the inline predicate
//! (user-cmp call / default ToString compare) emission.
//!
//! Returns `None` when the receiver is not `Type::Arr` or the
//! method is neither `sort` nor `toSorted` so the caller can keep
//! trying the remaining branches.

use crate::ast::ExprId;
use crate::ssa::{BinOp as SsaBinOp, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx};

mod helper;
mod pred;

/// Try to lower `<Arr>.sort | toSorted(cmp?)`. Returns
/// `Some(value)` when handled; `None` otherwise.
pub(crate) fn try_dispatch(
    ctx: &mut LowerCtx<'_>,
    method: &str,
    args: &[ExprId],
    recv_op: Operand,
    recv_ty: Type,
) -> Option<Operand> {
    // `arr.sort(cmp)` — in-place insertion sort calling
    // `cmp` for each compare. Returns the same array. The
    // comparator's return is treated as an i64 (or
    // implicitly-promoted-to-i64); ssa-lower picks ICmp/
    // FCmp(>0) based on its actual SSA type. Insertion
    // sort is O(n²) but works for moderate array sizes
    // and avoids needing closure-aware C runtime.
    if let Type::Arr(arr_id) = recv_ty
        && (method == "sort" || method == "toSorted")
    {
        // S276 — widen from `args.len() <= 1` to any arg count per ES
        // §23.1.3.{30,33} trailing-arg ignore. SSA-emit reads only
        // args[0] (cmp); trailing args eval-and-drop below so side-
        // effect exprs fire.
        let elem_ty = ctx.arr_layouts[arr_id.0 as usize];
        // toSorted clones the receiver via arr_slice
        // before sorting so the source stays intact.
        // arr_slice does the alloc + memcpy in one
        // runtime call; the rest of the body operates on
        // the clone.
        let recv_op = if method == "toSorted" {
            let len = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(Type::I64, recv_op, ARR_LEN_OFF),
                Type::I64,
                None,
            );
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.arr_slice,
                    vec![recv_op, Operand::ConstI64(0), Operand::Value(len)],
                ),
                Type::Arr(arr_id),
                None,
            );
            Operand::Value(v)
        } else {
            recv_op
        };
        let arr_ptr = match recv_op {
            Operand::Value(v) => v,
            _ => unreachable!(),
        };
        // V3-18 wedge — sort/toSorted with no comparator
        // emits inline element-type-aware `prev > cur`
        // instead of calling a user fn. cmp_val + cmp_ty
        // stay None for the no-arg path so the
        // pred-computation block can branch later.
        //
        // ES §23.1.3.{29,31} step 1 also accepts `undefined` for
        // the comparator literal; route a 1-arg call with arg type
        // Undefined to the same default-compare path (check.rs
        // mirror at the sort/toSorted arity special-case).
        let (cmp_val, cmp_ty) = if let Some(arg0) = args.first() {
            let arg_static_ty = ctx.expr_types.get(arg0).cloned();
            if matches!(arg_static_ty, Some(crate::check::Type::Undefined)) {
                // Drop the operand without lowering its side
                // effects — `undefined` is a literal keyword with
                // none.
                (None, None)
            } else {
                let v = ctx.lower_expr(*arg0);
                let t = ctx.operand_ty(&v);
                (Some(v), Some(t))
            }
        } else {
            (None, None)
        };
        // S276 — eval-and-drop trailing args past cmp so step()-style
        // side-effect exprs fire per ES §23.1.3.{30,33} trailing-arg
        // ignore. SSA-emit reads only args[0]; args[1..] discarded.
        for &a in args.iter().skip(1) {
            let _ = ctx.lower_expr(a);
        }
        // Perf Round 5 attack #1 (RFC 20260703-perf-arr-sort-nlogn):
        // a user comparator whose signature matches the element layout
        // exactly routes through `__torajs_arr_sort_cb` — a stable
        // O(n log n) merge sort in torajs-arr that calls the comparator
        // back through the closure ABI. The inline insertion sort below
        // is O(n²) (28× the comparison count at n=1000, measured vs
        // JSC) and stays only as the fallback for shapes the helper
        // doesn't cover (default no-comparator ToString compare,
        // Substr→Str materialization, non-{I64,F64,Str} elements).
        if helper::try_emit_sort_helper(ctx, &cmp_val, &cmp_ty, elem_ty, arr_ptr) {
            return Some(Operand::Value(arr_ptr));
        }
        emit_insertion_sort(ctx, arr_ptr, elem_ty, &cmp_val, &cmp_ty);
        return Some(Operand::Value(arr_ptr));
    }
    None
}

/// Emit the inline O(n²) insertion-sort fallback over `arr_ptr` in
/// place: outer loop grows the sorted prefix, inner-shift loop walks
/// it backward moving elements until the [`pred::emit_sort_pred`]
/// predicate releases. Leaves `ctx.cur_block` at the loop exit.
fn emit_insertion_sort(
    ctx: &mut LowerCtx<'_>,
    arr_ptr: ValueId,
    elem_ty: Type,
    cmp_val: &Option<Operand>,
    cmp_ty: &Option<Type>,
) {
    let len = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(arr_ptr), ARR_LEN_OFF),
        Type::I64,
        None,
    );
    let i_slot = ctx.alloca(Type::I64, Some("__sort_i"));
    // i = 1
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::ConstI64(1), Operand::Value(i_slot), 0),
    );
    let outer_hdr = ctx.f.add_block();
    let outer_body = ctx.f.add_block();
    let outer_after = ctx.f.add_block();
    ctx.f.set_term(ctx.cur_block, Terminator::Br(outer_hdr));
    // outer header: i < len?
    ctx.cur_block = outer_hdr;
    let i_now = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(i_slot), 0),
        Type::I64,
        None,
    );
    let in_outer = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Slt, Operand::Value(i_now), Operand::Value(len)),
        Type::Bool,
        None,
    );
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(in_outer),
            then_blk: outer_body,
            else_blk: outer_after,
        },
    );
    // outer body: load cur = xs[i], j = i, then inner loop
    ctx.cur_block = outer_body;
    let i_now2 = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(i_slot), 0),
        Type::I64,
        None,
    );
    // T-13.5: head-aware byte offset for arr.sort() reads.
    let off_i =
        ctx.emit_arr_slot_byte_offset(Operand::Value(arr_ptr), Operand::Value(i_now2), 3, false);
    let cur = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::LoadDyn(elem_ty, Operand::Value(arr_ptr), off_i),
        elem_ty,
        None,
    );
    let j_slot = ctx.alloca(Type::I64, Some("__sort_j"));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(i_now2), Operand::Value(j_slot), 0),
    );
    // inner loop: while j > 0 && cmp(xs[j-1], cur) > 0: shift
    let inner_hdr = ctx.f.add_block();
    let inner_check = ctx.f.add_block();
    let inner_body = ctx.f.add_block();
    let inner_after = ctx.f.add_block();
    ctx.f.set_term(ctx.cur_block, Terminator::Br(inner_hdr));
    // inner header: j > 0?
    ctx.cur_block = inner_hdr;
    let j_now = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(j_slot), 0),
        Type::I64,
        None,
    );
    let j_pos = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Sgt, Operand::Value(j_now), Operand::ConstI64(0)),
        Type::Bool,
        None,
    );
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(j_pos),
            then_blk: inner_check,
            else_blk: inner_after,
        },
    );
    // inner check: load xs[j-1], call cmp, test > 0
    ctx.cur_block = inner_check;
    let j_minus_1 = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::BinOp(SsaBinOp::Sub, Operand::Value(j_now), Operand::ConstI64(1)),
        Type::I64,
        None,
    );
    let off_jm1 =
        ctx.emit_arr_slot_byte_offset(Operand::Value(arr_ptr), Operand::Value(j_minus_1), 3, false);
    let prev = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::LoadDyn(elem_ty, Operand::Value(arr_ptr), off_jm1.clone()),
        elem_ty,
        None,
    );
    // V3-18 wedge — branch on cmp_val presence.
    // With a user comparator: call it, test ret > 0.
    // Without: directly compare prev > cur using the
    // element-type-aware predicate (Sgt for I64,
    // Ogt for F64, str_locale_compare for Str).
    let pred_v = pred::emit_sort_pred(ctx, cmp_val, cmp_ty, prev, cur, elem_ty);
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(pred_v),
            then_blk: inner_body,
            else_blk: inner_after,
        },
    );
    // inner body: xs[j] = xs[j-1]; j--
    ctx.cur_block = inner_body;
    let off_j =
        ctx.emit_arr_slot_byte_offset(Operand::Value(arr_ptr), Operand::Value(j_now), 3, false);
    // off_jm1 was computed in inner_check; recompute
    // here since this is a different block.
    let off_jm1_b =
        ctx.emit_arr_slot_byte_offset(Operand::Value(arr_ptr), Operand::Value(j_minus_1), 3, false);
    let prev2 = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::LoadDyn(elem_ty, Operand::Value(arr_ptr), off_jm1_b),
        elem_ty,
        None,
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::StoreDyn(Operand::Value(prev2), Operand::Value(arr_ptr), off_j),
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(j_minus_1), Operand::Value(j_slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(inner_hdr));
    // inner after: xs[j] = cur
    ctx.cur_block = inner_after;
    let j_final = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(j_slot), 0),
        Type::I64,
        None,
    );
    let off_jf =
        ctx.emit_arr_slot_byte_offset(Operand::Value(arr_ptr), Operand::Value(j_final), 3, false);
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::StoreDyn(Operand::Value(cur), Operand::Value(arr_ptr), off_jf),
    );
    // i++
    let i_next = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::BinOp(SsaBinOp::Add, Operand::Value(i_now2), Operand::ConstI64(1)),
        Type::I64,
        None,
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(i_next), Operand::Value(i_slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(outer_hdr));
    ctx.cur_block = outer_after;
}
