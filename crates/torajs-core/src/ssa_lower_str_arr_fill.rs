//! `<Arr>.fill(v, start?, end?)` dispatch — tenth sub-split carved
//! out of [`ssa_lower_str::try_lower_method_call`]. Lowers the JS
//! spec §23.1.3.7 uniform fill over the [start, end) range, picking
//! the right backing helper per element-layout width / kind
//! (`__torajs_arr_fill_*` for primitive 8-byte slots; an inline
//! per-slot SSA loop with drop-old → store-new → inc-new for
//! non-Copy refcounted elements / Substr / Any so the refcount
//! discipline stays balanced after the overwrite).
//!
//! Returns `None` for non-Arr receivers or for the `method != "fill"`
//! case so the caller can keep trying the remaining branches.

use crate::ast::ExprId;
use crate::ssa::{BinOp as SsaBinOp, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx};

/// Normalise one length-relative range bound (`args[idx]`) to a
/// canonical [0, len] index. Chunk 479 — fill's start / end arms
/// were the same shape; only the omitted / undef default differs
/// (`0` for start, `len` for end — both carried in `default_op`).
/// Chunk 480 reuses it for copyWithin's target / start / end (same
/// S219/S335 shape).
///
/// - S218 — undef slot per ES §23.1.3.7 step 5/9 short-circuits to
///   the omitted default before lower so `relative_to_len` isn't
///   invoked on a ConstPtrNull/I64 undef sentinel.
/// - S335 — Any slot decodes via `any_to_number → coerce_to_i64`
///   before `relative_to_len` (ToIntegerOrInfinity accepts
///   arbitrary-typed input; sister to S334).
/// - V3-18 wedge — negatives count from the end via
///   `relative_to_len` (max(len + n, 0) for n < 0, len for
///   n >= len); the arr_fill C runtime only does plain min/max
///   clamp, so both Copy and non-Copy paths see canonical indices.
pub(crate) fn normalize_range_bound(
    ctx: &mut LowerCtx<'_>,
    args: &[ExprId],
    idx: usize,
    default_op: Operand,
    len_for_norm: ValueId,
) -> Operand {
    if args.len() <= idx {
        return default_op;
    }
    let is_undef = matches!(
        ctx.expr_types.get(&args[idx]),
        Some(crate::check::Type::Undefined)
    );
    let is_any = matches!(
        ctx.expr_types.get(&args[idx]),
        Some(crate::check::Type::Any)
    );
    if is_undef {
        default_op
    } else if is_any {
        let raw = ctx.lower_expr(args[idx]);
        let f = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.any_to_number, vec![raw]),
            Type::F64,
            None,
        );
        let i = ctx.coerce_to_i64(Operand::Value(f));
        ctx.relative_to_len(i, Operand::Value(len_for_norm))
    } else {
        let raw = ctx.lower_expr(args[idx]);
        ctx.relative_to_len(raw, Operand::Value(len_for_norm))
    }
}

/// Non-Copy fill: per-slot drop-old + store-new + inc-new SSA loop
/// so the refcount discipline stays balanced after the overwrite.
/// Clamps [start, end) to [0, len] inline (matches arr_fill C
/// semantics). Leaves `ctx.cur_block` at the after-block and
/// returns the receiver.
fn emit_fill_rc_loop(
    ctx: &mut LowerCtx<'_>,
    recv_op: Operand,
    value: Operand,
    start: Operand,
    end: Operand,
    elem_ty: Type,
) -> Operand {
    let len_v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, recv_op.clone(), ARR_LEN_OFF),
        Type::I64,
        None,
    );
    let lo = ctx.clamp_i64_to_range(start, Operand::ConstI64(0), Operand::Value(len_v));
    let hi = ctx.clamp_i64_to_range(end, Operand::ConstI64(0), Operand::Value(len_v));
    let i_slot = ctx.alloca_in_entry(Type::I64, Some("__fill_i"));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(lo, Operand::Value(i_slot), 0),
    );
    let header = ctx.f.add_block();
    let body = ctx.f.add_block();
    let after = ctx.f.add_block();
    ctx.f.set_term(ctx.cur_block, Terminator::Br(header));
    ctx.cur_block = header;
    let i_now = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(i_slot), 0),
        Type::I64,
        None,
    );
    let cond = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Slt, Operand::Value(i_now), hi),
        Type::Bool,
        None,
    );
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(cond),
            then_blk: body,
            else_blk: after,
        },
    );
    ctx.cur_block = body;
    // T-13.5: head-aware offset for arr.fill loop.
    let off = ctx.emit_arr_slot_byte_offset(recv_op.clone(), Operand::Value(i_now), 3, false);
    let old = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::LoadDyn(elem_ty, recv_op.clone(), off.clone()),
        elem_ty,
        None,
    );
    ctx.emit_drop_value(Operand::Value(old), elem_ty);
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::StoreDyn(value.clone(), recv_op.clone(), off),
    );
    ctx.emit_rc_inc(value);
    let i_next = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::BinOp(SsaBinOp::Add, Operand::Value(i_now), Operand::ConstI64(1)),
        Type::I64,
        None,
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(i_next), Operand::Value(i_slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(header));
    ctx.cur_block = after;
    recv_op
}

/// Try to lower `<Arr>.fill(v, start?, end?)`. Returns
/// `Some(value)` when handled; `None` otherwise.
pub(crate) fn try_dispatch(
    ctx: &mut LowerCtx<'_>,
    method: &str,
    args: &[ExprId],
    recv_op: Operand,
    recv_ty: Type,
) -> Option<Operand> {
    // `arr.fill(v, start, end)` — uniform fill of the
    // [start, end) range. Element value is passed as
    // i64 (8-byte slot — works for i64 / Bool / Str /
    // Obj / Arr; f64 elements would need a bitcast not
    // yet in the IR). The intrinsic returns the same
    // pointer.
    //
    // Phase B refcount: for non-Copy elements, the
    // overwrite would leak the old value AND leave new-
    // value refcount imbalanced. Emit a per-slot SSA
    // loop that drops old + stores new + inc's new,
    // bypassing the C runtime for this case.
    // S246 widens the 3-arg ceiling to 4-arg so the trailing-arg
    // shape (fill(v, s, e, trailing)) still routes through this
    // fast path; args[3] is never lowered (trailing-arg ignore).
    // S310 widens upper bound to >=1 so any number of trailing
    // args route here; existing skip(3) loop already drains them.
    if let Type::Arr(arr_id) = recv_ty
        && method == "fill"
        && !args.is_empty()
    {
        let fill_elem = ctx.arr_layouts[arr_id.0 as usize];
        // L3b Array<Any>.fill — NaN-box the value and route through
        // `__torajs_arr_fill_any`. Pre-fix the non-Copy per-slot
        // loop below wrote raw bits into the 8-byte AnyValue slot
        // (StoreDyn(value, off) with value still raw), which
        // SIGSEGV'd the next decode pass. Mirror of the
        // local-Ident / K.8 / (b) Array<Any>.push helper family.
        if matches!(fill_elem, Type::Any) {
            return Some(ctx.emit_arr_any_fill_at(recv_op, args, recv_ty));
        }
        let mut value = ctx.lower_expr(args[0]);
        // W4 — align with the elem width, then cross the i64-param
        // intrinsic boundary as raw bits.
        if fill_elem == Type::F64 && ctx.operand_ty(&value) == Type::I64 {
            value = ctx.coerce_to_f64(value);
        }
        let value = ctx.raw_slot_arg(value);
        // V3-18 m1.h.53 — start defaults to 0, end
        // defaults to arr.length per JS spec §22.1.3.6.
        // V3-18 wedge — negatives count from the
        // end via relative_to_len (max(len + n, 0)
        // for n < 0, len for n >= len). The arr_fill
        // C runtime only does plain min/max clamp,
        // so the normalisation happens here so both
        // Copy and non-Copy paths see canonical
        // [0, len] indices.
        let len_for_norm = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(Type::I64, recv_op, ARR_LEN_OFF),
            Type::I64,
            None,
        );
        // S218 / S335 / V3-18 — see `normalize_fill_bound` doc:
        // start defaults to 0, end defaults to arr.length per JS
        // spec §22.1.3.6.
        let start = normalize_range_bound(ctx, args, 1, Operand::ConstI64(0), len_for_norm);
        let end = normalize_range_bound(ctx, args, 2, Operand::Value(len_for_norm), len_for_norm);
        let elem_ty = ctx.arr_layouts[arr_id.0 as usize];
        // S298 — lower-and-drop trailing args past the 3 useful
        // (value, start, end) slots per ES §23.1.3.7 trailing-arg
        // ignore (S272 idiom). check.rs S246 already widened to
        // accept the 4-arg shape; mirror at lower-time so step()-
        // style side-effect exprs fire.
        for &a in args.iter().skip(3) {
            let _ = ctx.lower_expr(a);
        }
        if elem_ty.is_copy() {
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.arr_fill, vec![recv_op, value, start, end]),
                Type::Arr(arr_id),
                None,
            );
            // RFC 20260705 owned-result invariant: chaining result
            // carries its own ref.
            ctx.emit_rc_inc(Operand::Value(v));
            return Some(Operand::Value(v));
        }
        let out = emit_fill_rc_loop(ctx, recv_op, value, start, end, elem_ty);
        ctx.emit_rc_inc(out.clone());
        return Some(out);
    }
    None
}
