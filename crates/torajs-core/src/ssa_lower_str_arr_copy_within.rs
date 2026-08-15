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
use crate::ssa_lower_str_arr_fill::normalize_range_bound;

/// Phase B refcount discipline for non-Copy elements — the dst range
/// gets aliased ptrs from the src range, so the sequence MUST be
/// `inc(src) → drop(dst)` before arr_copy_within's memmove (an
/// overlapping element could otherwise be freed before its inc =
/// use-after-free). Replicates arr_copy_within's clamp: lo =
/// clamp(start), hi = clamp(end), to = clamp(target), count =
/// min(hi-lo, len-to); then incs src [lo, lo+count) and drops dst
/// [to, to+count).
fn emit_copy_within_rc_ranges(
    ctx: &mut LowerCtx<'_>,
    recv_op: Operand,
    elem_ty: Type,
    target: Operand,
    start: Operand,
    end: Operand,
) {
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
    let raw_count = ctx.clamp_i64_to_range(Operand::Value(diff), Operand::ConstI64(0), len_op);
    // capacity left at dst = len - to
    let cap_left = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::BinOp(SsaBinOp::Sub, len_op, to),
        Type::I64,
        None,
    );
    // count = min(raw_count, cap_left), clamped >= 0
    let count = ctx.clamp_i64_to_range(raw_count, Operand::ConstI64(0), Operand::Value(cap_left));
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
    ctx.emit_arr_rc_inc_range(recv_op, elem_ty, lo, Operand::Value(src_end));
    ctx.emit_arr_rc_drop_range(recv_op, elem_ty, to, Operand::Value(dst_end));
}

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
        // S219 / S335 — see `normalize_range_bound` doc (fill's
        // sibling, chunk 480 reuse): target/start default 0,
        // end===undefined takes the omitted default (len).
        let target = normalize_range_bound(ctx, args, 0, Operand::ConstI64(0), len_for_norm);
        let start = normalize_range_bound(ctx, args, 1, Operand::ConstI64(0), len_for_norm);
        let end = normalize_range_bound(ctx, args, 2, Operand::Value(len_for_norm), len_for_norm);
        let elem_ty = ctx.arr_layouts[arr_id.0 as usize];
        if elem_ty.is_refcounted() {
            emit_copy_within_rc_ranges(ctx, recv_op, elem_ty, target, start, end);
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
        // RFC 20260705 owned-result invariant: chaining result
        // carries its own ref.
        ctx.emit_rc_inc(Operand::Value(v));
        return Some(Operand::Value(v));
    }
    None
}
