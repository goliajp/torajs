//! The `Any` lhs lane of `??` (`crate::ssa_lower_nullish` layer 1):
//! the box's tag decides, rhs lowers only on the nullish branch, and
//! the non-nullish value unboxes to rhs's SSA type. Split from the
//! nullish lowering at the file-size line.

use crate::ast::{Expr, ExprId};
use crate::ssa::{BinOp as SsaBinOp, BlockId, IPred, InstKind, Operand, Terminator, Type};
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
        InstKind::Call(ctx.intrinsics.any_unbox_tag, vec![lhs_op]),
        Type::I64,
        None,
    );
    let nullish = nullish_tag(ctx, tag);
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
    let rhs_op = owned_rhs(ctx, rhs);
    let rhs_ty = Type::Any;
    let rhs_end = ctx.cur_block;
    ctx.cur_block = unbox_blk;
    let unboxed = unbox_lhs_to_rhs_ty(ctx, lhs_op, rhs_ty);
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

/// The nullish arm's value: rhs, lowered in the current block, as an
/// owned `any`.
fn owned_rhs(ctx: &mut LowerCtx<'_>, rhs: ExprId) -> Operand {
    ctx.proofs.enter_cond();
    let rhs_raw = ctx.lower_expr(rhs);
    ctx.proofs.leave_cond();
    // Rotation 362 — the join is ANY (mirrors check_nullish): a
    // non-nullish lhs IS the result, whatever its tag holds, so the
    // old unbox-to-rhs-width read an f64 box's raw bits as an i64
    // (probe: `argsAnyArr[0] ?? nParam` printed 4611686018427387904)
    // and squeezed a non-bool payload through an ICmp. Box the rhs
    // side instead; a borrow-shape rhs takes +1 before the box (the
    // chunk-722 transfer contract, same as the ternary widen).
    if ctx.operand_ty(&rhs_raw) != Type::Any {
        let is_borrow = !ctx.expr_transfers_ownership(rhs);
        return ctx.box_owned_from_expr(rhs, rhs_raw, is_borrow);
    }
    if !ctx.expr_transfers_ownership(rhs) {
        ctx.emit_owned_result_inc(rhs_raw, Type::Any);
    }
    rhs_raw
}

/// Ask the member read `lhs` of `lhs ?? rhs` to answer the nullish test
/// itself (`shadow_read::boxed_load`): a read through a binding, whose
/// receiver no later step releases, so a nullish arm that leaves the
/// read early skips nothing. Whether it did comes from [`split_taken`].
pub(crate) fn split(ctx: &mut LowerCtx<'_>, lhs: ExprId, rhs: ExprId) -> bool {
    let ast = ctx.ast;
    let asked = matches!(ast.get_expr(lhs), Expr::Member { obj, .. }
            if matches!(ast.get_expr(*obj), Expr::Ident(_)))
        && !matches!(ast.get_expr(rhs), Expr::Assign { .. })
        && ctx.binop.nullish_split.is_none();
    if asked {
        ctx.binop.nullish_split = Some((lhs, None));
    }
    asked
}

/// The block the read left open for a nullish lhs, when it took the
/// request [`split`] made.
pub(crate) fn split_taken(ctx: &mut LowerCtx<'_>, asked: bool) -> Option<BlockId> {
    if !asked {
        return None;
    }
    ctx.binop.nullish_split.take().and_then(|(_, b)| b)
}

/// `lhs ?? rhs` whose lhs read answered the nullish test: `lhs_op`, at
/// the current block, is the owned non-nullish value and is the result
/// as it is; rhs lowers in `nullish_blk`.
pub(crate) fn join_split(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    lhs_op: Operand,
    nullish_blk: BlockId,
    rhs: ExprId,
) -> Operand {
    let res_slot = ctx.alloca_in_entry(Type::Any, Some("__nullish_any"));
    let after = ctx.f.add_block();
    let blk = ctx.cur_block;
    ctx.f
        .append_void(blk, InstKind::Store(lhs_op, Operand::Value(res_slot), 0));
    ctx.f.set_term(blk, Terminator::Br(after));
    ctx.cur_block = nullish_blk;
    let rhs_op = owned_rhs(ctx, rhs);
    let blk = ctx.cur_block;
    ctx.f
        .append_void(blk, InstKind::Store(rhs_op, Operand::Value(res_slot), 0));
    ctx.f.set_term(blk, Terminator::Br(after));
    ctx.cur_block = after;
    ctx.owned_member_reads.insert(eid);
    let r = ctx.f.append_inst(
        after,
        InstKind::Load(Type::Any, Operand::Value(res_slot), 0),
        Type::Any,
        None,
    );
    Operand::Value(r)
}

pub(crate) fn nullish_tag(ctx: &mut LowerCtx<'_>, tag: crate::ssa::ValueId) -> crate::ssa::ValueId {
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
        ctx.emit_rc_inc(lhs_op);
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
        InstKind::Call(ctx.intrinsics.any_unbox_value, vec![lhs_op]),
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
