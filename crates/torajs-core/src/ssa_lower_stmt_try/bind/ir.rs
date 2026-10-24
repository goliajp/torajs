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

/// The f64 a number slot binds: the bits of an F64 tag, the widened
/// value of an I64 one.
pub(super) fn decode_f64(ctx: &mut LowerCtx, tag: ValueId, v: ValueId) -> ValueId {
    let out = ctx.alloca(Type::F64, None);
    let (bits_blk, int_blk, join) = (ctx.f.add_block(), ctx.f.add_block(), ctx.f.add_block());
    let is_f64 = icmp(ctx, IPred::Eq, tag, super::TAG_F64);
    let cb = ctx.cur_block;
    ctx.f.set_term(
        cb,
        Terminator::CondBr {
            cond: Operand::Value(is_f64),
            then_blk: bits_blk,
            else_blk: int_blk,
        },
    );
    for (blk, inst) in [
        (bits_blk, InstKind::BitCastI64ToF64(Operand::Value(v))),
        (int_blk, InstKind::SiToFp(Operand::Value(v))),
    ] {
        ctx.cur_block = blk;
        let f = append(ctx, inst, Type::F64);
        ctx.f.append_void(
            blk,
            InstKind::Store(Operand::Value(f), Operand::Value(out), 0),
        );
        ctx.f.set_term(blk, Terminator::Br(join));
    }
    ctx.cur_block = join;
    append(
        ctx,
        InstKind::Load(Type::F64, Operand::Value(out), 0),
        Type::F64,
    )
}
