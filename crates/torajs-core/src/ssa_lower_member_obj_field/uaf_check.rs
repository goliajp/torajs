//! The read-side half of the `--cfg torajs_uaf_detect` build: before a
//! typed field load, a cell whose class-tag slot holds the allocator's
//! dead mark traps. The allocator half keeps freed blocks out of reuse,
//! so a stale receiver or argument stops at its first field read
//! instead of reading whichever object took over the address.

use crate::ssa::{IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::{LowerCtx, OBJ_CLASS_TAG_OFF};
use torajs_mmalloc::tlab::UAF_POISON;

pub(crate) fn emit(ctx: &mut LowerCtx<'_>, cell: Operand) {
    let tag = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, cell, OBJ_CLASS_TAG_OFF),
        Type::I64,
        None,
    );
    let dead = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(
            IPred::Eq,
            Operand::Value(tag),
            Operand::ConstI64(UAF_POISON as i64),
        ),
        Type::Bool,
        None,
    );
    // a fresh block ends in `unreachable`, which codegen emits as `brk`
    let trap = ctx.f.add_block();
    let live = ctx.f.add_block();
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(dead),
            then_blk: trap,
            else_blk: live,
        },
    );
    ctx.cur_block = live;
}
