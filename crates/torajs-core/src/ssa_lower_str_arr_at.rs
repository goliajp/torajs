//! `<Arr>.at(i?)` dispatch — thirteenth sub-split carved out of
//! [`ssa_lower_str::try_lower_method_call`]. Inline SSA that
//! reads element `i` with JS spec §23.1.3.1 negative-index wrap
//! (`i < 0 ? len + i : i`) and out-of-bounds UB matching the
//! unchecked `xs[i]` indexing convention.
//!
//! Arity defaults: 0-arg form returns `arr[0]` per
//! ToIntegerOrInfinity(undefined) = 0; `args[1..]` are
//! lower-and-dropped per S299 trailing-arg eval-then-discard.
//! S225 explicit-undefined index short-circuits to ConstI64(0)
//! before coerce since ConstPtrNull can't materialize via
//! coerce_to_i64.
//!
//! Returns `None` for non-Arr receivers or `method != "at"` so the
//! caller keeps trying the remaining branches.

use crate::ast::ExprId;
use crate::ssa::{BinOp as SsaBinOp, IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx};

/// Try to lower `<Arr>.at(i?)`. Returns `Some(value)` when handled;
/// `None` otherwise.
pub(crate) fn try_dispatch(
    ctx: &mut LowerCtx<'_>,
    method: &str,
    args: &[ExprId],
    recv_op: Operand,
    recv_ty: Type,
) -> Option<Operand> {
    // `arr.at(i?)` — element at i with negative-index wrap.
    // Inline SSA: idx = i < 0 ? len + i : i; load at idx.
    // Out-of-bounds is UB (matches the unchecked indexing
    // convention).
    //
    // Arity defaults per ES §23.1.3.1 step 2-3: undefined index
    // routes through ToIntegerOrInfinity → 0, so `arr.at()`
    // returns `arr[0]`. 0-arg form emits `i_val = ConstI64(0)`
    // and skips through the rest of the negative-wrap select.
    // S242 — Array<T>.at(idx, ...trailing) trailing-arg ignore per
    // ES §23.1.3.1: args[1..] is never read (i_val uses args[0] only).
    // S299 — widen upper-cap `args.len() <= 2` → no cap + lower-and-drop
    // args[1..] so step()-style side-effect exprs fire per ES eval-then-
    // discard semantics. Mirrors check.rs S299.
    if let Type::Arr(arr_id) = recv_ty
        && method == "at"
    {
        let elem_ty = ctx.arr_layouts[arr_id.0 as usize];
        for &a in args.iter().skip(1) {
            let _ = ctx.lower_expr(a);
        }
        // ES §23.1.3.1 step 2 — ToIntegerOrInfinity on `index`. The
        // inline ICmp/Add/LoadDyn chain below is i64-only; without
        // this coerce, f64 args (`a.at(1.5)`) panic backend GPR
        // materialization. coerce_to_i64 const-folds NaN → 0 and
        // ±∞ → i64::{MAX,MIN} (spec), FpToSi otherwise.
        //
        // S225 — explicit `undefined` index lowers as a ConstPtrNull
        // which coerce_to_i64 cannot materialize (same shape as the
        // S222 charAt early-intercept). Short-circuit to ConstI64(0)
        // before coerce so the negative-wrap select picks arr[0].
        let i_val = if args.is_empty() {
            Operand::ConstI64(0)
        } else if matches!(
            ctx.expr_types.get(&args[0]),
            Some(crate::check::Type::Undefined)
        ) {
            Operand::ConstI64(0)
        } else {
            let raw = ctx.lower_expr(args[0]);
            ctx.coerce_to_i64(raw)
        };
        let len = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(Type::I64, recv_op, ARR_LEN_OFF),
            Type::I64,
            None,
        );
        let is_neg = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::ICmp(IPred::Slt, i_val, Operand::ConstI64(0)),
            Type::Bool,
            None,
        );
        let i_plus_len = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::BinOp(SsaBinOp::Add, i_val, Operand::Value(len)),
            Type::I64,
            None,
        );
        // adj = is_neg ? i + len : i (via select-shape:
        // alloca + cond_br).
        let adj_slot = ctx.alloca_in_entry(Type::I64, Some("__at_idx"));
        let neg_blk = ctx.f.add_block();
        let pos_blk = ctx.f.add_block();
        let after_blk = ctx.f.add_block();
        let cb = ctx.cur_block;
        ctx.f.set_term(
            cb,
            Terminator::CondBr {
                cond: Operand::Value(is_neg),
                then_blk: neg_blk,
                else_blk: pos_blk,
            },
        );
        ctx.f.append_void(
            neg_blk,
            InstKind::Store(Operand::Value(i_plus_len), Operand::Value(adj_slot), 0),
        );
        ctx.f.set_term(neg_blk, Terminator::Br(after_blk));
        ctx.f
            .append_void(pos_blk, InstKind::Store(i_val, Operand::Value(adj_slot), 0));
        ctx.f.set_term(pos_blk, Terminator::Br(after_blk));
        ctx.cur_block = after_blk;
        let adj = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(Type::I64, Operand::Value(adj_slot), 0),
            Type::I64,
            None,
        );
        // T-13.5 deque: head-aware offset for arr.at(i).
        let off = ctx.emit_arr_slot_byte_offset(recv_op.clone(), Operand::Value(adj), 3, false);
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::LoadDyn(elem_ty, recv_op, off),
            elem_ty,
            None,
        );
        return Some(Operand::Value(v));
    }
    None
}
