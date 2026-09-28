//! The `Any` lhs lane of `??` (`crate::ssa_lower_nullish` layer 1):
//! the box's tag decides, rhs lowers only on the nullish branch, and
//! the non-nullish value unboxes to rhs's SSA type. Split from the
//! nullish lowering at the file-size line.

use crate::ast::ExprId;
use crate::ssa::{BinOp as SsaBinOp, IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;

/// Chunk 723 — rebuilt on the Ternary join shape: the tag test
/// branches FIRST and rhs lowers inside the nullish branch, per ES
/// §13.4.2 (rhs is unevaluated when lhs is neither null nor
/// undefined — the old pre-branch eval ran `a ?? loud()`'s side
/// effect unconditionally). The join result is uniformly owned
/// when refcounted (borrow rhs takes a tail inc; the unbox arms
/// already detach with +1), and an owned lhs box releases in the
/// after block: the nullish path holds a null/undef immediate
/// whose drop no-ops, the unbox path detached its payload first
/// (probe p723a/b: `mkAny(i) ?? "z"` churn leaked the Call's box
/// on every path pre-fix).
pub(crate) fn lower_any_lhs(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    lhs: ExprId,
    lhs_op: Operand,
    rhs: ExprId,
) -> Operand {
    let cur_block = ctx.cur_block;
    let tag = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.any_unbox_tag, vec![lhs_op.clone()]),
        Type::I64,
        None,
    );
    let nullish = emit_nullish_check(ctx, tag);
    let rhs_blk = ctx.f.add_block();
    let unbox_blk = ctx.f.add_block();
    let after = ctx.f.add_block();
    let cb = ctx.cur_block;
    ctx.f.set_term(
        cb,
        Terminator::CondBr {
            cond: Operand::Value(nullish),
            then_blk: rhs_blk,
            else_blk: unbox_blk,
        },
    );
    ctx.cur_block = rhs_blk;
    let rhs_raw = ctx.lower_expr(rhs);
    let rhs_raw_ty = ctx.operand_ty(&rhs_raw);
    // Rotation 362 — the join is ANY (mirrors check_nullish): a
    // non-nullish lhs IS the result, whatever its tag holds, so the
    // old unbox-to-rhs-width read an f64 box's raw bits as an i64
    // (probe: `argsAnyArr[0] ?? nParam` printed 4611686018427387904)
    // and squeezed a non-bool payload through an ICmp. Box the rhs
    // side instead; a borrow-shape rhs takes +1 before the box (the
    // chunk-722 transfer contract, same as the ternary widen).
    let rhs_op = if rhs_raw_ty == Type::Any {
        rhs_raw
    } else {
        if rhs_raw_ty.is_refcounted() && !ctx.expr_transfers_ownership(rhs) {
            ctx.emit_rc_inc(rhs_raw.clone());
        }
        ctx.box_to_any_from_expr(rhs, rhs_raw)
    };
    let rhs_ty = Type::Any;
    let rhs_end = ctx.cur_block;
    if rhs_raw_ty == Type::Any && !ctx.expr_transfers_ownership(rhs) {
        ctx.emit_owned_result_inc_in(rhs_end, rhs_op.clone(), rhs_ty);
    }
    ctx.cur_block = unbox_blk;
    let unboxed = unbox_lhs_to_rhs_ty(ctx, lhs_op.clone(), rhs_ty);
    let unbox_end = ctx.cur_block;
    let res_slot = ctx.alloca_in_entry(rhs_ty, Some("__nullish_any"));
    ctx.f.append_void(
        rhs_end,
        InstKind::Store(rhs_op, Operand::Value(res_slot), 0),
    );
    ctx.f.set_term(rhs_end, Terminator::Br(after));
    ctx.f.append_void(
        unbox_end,
        InstKind::Store(unboxed, Operand::Value(res_slot), 0),
    );
    ctx.f.set_term(unbox_end, Terminator::Br(after));
    ctx.cur_block = after;
    if ctx.expr_owned_shape(lhs) {
        ctx.emit_drop_value(lhs_op, Type::Any);
    }
    if rhs_ty.is_refcounted() {
        ctx.owned_member_reads.insert(eid);
    }
    let cur_block = ctx.cur_block;
    let r = ctx.f.append_inst(
        cur_block,
        InstKind::Load(rhs_ty, Operand::Value(res_slot), 0),
        rhs_ty,
        None,
    );
    Operand::Value(r)
}

fn emit_nullish_check(ctx: &mut LowerCtx<'_>, tag: crate::ssa::ValueId) -> crate::ssa::ValueId {
    let cur_block = ctx.cur_block;
    let is_null = ctx.f.append_inst(
        cur_block,
        InstKind::ICmp(IPred::Eq, Operand::Value(tag), Operand::ConstI64(0)),
        Type::Bool,
        None,
    );
    let is_undef = ctx.f.append_inst(
        cur_block,
        InstKind::ICmp(IPred::Eq, Operand::Value(tag), Operand::ConstI64(5)),
        Type::Bool,
        None,
    );
    ctx.f.append_inst(
        cur_block,
        InstKind::BinOp(
            SsaBinOp::Or,
            Operand::Value(is_null),
            Operand::Value(is_undef),
        ),
        Type::Bool,
        None,
    )
}

fn unbox_lhs_to_rhs_ty(ctx: &mut LowerCtx<'_>, lhs_op: Operand, rhs_ty: Type) -> Operand {
    if matches!(rhs_ty, Type::Any) {
        ctx.emit_rc_inc(lhs_op.clone());
        return lhs_op;
    }
    if rhs_ty.is_refcounted() {
        // Owned unbox: a cell gains the consumer's +1 inside the
        // kernel, and a ShortStr materialization arrives rc=1 as the
        // stake itself — the old `unbox_value` + `rc_inc` pair
        // double-staked the materialization (546-02 M2 shape).
        let cur_block = ctx.cur_block;
        let val = ctx.f.append_inst(
            cur_block,
            InstKind::Call(ctx.intrinsics.any_unbox_value_owned, vec![lhs_op]),
            Type::I64,
            None,
        );
        return Operand::Value(val);
    }
    let cur_block = ctx.cur_block;
    let val = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.any_unbox_value, vec![lhs_op.clone()]),
        Type::I64,
        None,
    );
    // The scalar arms below never consume a Heap payload — reclaim a
    // ShortStr-materialized temp before the raw bits are reshaped
    // (no-op otherwise; `val` is an SSA value, not a memory read).
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.any_unbox_settle,
            vec![lhs_op, Operand::Value(val)],
        ),
    );
    match rhs_ty {
        Type::I64 | Type::I32 => Operand::Value(val),
        Type::F64 => {
            let cur_block = ctx.cur_block;
            let f = ctx.f.append_inst(
                cur_block,
                InstKind::BitCastI64ToF64(Operand::Value(val)),
                Type::F64,
                None,
            );
            Operand::Value(f)
        }
        Type::Bool => {
            let cur_block = ctx.cur_block;
            let b = ctx.f.append_inst(
                cur_block,
                InstKind::ICmp(IPred::Ne, Operand::Value(val), Operand::ConstI64(0)),
                Type::Bool,
                None,
            );
            Operand::Value(b)
        }
        _ => Operand::Value(val),
    }
}
