//! Both ends of a Latin-1 run scanned a byte at a time against the
//! Latin-1 projection of TrimString's set.

use super::{bin, cmp, cond_br, load, store};
use crate::ssa::{BinOp, BlockId, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::LowerCtx;

/// Both ends of the Latin-1 run `[base, base + len)` scanned from `at`:
/// the offset the leading walk stopped at, the end the trailing walk
/// stopped at (never below the first), and the open block where both
/// are known.
pub(super) fn scan_ends(
    ctx: &mut LowerCtx<'_>,
    at: BlockId,
    base: ValueId,
    len: ValueId,
) -> (ValueId, ValueId, BlockId) {
    let lo_slot = ctx
        .f
        .append_inst(at, InstKind::Alloca(Type::I64), Type::Ptr, None);
    let hi_slot = ctx
        .f
        .append_inst(at, InstKind::Alloca(Type::I64), Type::Ptr, None);
    let lo_head = ctx.f.add_block();
    let lo_probe = ctx.f.add_block();
    let lo_next = ctx.f.add_block();
    let hi_init = ctx.f.add_block();
    let hi_head = ctx.f.add_block();
    let hi_probe = ctx.f.add_block();
    let hi_next = ctx.f.add_block();
    let done = ctx.f.add_block();
    store(ctx, at, Operand::ConstI64(0), lo_slot, 0);
    ctx.f.set_term(at, Terminator::Br(lo_head));

    // leading side: lo walks up while it sees whitespace
    let lo = load(ctx, lo_head, Type::I64, Operand::Value(lo_slot), 0);
    let lo_in = cmp(
        ctx,
        lo_head,
        IPred::Slt,
        Operand::Value(lo),
        Operand::Value(len),
    );
    cond_br(ctx, lo_head, lo_in, lo_probe, hi_init);
    let b = ctx.f.append_inst(
        lo_probe,
        InstKind::LoadU8Dyn(Operand::Value(base), Operand::Value(lo)),
        Type::I64,
        None,
    );
    branch_on_ws(ctx, lo_probe, b, lo_next, hi_init);
    let lo1 = bin(
        ctx,
        lo_next,
        BinOp::Add,
        Operand::Value(lo),
        Operand::ConstI64(1),
    );
    store(ctx, lo_next, Operand::Value(lo1), lo_slot, 0);
    ctx.f.set_term(lo_next, Terminator::Br(lo_head));

    // trailing side: hi walks down to lo
    let lo_end = load(ctx, hi_init, Type::I64, Operand::Value(lo_slot), 0);
    store(ctx, hi_init, Operand::Value(len), hi_slot, 0);
    ctx.f.set_term(hi_init, Terminator::Br(hi_head));
    let hi = load(ctx, hi_head, Type::I64, Operand::Value(hi_slot), 0);
    let hi_in = cmp(
        ctx,
        hi_head,
        IPred::Sgt,
        Operand::Value(hi),
        Operand::Value(lo_end),
    );
    cond_br(ctx, hi_head, hi_in, hi_probe, done);
    let last = bin(
        ctx,
        hi_probe,
        BinOp::Sub,
        Operand::Value(hi),
        Operand::ConstI64(1),
    );
    let b = ctx.f.append_inst(
        hi_probe,
        InstKind::LoadU8Dyn(Operand::Value(base), Operand::Value(last)),
        Type::I64,
        None,
    );
    branch_on_ws(ctx, hi_probe, b, hi_next, done);
    let hi1 = bin(
        ctx,
        hi_next,
        BinOp::Sub,
        Operand::Value(hi),
        Operand::ConstI64(1),
    );
    store(ctx, hi_next, Operand::Value(hi1), hi_slot, 0);
    ctx.f.set_term(hi_next, Terminator::Br(hi_head));

    let hi_end = load(ctx, done, Type::I64, Operand::Value(hi_slot), 0);
    (lo_end, hi_end, done)
}

/// `b` in TAB..CR, SP or NBSP goes to `ws`, anything else to `other`.
/// SP is asked first, then 0x21..0x9F, which holds no member and every
/// printable ASCII byte, so the two bytes a field usually ends on are
/// each settled by one comparison; TAB..CR and NBSP are asked last.
fn branch_on_ws(ctx: &mut LowerCtx<'_>, at: BlockId, b: ValueId, ws: BlockId, other: BlockId) {
    let not_sp = ctx.f.add_block();
    let rare = ctx.f.add_block();
    let not_ctl = ctx.f.add_block();
    let sp = cmp(
        ctx,
        at,
        IPred::Eq,
        Operand::Value(b),
        Operand::ConstI64(i64::from(b' ')),
    );
    cond_br(ctx, at, sp, ws, not_sp);
    let above_sp = bin(
        ctx,
        not_sp,
        BinOp::Sub,
        Operand::Value(b),
        Operand::ConstI64(0x21),
    );
    let plain = cmp(
        ctx,
        not_sp,
        IPred::Ult,
        Operand::Value(above_sp),
        Operand::ConstI64(0xa0 - 0x21),
    );
    cond_br(ctx, not_sp, plain, other, rare);
    let rel = bin(
        ctx,
        rare,
        BinOp::Sub,
        Operand::Value(b),
        Operand::ConstI64(i64::from(b'\t')),
    );
    let ctl = cmp(
        ctx,
        rare,
        IPred::Ult,
        Operand::Value(rel),
        Operand::ConstI64(i64::from(b'\r' - b'\t' + 1)),
    );
    cond_br(ctx, rare, ctl, ws, not_ctl);
    let nbsp = cmp(
        ctx,
        not_ctl,
        IPred::Eq,
        Operand::Value(b),
        Operand::ConstI64(0xa0),
    );
    cond_br(ctx, not_ctl, nbsp, ws, other);
}
