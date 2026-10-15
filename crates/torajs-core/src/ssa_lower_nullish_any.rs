//! The `Any` lhs lane of `??` (`crate::ssa_lower_nullish` layer 1):
//! the box's bits decide, rhs lowers only on the nullish branch, and
//! the non-nullish value is the result as it is. Split from the
//! nullish lowering at the file-size line.

use crate::ast::{Expr, ExprId};
use crate::ssa::{BinOp as SsaBinOp, BlockId, IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;

/// Chunk 723 — rebuilt on the Ternary join shape: the tag test
/// branches FIRST and rhs lowers inside the nullish branch, per ES
/// §13.4.2 (rhs is unevaluated when lhs is neither null nor
/// undefined — the old pre-branch eval ran `a ?? loud()`'s side
/// effect unconditionally). The join result is uniformly owned: a
/// borrowed rhs or lhs takes a stake on its arm, and an owned lhs
/// hands its own over, since the nullish arm holds a null / undefined
/// immediate that owns nothing (probe p723a/b: `mkAny(i) ?? "z"`
/// churn leaked the Call's box on every path before the owned lhs
/// was accounted for).
pub(crate) fn lower_any_lhs(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    lhs: ExprId,
    lhs_op: Operand,
    rhs: ExprId,
) -> Operand {
    let rhs_blk = ctx.f.add_block();
    let unbox_blk = ctx.f.add_block();
    let after = ctx.f.add_block();
    branch_nullish(ctx, &lhs_op, rhs_blk, unbox_blk);
    ctx.cur_block = rhs_blk;
    let rhs_op = owned_rhs(ctx, rhs);
    let rhs_end = ctx.cur_block;
    // a non-nullish lhs is the result: an owned one hands its stake
    // over, a borrowed one takes a stake of its own; a nullish one is
    // an immediate that owns nothing
    ctx.cur_block = unbox_blk;
    if !ctx.expr_owned_shape(lhs) {
        ctx.emit_rc_inc(lhs_op);
    }
    let unbox_end = ctx.cur_block;
    let res_slot = ctx.alloca_in_entry(Type::Any, Some("__nullish_any"));
    ctx.f.append_void(
        rhs_end,
        InstKind::Store(rhs_op, Operand::Value(res_slot), 0),
    );
    ctx.f.set_term(rhs_end, Terminator::Br(after));
    ctx.f.append_void(
        unbox_end,
        InstKind::Store(lhs_op, Operand::Value(res_slot), 0),
    );
    ctx.f.set_term(unbox_end, Terminator::Br(after));
    ctx.cur_block = after;
    ctx.owned_member_reads.insert(eid);
    let cur_block = ctx.cur_block;
    let r = ctx.f.append_inst(
        cur_block,
        InstKind::Load(Type::Any, Operand::Value(res_slot), 0),
        Type::Any,
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

/// Branch on whether the NaN-box bits of `v` are null or undefined —
/// the answer `__torajs_anyv_unbox_tag` gives as Null / Undef for every
/// 64-bit pattern — to `nullish` or `other`, terminating the current
/// block. A pattern with any of the top 16 bits set (a number, a
/// ShortStr) is decided by that one test; below it, the nullish ones
/// are those that are neither a cell (TAG_BIT_TYPE_OTHER clear,
/// non-zero) nor a bool (`v | 1 == true`): null 0x02, undefined 0x0A
/// and the array hole 0.
fn branch_nullish(ctx: &mut LowerCtx<'_>, v: &Operand, nullish: BlockId, other: BlockId) {
    use crate::ssa_lower_anyv_inline::{bin, branch, cmp_zero, emit_bits, inst};
    use torajs_anyvalue::nanbox::{TAG_BIT_TYPE_OTHER, TOP_16_MASK, VALUE_TRUE};
    let bits = emit_bits(ctx, v);
    let top = bin(SsaBinOp::And, bits, Operand::ConstI64(TOP_16_MASK as i64));
    let top = inst(ctx, top, Type::I64);
    let top_set = inst(ctx, cmp_zero(IPred::Ne, top), Type::Bool);
    let low_blk = ctx.f.add_block();
    branch(ctx, top_set, other, low_blk);

    ctx.cur_block = low_blk;
    let tag_bit = Operand::ConstI64(TAG_BIT_TYPE_OTHER as i64);
    let tagged = inst(ctx, bin(SsaBinOp::And, bits, tag_bit), Type::I64);
    let tagged = inst(ctx, cmp_zero(IPred::Ne, tagged), Type::Bool);
    let zero = inst(ctx, cmp_zero(IPred::Eq, bits), Type::Bool);
    let not_cell = InstKind::BinOp(SsaBinOp::Or, Operand::Value(tagged), Operand::Value(zero));
    let not_cell = inst(ctx, not_cell, Type::Bool);
    let low_set = inst(
        ctx,
        bin(SsaBinOp::Or, bits, Operand::ConstI64(1)),
        Type::I64,
    );
    let true_bits = Operand::ConstI64(VALUE_TRUE as i64);
    let not_bool = InstKind::ICmp(IPred::Ne, Operand::Value(low_set), true_bits);
    let not_bool = inst(ctx, not_bool, Type::Bool);
    let both = InstKind::BinOp(
        SsaBinOp::And,
        Operand::Value(not_cell),
        Operand::Value(not_bool),
    );
    let is_nullish = inst(ctx, both, Type::Bool);
    branch(ctx, is_nullish, nullish, other);
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
