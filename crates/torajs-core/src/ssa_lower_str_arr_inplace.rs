//! Array-receiver in-place / clone-then-rewrite shapes
//! (`reverse` / `toReversed` / `with`) — ninth sub-split carved out
//! of [`ssa_lower_str::try_lower_method_call`]. Lowers the three
//! short-body Arr-receiver methods that produce either the same
//! pointer (in-place) or a fresh clone with a localised rewrite:
//!
//! - `reverse()` — in-place via `arr_reverse`, returns receiver.
//! - `toReversed()` — `arr_to_reversed` clones first then reverses.
//! - `with(idx, value)` — clones the receiver, then performs the
//!   element-type-aware drop-old / inc-new sequence on the single
//!   target slot so the surrounding refcount discipline stays
//!   balanced for non-Copy elements.
//!
//! Returns `None` for non-Arr receivers or methods outside this
//! dispatch so the caller can keep trying the remaining branches.

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx};

/// Try to lower `<Arr>.reverse | toReversed | with(idx, value)`.
/// Returns `Some(value)` when handled; `None` otherwise.
pub(crate) fn try_dispatch(
    ctx: &mut LowerCtx<'_>,
    method: &str,
    args: &[ExprId],
    recv_op: Operand,
    recv_ty: Type,
) -> Option<Operand> {
    // `arr.reverse()` — in-place over the receiver,
    // returns the same array pointer for chaining.
    // S287 — accept any trailing operands per ES §23.1.3.27
    // trailing-arg ignore; lower-and-drop so step()-style
    // side-effect exprs fire per ES eval-then-discard (S272
    // idiom). Runtime helper ABI is single-arg `recv`.
    if let Type::Arr(arr_id) = recv_ty
        && method == "reverse"
    {
        for &a in args.iter() {
            let _ = ctx.lower_expr(a);
        }
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.arr_reverse, vec![recv_op]),
            Type::Arr(arr_id),
            None,
        );
        // RFC 20260705 owned-result invariant: the chaining result
        // carries its own ref; the receiver binding keeps its own.
        ctx.emit_rc_inc(Operand::Value(v));
        return Some(Operand::Value(v));
    }
    // `arr.toReversed()` — non-mutating reverse. Fresh
    // alloc + reverse-direction slot copy via the C
    // runtime; original untouched. Phase B refcount:
    // for non-Copy elements, inc each derived slot to
    // share ownership with the source (see arr.slice
    // for rationale).
    if let Type::Arr(arr_id) = recv_ty
        && method == "toReversed"
    {
        // S287 — accept trailing args per ES §23.1.3.33; lower-
        // and-drop before the call to fire side-effect exprs.
        for &a in args.iter() {
            let _ = ctx.lower_expr(a);
        }
        // 刀 13b (RFC 20260721-array-proto-cluster) — an Any-elem
        // receiver rides the §23.1.3.33 step 5 [[Get]] order through
        // `arr_any_to_reversed`: the SOURCE reads high→low
        // (`result[i] = Get(O, len-1-i)`), so an accessor getter's
        // mid-iteration mutations (length shrink, element writes)
        // are observed by the later lower-index reads, holes/OOB
        // continue to the prototype digit keys. A forward gather +
        // raw swap is NOT order-equivalent under getter side effects
        // (design.md 刀 13b 实测翻盘). The kernel incs each slot
        // itself (owned contract) — no rc_inc range. Typed receivers
        // keep the raw `arr_to_reversed` copy below.
        let elem_ty = ctx.arr_layouts[arr_id.0 as usize];
        if elem_ty == Type::Any {
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.arr_any_to_reversed, vec![recv_op]),
                Type::Arr(arr_id),
                None,
            );
            ctx.emit_throw_check(None);
            return Some(Operand::Value(v));
        }
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.arr_to_reversed, vec![recv_op]),
            Type::Arr(arr_id),
            None,
        );
        if elem_ty.is_refcounted() {
            let len = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(Type::I64, Operand::Value(v), ARR_LEN_OFF),
                Type::I64,
                None,
            );
            ctx.emit_arr_rc_inc_range(
                Operand::Value(v),
                elem_ty,
                Operand::ConstI64(0),
                Operand::Value(len),
            );
        }
        return Some(Operand::Value(v));
    }
    // `arr.with(i, v)` — non-mutating index update. The
    // C helper memcpy's the source array, then writes
    // `v` into the (negative-wrapped) `i` slot. Out-of-
    // bounds `i` is UB. Element value passed as i64 (the
    // 8-byte slot width); f64 elements would need a
    // bitcast not yet in the IR (matches `fill`).
    //
    // Phase B refcount: for non-Copy elements, the
    // derived array shares ownership of all non-`i`
    // slots with the source AND of slot `i` with the
    // caller's `v`. inc every slot uniformly — caller's
    // drop on `v` and source's per-slot drops balance
    // against the derived array's own per-slot drops.
    if let Type::Arr(arr_id) = recv_ty
        && method == "with"
        && args.len() >= 2
    {
        // S339 — `xs.with(Any idx, val)` per ES §23.1.3.39 step 2:
        // ToIntegerOrInfinity accepts arbitrary-typed input. Decode
        // Any via anyv_to_number → coerce_to_i64 so the helper's
        // (Arr, i64, v: i64) ABI sees a clean i64 idx. Pattern A
        // sister to S331/S332/S333/S334/S335. check.rs mirror at
        // ~8729 widens the strict-Number gate to accept Any.
        let arg0_any = matches!(ctx.expr_types.get(&args[0]), Some(crate::check::Type::Any));
        let i_val = if arg0_any {
            let raw = ctx.lower_expr(args[0]);
            let f = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.any_to_number, vec![raw]),
                Type::F64,
                None,
            );
            ctx.coerce_to_i64(Operand::Value(f))
        } else {
            ctx.lower_expr(args[0])
        };
        let v_raw = ctx.lower_expr(args[1]);
        let v_ty = ctx.operand_ty(&v_raw);
        // RFC 20260713-array-proto-residual B2 — an Any-elem receiver
        // stores NaN-box slots; a raw scalar/pointer written verbatim
        // reads back as garbage (and the post-call NaN-box-safe rc
        // walk chases it — the with/immutable SIGSEGV). Box first;
        // the walk then incs the slot like every copied box.
        let elem_layout = ctx.arr_layouts[arr_id.0 as usize];
        let v_val = if elem_layout == Type::Any && v_ty != Type::Any {
            ctx.box_to_any(v_raw)
        } else if elem_layout == Type::F64 {
            // RFC 20260726-array-elem-width — the helper's slot
            // argument is spelled i64 because a slot is 8 bytes, not
            // because the element is an integer. Handing it a raw i64
            // wrote the integer's own bits into an f64 slot, so
            // `xs.with(1, 99)` on a widened array read back as
            // 4.9e-322 — silently, exit 0. An f64 value used to panic
            // here as "needs an IR bitcast"; `BitCastF64ToI64` has
            // existed since, so both cases convert and then pun.
            let as_f64 = if v_ty == Type::F64 {
                v_raw
            } else {
                ctx.coerce_to_f64(v_raw)
            };
            Operand::Value(ctx.f.append_inst(
                ctx.cur_block,
                InstKind::BitCastF64ToI64(as_f64),
                Type::I64,
                None,
            ))
        } else {
            v_raw
        };
        // S283 — lower-and-drop trailing args[2..] per S272 idiom so
        // step()-style side-effect exprs fire per ES eval-then-discard
        // semantics. The arr_with helper is 2-arg only; trailing slots
        // never reach it.
        for &a in args.iter().skip(2) {
            let _ = ctx.lower_expr(a);
        }
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.arr_with, vec![recv_op, i_val, v_val]),
            Type::Arr(arr_id),
            None,
        );
        // ES §23.1.3.39 step 7 — out-of-range index throws
        // RangeError. The runtime helper records a pending throw via
        // TLS and returns a null ptr; propagate immediately so the
        // following rc-inc walk never touches the null result.
        ctx.emit_throw_check(None);
        let elem_ty = ctx.arr_layouts[arr_id.0 as usize];
        if elem_ty.is_refcounted() {
            let len = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(Type::I64, Operand::Value(v), ARR_LEN_OFF),
                Type::I64,
                None,
            );
            ctx.emit_arr_rc_inc_range(
                Operand::Value(v),
                elem_ty,
                Operand::ConstI64(0),
                Operand::Value(len),
            );
        }
        return Some(Operand::Value(v));
    }
    None
}
