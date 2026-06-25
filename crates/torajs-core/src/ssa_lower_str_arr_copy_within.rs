//! `<Arr>.copyWithin(target, start?, end?)` dispatch — eighth
//! sub-split carved out of [`ssa_lower_str::try_lower_method_call`].
//! Lowers the JS spec §22.1.3.3 in-place memmove with full
//! ToIntegerOrInfinity normalisation on target / start / end (negative
//! → length-relative wrap, positive overshoot → length clamp) plus
//! the Phase B refcount discipline: when the element type is non-Copy
//! the dst range gets aliased ptrs from the src range, so the
//! sequence MUST be `inc(src) → drop(dst) → arr_copy_within` to avoid
//! freeing an overlapping element before its inc.
//!
//! Returns `None` for non-Arr receivers or for the
//! `method != "copyWithin"` case so the caller can keep trying the
//! remaining branches.

use crate::ast::ExprId;
use crate::ssa::{BinOp as SsaBinOp, InstKind, Operand, Type};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx};

/// Try to lower `<Arr>.copyWithin(target, start?, end?)`. Returns
/// `Some(value)` when handled; `None` otherwise.
pub(crate) fn try_dispatch(
    ctx: &mut LowerCtx<'_>,
    method: &str,
    args: &[ExprId],
    recv_op: Operand,
    recv_ty: Type,
) -> Option<Operand> {
    // `arr.copyWithin(target, start, end)` — in-place
    // memmove via runtime helper.
    //
    // V3-18 wedge — start / end are optional per JS
    // spec §22.1.3.3:
    //   xs.copyWithin(t)        = (t, 0, len)
    //   xs.copyWithin(t, s)     = (t, s, len)
    //   xs.copyWithin(t, s, e)  = (t, s, e)
    // Pre-fix the 3-arg branch was strict so check.rs's
    // fixed-arity rejected the shorter calls. Defaults
    // resolved here so the rc-aware path below sees
    // the canonical 3-arg form.
    //
    // Phase B refcount: for non-Copy elements the dst
    // range gets aliased ptrs from the src range. The
    // sequence MUST be inc-src-first then drop-dst,
    // otherwise an overlapping element could be freed
    // before its inc (use-after-free). Then arr_copy_within
    // does the bytewise memmove; refcounts now reflect
    // the post-copy slot ownership so the array's
    // eventual element-walk drop is balanced.
    // S246 widens the 3-arg ceiling to 4-arg so the trailing-arg
    // shape (copyWithin(t, s, e, trailing)) still routes through
    // this fast path; args[3] is never lowered (trailing-arg ignore).
    // S310 widens upper bound to >=1 so any number of trailing
    // args route here; existing skip(3) loop already drains them.
    if let Type::Arr(arr_id) = recv_ty
        && method == "copyWithin"
        && !args.is_empty()
    {
        // V3-18 wedge — copyWithin per JS spec
        // §22.1.3.3 normalises target / start / end
        // through ToIntegerOrInfinity, then for each
        // index n: if n < 0, n = max(len + n, 0); if
        // n >= len, n = len. Pre-fix tora used a
        // plain clamp_i64_to_range(0, len) which
        // mapped any negative input to 0, dropping
        // the canonical TS pattern of using
        // negatives to count from the end (e.g.
        // copyWithin(-2) shifts the tail to the
        // front).
        let len_for_norm = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(Type::I64, recv_op, ARR_LEN_OFF),
            Type::I64,
            None,
        );
        // S219 — Array.copyWithin(target [, start [, end ]]) per ES
        // §23.1.3.4: target/start go through ToIntegerOrInfinity
        // (undef → 0); end===undefined takes the omitted default (len).
        // Short-circuit each undef slot before relative_to_len so the
        // helper isn't called on a ConstPtrNull undef sentinel.
        // S335 — Any args for copyWithin: decode each Any arg via
        // anyv_to_number → coerce_to_i64 before relative_to_len so
        // the helper sees a clean i64. Sister to S334.
        let arg0_undef = matches!(
            ctx.expr_types.get(&args[0]),
            Some(crate::check::Type::Undefined)
        );
        let arg0_any = matches!(ctx.expr_types.get(&args[0]), Some(crate::check::Type::Any));
        let target = if arg0_undef {
            Operand::ConstI64(0)
        } else if arg0_any {
            let raw = ctx.lower_expr(args[0]);
            let f = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.any_to_number, vec![raw]),
                Type::F64,
                None,
            );
            let i = ctx.coerce_to_i64(Operand::Value(f));
            ctx.relative_to_len(i, Operand::Value(len_for_norm))
        } else {
            let raw_target = ctx.lower_expr(args[0]);
            ctx.relative_to_len(raw_target, Operand::Value(len_for_norm))
        };
        let start = if args.len() >= 2 {
            let arg1_undef = matches!(
                ctx.expr_types.get(&args[1]),
                Some(crate::check::Type::Undefined)
            );
            let arg1_any = matches!(ctx.expr_types.get(&args[1]), Some(crate::check::Type::Any));
            if arg1_undef {
                Operand::ConstI64(0)
            } else if arg1_any {
                let raw = ctx.lower_expr(args[1]);
                let f = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Call(ctx.intrinsics.any_to_number, vec![raw]),
                    Type::F64,
                    None,
                );
                let i = ctx.coerce_to_i64(Operand::Value(f));
                ctx.relative_to_len(i, Operand::Value(len_for_norm))
            } else {
                let raw = ctx.lower_expr(args[1]);
                ctx.relative_to_len(raw, Operand::Value(len_for_norm))
            }
        } else {
            Operand::ConstI64(0)
        };
        let end = if args.len() >= 3 {
            let arg2_undef = matches!(
                ctx.expr_types.get(&args[2]),
                Some(crate::check::Type::Undefined)
            );
            let arg2_any = matches!(ctx.expr_types.get(&args[2]), Some(crate::check::Type::Any));
            if arg2_undef {
                Operand::Value(len_for_norm)
            } else if arg2_any {
                let raw = ctx.lower_expr(args[2]);
                let f = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Call(ctx.intrinsics.any_to_number, vec![raw]),
                    Type::F64,
                    None,
                );
                let i = ctx.coerce_to_i64(Operand::Value(f));
                ctx.relative_to_len(i, Operand::Value(len_for_norm))
            } else {
                let raw = ctx.lower_expr(args[2]);
                ctx.relative_to_len(raw, Operand::Value(len_for_norm))
            }
        } else {
            // end = recv.length
            Operand::Value(len_for_norm)
        };
        let elem_ty = ctx.arr_layouts[arr_id.0 as usize];
        if elem_ty.is_refcounted() {
            // Replicate arr_copy_within's clamp: lo = clamp(start),
            // hi = clamp(end), to = clamp(target), count = min(hi-lo,
            // len-to). Then inc src [lo, lo+count) and drop dst
            // [to, to+count) before the memmove.
            let len_v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(Type::I64, recv_op, ARR_LEN_OFF),
                Type::I64,
                None,
            );
            let len_op = Operand::Value(len_v);
            let lo = ctx.clamp_i64_to_range(start, Operand::ConstI64(0), len_op);
            let hi = ctx.clamp_i64_to_range(end, Operand::ConstI64(0), len_op);
            let to = ctx.clamp_i64_to_range(target, Operand::ConstI64(0), len_op);
            // raw_count = max(0, hi - lo)
            let diff = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::BinOp(SsaBinOp::Sub, hi, lo),
                Type::I64,
                None,
            );
            let raw_count =
                ctx.clamp_i64_to_range(Operand::Value(diff), Operand::ConstI64(0), len_op);
            // capacity left at dst = len - to
            let cap_left = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::BinOp(SsaBinOp::Sub, len_op, to),
                Type::I64,
                None,
            );
            // count = min(raw_count, cap_left), clamped >= 0
            let count =
                ctx.clamp_i64_to_range(raw_count, Operand::ConstI64(0), Operand::Value(cap_left));
            // src_end = lo + count, dst_end = to + count
            let src_end = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::BinOp(SsaBinOp::Add, lo, count),
                Type::I64,
                None,
            );
            let dst_end = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::BinOp(SsaBinOp::Add, to, count),
                Type::I64,
                None,
            );
            ctx.emit_arr_rc_inc_range(recv_op, lo, Operand::Value(src_end));
            ctx.emit_arr_rc_drop_range(recv_op, elem_ty, to, Operand::Value(dst_end));
        }
        // S298 — lower-and-drop trailing args past the 3 useful
        // (target, start, end) slots per ES §23.1.3.4 trailing-arg
        // ignore (S272 idiom). check.rs S246 already accepts the
        // 4-arg shape; mirror at lower-time so step()-style side-
        // effect exprs fire.
        for &a in args.iter().skip(3) {
            let _ = ctx.lower_expr(a);
        }
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.arr_copy_within,
                vec![recv_op, target, start, end],
            ),
            Type::Arr(arr_id),
            None,
        );
        return Some(Operand::Value(v));
    }
    None
}
