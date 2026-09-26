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
        eval_discard(ctx, args);
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.arr_reverse, vec![recv_op]),
            Type::Arr(arr_id),
            None,
        );
        // The kernel refuses a frozen receiver (`__torajs_arr_frozen_guard`)
        // and is an intrinsic, so the check is forced with `None`.
        ctx.emit_throw_check(None);
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
        eval_discard(ctx, args);
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
        // A copy out of an `Arr<Substr>` is `Arr<Str>` (views do not
        // leave their split block — rotation 468).
        let out_id = ctx.copied_arr_layout(arr_id);
        let fill = hole_fill_for(ctx, elem_ty);
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.arr_to_reversed, vec![recv_op, fill]),
            Type::Arr(out_id),
            None,
        );
        if elem_ty.is_refcounted() {
            let len = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(Type::I64, Operand::Value(v), ARR_LEN_OFF),
                Type::I64,
                None,
            );
            ctx.emit_adopt_copied_range(
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
        // S339 — `xs.with(idx, val)` per ES §23.1.3.39 step 2:
        // ToIntegerOrInfinity COERCES `index`, so every operand shape
        // reaches it and only `Number` stays on the typed-tier fast
        // path. `lower_to_number_operand` + `coerce_to_i64` give the
        // helper's `(Arr, i64, v: i64)` ABI a clean index; rotation 463
        // widened this from an `Any`-only admission, in step with the
        // checker mirror in `check_type_of_call_array_with_any`.
        let i_val = if matches!(
            ctx.expr_types.get(&args[0]),
            Some(crate::check::Type::Number)
        ) {
            ctx.lower_expr(args[0])
        } else {
            let n = ctx.lower_to_number_operand(args[0]);
            ctx.coerce_to_i64(n)
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
        eval_discard(ctx, args.get(2..).unwrap_or(&[]));
        // A copy out of an `Arr<Substr>` is `Arr<Str>` (views do not
        // leave their split block — rotation 468).
        let out_id = ctx.copied_arr_layout(arr_id);
        let fill = hole_fill_for(ctx, elem_layout);
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.arr_with, vec![recv_op, i_val, v_val, fill]),
            Type::Arr(out_id),
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
            ctx.emit_adopt_copied_range(
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

/// The array twin of `emit_field_writable_guard`: a FROZEN receiver
/// raises §10.1.9's TypeError before the first slot write, and nothing
/// else does — sealed / preventExtensions keep writable elements, so
/// this reads `FLAG_FROZEN` alone (the same two-instruction header
/// read; the u16 flags are the high half of the u32 at +4).
///
/// Needed in the lowering, not only in the kernels: `fill` on a
/// refcounted element type, `sort`'s insertion path and `copyWithin`
/// on refcounted elements all write slots INLINE without any kernel
/// call, the same way the pre-reserved push loop does. Emitted once at
/// each method's entry, after its arguments are evaluated — which is
/// where §23.1.3.7 / .30 / .4 put the failing Set — so the kernel
/// guards behind it only ever fire for a caller that reaches a kernel
/// directly. Leaves `cur_block` on the fall-through block.
/// `writes` is the caller's "a slot will actually be written" predicate
/// — a non-empty range, a positive copy count, two or more elements to
/// sort. §10.1.9 raises at the Set, so a frozen receiver that nothing
/// would be written to is legal: `Object.freeze([]).fill(1)` and
/// `Object.freeze([1]).reverse()` are test262 cases, and the first
/// version of this guard (b5dbd89b5) failed both by raising at entry.
pub(crate) fn emit_arr_frozen_guard(ctx: &mut LowerCtx<'_>, recv_op: Operand, writes: Operand) {
    emit_arr_lock_bits_guard(ctx, recv_op, torajs_rc::FLAG_FROZEN, Some(writes), None);
}

/// The guard above with the bits and the conjunct as parameters: the
/// pre-reserved fill loop refuses `FROZEN | SEALED | NON_EXTENSIBLE`
/// (an append adds an index — §10.1.8.1) where the inline writers
/// above refuse `FROZEN` alone. `cond` ANDs into the hit (`None` is
/// unconditional); `owned` is a minted value the throw path must
/// release before leaving (see `emit_throw_check_owned`).
pub(crate) fn emit_arr_lock_bits_guard(
    ctx: &mut LowerCtx<'_>,
    recv_op: Operand,
    bits: u16,
    cond: Option<Operand>,
    owned: Option<(Operand, Type)>,
) {
    let hdr_word = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I32, recv_op, 4),
        Type::I32,
        None,
    );
    let lock_bits = (bits as u32) << 16;
    let masked = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::BinOp(
            crate::ssa::BinOp::And,
            Operand::Value(hdr_word),
            Operand::ConstI32(lock_bits as i32),
        ),
        Type::I32,
        None,
    );
    let locked = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(
            crate::ssa::IPred::Ne,
            Operand::Value(masked),
            Operand::ConstI32(0),
        ),
        Type::Bool,
        None,
    );
    let hit = match cond {
        Some(c) => ctx.f.append_inst(
            ctx.cur_block,
            InstKind::BinOp(crate::ssa::BinOp::And, Operand::Value(locked), c),
            Type::Bool,
            None,
        ),
        None => locked,
    };
    let throw_blk = ctx.f.add_block();
    let cont_blk = ctx.f.add_block();
    ctx.f.set_term(
        ctx.cur_block,
        crate::ssa::Terminator::CondBr {
            cond: Operand::Value(hit),
            then_blk: throw_blk,
            else_blk: cont_blk,
        },
    );
    ctx.cur_block = throw_blk;
    let raiser = ctx.intrinsics.throw_readonly_assign;
    ctx.f
        .append_void(ctx.cur_block, InstKind::Call(raiser, vec![]));
    match owned {
        Some((v, ty)) => ctx.emit_throw_check_owned(None, v, ty),
        None => ctx.emit_throw_check(None),
    }
    // The check's fall-through can never run (the raiser always arms
    // the flag) and is terminated as such — a `Br` back to the
    // continue block would make this a loop-body block wherever the
    // caller sits in a loop (the pre-reserve lane measured +69% on
    // array-sum-1m from exactly that).
    let cb = ctx.cur_block;
    ctx.f.set_term(cb, crate::ssa::Terminator::Unreachable);
    ctx.cur_block = cont_blk;
}

/// The `undefined` a [[Get]] copy writes where its source has a hole:
/// the element type's own undefined cell (the Str-shaped one for
/// strings and function values, the generic one for other pointers) —
/// a hole word `0` reads as `null`, and the copy carries no hole mark
/// to say otherwise. Other kinds pass `0` (keep the copied word): a
/// number slot's hole already holds its undefined sentinel.
pub(crate) fn hole_fill_for(ctx: &mut LowerCtx, elem_ty: Type) -> Operand {
    // A copy out of an `Arr<Substr>` lands as `Arr<Str>`.
    let slot_ty = if elem_ty == Type::Substr {
        Type::Str
    } else {
        elem_ty
    };
    let Some(cell) = ctx.str_undef_sentinel_for(slot_ty) else {
        return Operand::ConstI64(0);
    };
    Operand::Value(
        ctx.f
            .append_inst(ctx.cur_block, InstKind::PtrToInt(cell), Type::I64, None),
    )
}

/// Evaluate arguments the method ignores, for their side effects.
fn eval_discard(ctx: &mut LowerCtx, args: &[ExprId]) {
    for &a in args {
        let _ = ctx.lower_expr(a);
    }
}
