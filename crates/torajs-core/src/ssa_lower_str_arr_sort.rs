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
//! Returns `None` when the receiver is not `Type::Arr` or the
//! method is neither `sort` nor `toSorted` so the caller can keep
//! trying the remaining branches.

use crate::ast::ExprId;
use crate::ssa::{BinOp as SsaBinOp, FPred, IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx};

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
        let len = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(Type::I64, recv_op, ARR_LEN_OFF),
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
        let off_i = ctx.emit_arr_slot_byte_offset(
            Operand::Value(arr_ptr),
            Operand::Value(i_now2),
            3,
            false,
        );
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
        let off_jm1 = ctx.emit_arr_slot_byte_offset(
            Operand::Value(arr_ptr),
            Operand::Value(j_minus_1),
            3,
            false,
        );
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
        let pred_v = match (&cmp_val, &cmp_ty) {
            (Some(cv), Some(ct)) => {
                let cmp_ret = ctx.call_fn_value(
                    cv.clone(),
                    *ct,
                    vec![Operand::Value(prev), Operand::Value(cur)],
                );
                let cmp_ret_ty = ctx.f.value_type(cmp_ret);
                match cmp_ret_ty {
                    Type::F64 => ctx.f.append_inst(
                        ctx.cur_block,
                        InstKind::FCmp(FPred::Ogt, Operand::Value(cmp_ret), Operand::ConstF64(0.0)),
                        Type::Bool,
                        None,
                    ),
                    _ => ctx.f.append_inst(
                        ctx.cur_block,
                        InstKind::ICmp(IPred::Sgt, Operand::Value(cmp_ret), Operand::ConstI64(0)),
                        Type::Bool,
                        None,
                    ),
                }
            }
            _ => {
                // ES §23.1.3.30 SortCompare with no comparator =
                // ToString each operand then code-unit lex compare.
                // Pre-Spec-fix wedge used numeric `prev > cur` which
                // ordered `[10, 2]` as `[2, 10]` — disagrees with bun.
                // For each numeric / bool element, route through the
                // existing `*_to_str` intrinsics so the resulting Str
                // operands feed the same `str_locale_compare`
                // (bytewise; see runtime doc) the Str arm uses. Obj /
                // Arr / etc fall back to the legacy pointer ICmp —
                // no `*_to_str` exists for them yet and the spec
                // result (`"[object Object]"`-tied tie-break) is
                // niche enough to leave behind a follow-up.
                let to_str = |ctx: &mut LowerCtx, v, ty: Type| match ty {
                    Type::Str | Type::Substr => Some(v),
                    Type::I64 => Some(ctx.f.append_inst(
                        ctx.cur_block,
                        InstKind::Call(ctx.intrinsics.i64_to_str, vec![Operand::Value(v)]),
                        Type::Str,
                        None,
                    )),
                    Type::F64 => Some(ctx.f.append_inst(
                        ctx.cur_block,
                        InstKind::Call(ctx.intrinsics.f64_to_str, vec![Operand::Value(v)]),
                        Type::Str,
                        None,
                    )),
                    Type::Bool => Some(ctx.f.append_inst(
                        ctx.cur_block,
                        InstKind::Call(ctx.intrinsics.bool_to_str, vec![Operand::Value(v)]),
                        Type::Str,
                        None,
                    )),
                    _ => None,
                };
                let prev_s = to_str(ctx, prev, elem_ty);
                let cur_s = to_str(ctx, cur, elem_ty);
                if let (Some(ps), Some(cs)) = (prev_s, cur_s) {
                    let r = ctx.f.append_inst(
                        ctx.cur_block,
                        InstKind::Call(
                            ctx.intrinsics.str_locale_compare,
                            vec![Operand::Value(ps), Operand::Value(cs)],
                        ),
                        Type::I64,
                        None,
                    );
                    ctx.f.append_inst(
                        ctx.cur_block,
                        InstKind::ICmp(IPred::Sgt, Operand::Value(r), Operand::ConstI64(0)),
                        Type::Bool,
                        None,
                    )
                } else {
                    ctx.f.append_inst(
                        ctx.cur_block,
                        InstKind::ICmp(IPred::Sgt, Operand::Value(prev), Operand::Value(cur)),
                        Type::Bool,
                        None,
                    )
                }
            }
        };
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
        let off_jm1_b = ctx.emit_arr_slot_byte_offset(
            Operand::Value(arr_ptr),
            Operand::Value(j_minus_1),
            3,
            false,
        );
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
        let off_jf = ctx.emit_arr_slot_byte_offset(
            Operand::Value(arr_ptr),
            Operand::Value(j_final),
            3,
            false,
        );
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
        return Some(Operand::Value(arr_ptr));
    }
    None
}
