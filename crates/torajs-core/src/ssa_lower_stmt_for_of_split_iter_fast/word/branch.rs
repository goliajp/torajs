//! The two control-flow shapes the word scan emits on every path.

use crate::ssa::{BlockId, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::LowerCtx;

pub(super) fn non_zero(ctx: &mut LowerCtx, blk: BlockId, x: ValueId) -> ValueId {
    ctx.f.append_inst(
        blk,
        InstKind::ICmp(IPred::Ne, Operand::Value(x), Operand::ConstI64(0)),
        Type::Bool,
        None,
    )
}

pub(super) fn cond_br(ctx: &mut LowerCtx, blk: BlockId, c: ValueId, t: BlockId, e: BlockId) {
    ctx.f.set_term(
        blk,
        Terminator::CondBr {
            cond: Operand::Value(c),
            then_blk: t,
            else_blk: e,
        },
    );
}
