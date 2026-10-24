//! The instruction builders the catch binding's tests are spelled in.

use crate::ssa::{BlockId, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::LowerCtx;

/// Continue in a fresh block when `cond` holds, branch to `bad` when
/// it does not.
pub(super) fn guard(ctx: &mut LowerCtx, cond: ValueId, bad: BlockId) {
    let next = ctx.f.add_block();
    let cb = ctx.cur_block;
    ctx.f.set_term(
        cb,
        Terminator::CondBr {
            cond: Operand::Value(cond),
            then_blk: next,
            else_blk: bad,
        },
    );
    ctx.cur_block = next;
}

pub(super) fn call(ctx: &mut LowerCtx, f: crate::ssa::FuncId, name: Option<&str>) -> ValueId {
    let cb = ctx.cur_block;
    ctx.f
        .append_inst(cb, InstKind::Call(f, vec![]), Type::I64, name)
}

pub(super) fn append(ctx: &mut LowerCtx, inst: InstKind, ty: Type) -> ValueId {
    let cb = ctx.cur_block;
    ctx.f.append_inst(cb, inst, ty, None)
}

pub(super) fn icmp(ctx: &mut LowerCtx, pred: IPred, v: ValueId, k: i64) -> ValueId {
    append(
        ctx,
        InstKind::ICmp(pred, Operand::Value(v), Operand::ConstI64(k)),
        Type::Bool,
    )
}

pub(super) fn or(ctx: &mut LowerCtx, a: ValueId, b: ValueId) -> ValueId {
    let op = crate::ssa::BinOp::Or;
    append(
        ctx,
        InstKind::BinOp(op, Operand::Value(a), Operand::Value(b)),
        Type::Bool,
    )
}

pub(super) fn and(ctx: &mut LowerCtx, a: ValueId, b: ValueId) -> ValueId {
    let op = crate::ssa::BinOp::And;
    append(
        ctx,
        InstKind::BinOp(op, Operand::Value(a), Operand::Value(b)),
        Type::Bool,
    )
}
