//! Both ends of a Latin-1 run scanned a byte at a time against the
//! Latin-1 projection of TrimString's set, looked up in the runtime's
//! 256-entry table. A step is two loads and one branch, so the integer
//! pipes, which the split loops a trim usually sits in keep full, pay
//! one instruction for each byte where a compare chain pays up to five
//! for the byte a walk stops on.

use super::{bin, cmp, cond_br, load, store};
use crate::ssa::{BinOp, BlockId, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::LowerCtx;

/// `__TORAJS_LATIN1_TRIM_WS` in torajs-str: `1` at TAB..CR, SP, NBSP.
const WS_TABLE_SYM: &str = "___TORAJS_LATIN1_TRIM_WS";

/// Both ends of the Latin-1 run `[base, base + len)` scanned from `at`:
/// the offset the leading walk stopped at, the end the trailing walk
/// stopped at (never below the first), and the open block where both
/// are known. The trailing walk goes first: when it stops above zero
/// the byte below its end is not whitespace, so the leading walk is
/// bound to stop at or before it and tests no bound; when it reaches
/// zero the run is all whitespace and both ends are zero.
pub(super) fn scan_ends(
    ctx: &mut LowerCtx<'_>,
    at: BlockId,
    base: ValueId,
    len: ValueId,
) -> (ValueId, ValueId, BlockId) {
    let v = Operand::Value;
    let lo_slot = ctx
        .f
        .append_inst(at, InstKind::Alloca(Type::I64), Type::Ptr, None);
    let hi_slot = ctx
        .f
        .append_inst(at, InstKind::Alloca(Type::I64), Type::Ptr, None);
    let hi_head = ctx.f.add_block();
    let hi_probe = ctx.f.add_block();
    let hi_next = ctx.f.add_block();
    let lo_pre = ctx.f.add_block();
    let lo_probe = ctx.f.add_block();
    let lo_next = ctx.f.add_block();
    let done = ctx.f.add_block();
    let table = ctx.f.append_inst(
        at,
        InstKind::GlobalRef(WS_TABLE_SYM.to_string()),
        Type::Ptr,
        None,
    );
    // the trailing probe reads `base - 1 + hi`, the byte below `hi`
    let b_int = ctx
        .f
        .append_inst(at, InstKind::PtrToInt(v(base)), Type::I64, None);
    let below_int = bin(ctx, at, BinOp::Sub, v(b_int), Operand::ConstI64(1));
    let below = ctx
        .f
        .append_inst(at, InstKind::IntToPtr(v(below_int)), Type::Ptr, None);
    store(ctx, at, v(len), hi_slot, 0);
    store(ctx, at, Operand::ConstI64(0), lo_slot, 0);
    ctx.f.set_term(at, Terminator::Br(hi_head));

    // trailing side: hi walks down to zero while the byte below it is
    // whitespace; hi never goes below zero, so the bound is a test
    // against zero
    let hi = load(ctx, hi_head, Type::I64, v(hi_slot), 0);
    let hi_in = cmp(ctx, hi_head, IPred::Ne, v(hi), Operand::ConstI64(0));
    cond_br(ctx, hi_head, hi_in, hi_probe, done);
    let ws = is_ws(ctx, hi_probe, table, below, hi);
    cond_br(ctx, hi_probe, ws, hi_next, lo_pre);
    let hi1 = bin(ctx, hi_next, BinOp::Sub, v(hi), Operand::ConstI64(1));
    store(ctx, hi_next, v(hi1), hi_slot, 0);
    ctx.f.set_term(hi_next, Terminator::Br(hi_head));

    // leading side: lo walks up while it sees whitespace, stopped at
    // the latest by the byte the trailing walk stopped on. Its start is
    // set in a block of its own: on the trailing walk's exit edge it
    // would be a copy behind one more jump.
    store(ctx, lo_pre, Operand::ConstI64(0), lo_slot, 0);
    ctx.f.set_term(lo_pre, Terminator::Br(lo_probe));
    let lo = load(ctx, lo_probe, Type::I64, v(lo_slot), 0);
    let ws = is_ws(ctx, lo_probe, table, base, lo);
    cond_br(ctx, lo_probe, ws, lo_next, done);
    let lo1 = bin(ctx, lo_next, BinOp::Add, v(lo), Operand::ConstI64(1));
    store(ctx, lo_next, v(lo1), lo_slot, 0);
    ctx.f.set_term(lo_next, Terminator::Br(lo_probe));

    let lo_end = load(ctx, done, Type::I64, v(lo_slot), 0);
    let hi_end = load(ctx, done, Type::I64, v(hi_slot), 0);
    (lo_end, hi_end, done)
}

/// Whether the byte at `ptr + off` is in the table.
fn is_ws(
    ctx: &mut LowerCtx<'_>,
    at: BlockId,
    table: ValueId,
    ptr: ValueId,
    off: ValueId,
) -> ValueId {
    let v = Operand::Value;
    let b = ctx
        .f
        .append_inst(at, InstKind::LoadU8Dyn(v(ptr), v(off)), Type::I64, None);
    let t = ctx
        .f
        .append_inst(at, InstKind::LoadU8Dyn(v(table), v(b)), Type::I64, None);
    cmp(ctx, at, IPred::Ne, v(t), Operand::ConstI64(0))
}
