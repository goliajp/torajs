//! S7-r2 fast-arm machinery for [`crate::ssa_lower_stmt_for_of_split_iter`]
//! — the hoisted-invariant header dispatch and the inline Latin-1
//! byte-scan `next`. Split out as a sibling when the versioned-loop
//! emission pushed the parent past the 500-line file cap; the
//! parent answers "what loop shape does ForOfSplitIter lower to",
//! this file answers "what does one fast-arm iteration emit".
//! Rationale for the versioning itself lives in the parent's
//! module doc (## S7-r2 section).

use crate::ssa::{BinOp as SsaBinOp, BlockId, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_stmt_for_of_split_iter::SplitView;

/// `flags u16 @6` of the packed Str header u64 — mirrored here as
/// header-word masks (`flag << 48`). Values are wire-format
/// (`torajs-str/src/layout.rs` STR_FLAG_IS_LATIN1 = 0x0002,
/// `torajs-rc/src/flags.rs` FLAG_STATIC_LITERAL = 0x0004).
const HDR_MASK_IS_LATIN1: i64 = 0x0002i64 << 48;
const HDR_STATIC_LITERAL: i64 = 0x0004i64 << 48;
/// Str payload starts at byte 16 (`STR_DATA_OFF`).
const STR_DATA_OFF: i64 = 16;

/// The loop shape [`emit_versioned_header`] wires into: the lowered
/// operands, the iter / Substr slots, and the pre-created blocks.
pub(crate) struct SplitLoop {
    pub(crate) parent_op: Operand,
    pub(crate) sep_op: Operand,
    pub(crate) iter_slot: ValueId,
    pub(crate) sub_slot: ValueId,
    pub(crate) v_slot: ValueId,
    /// Whether the fast arm must write each part into `sub_slot`;
    /// false when the body only reads it through the view.
    pub(crate) fill_sub: bool,
    pub(crate) target: u8,
    pub(crate) header: BlockId,
    pub(crate) body_blk: BlockId,
    pub(crate) after: BlockId,
}

/// Emit the S7-r2 versioned loop machinery: hoisted encoding test,
/// the iter init on the slow side only, per-iteration dispatch in
/// `header`, the inline byte-scan `next` on the fast arm, the
/// `split_iter_next` call on the slow arm. Both arms yield into
/// `sub_slot` (the fast arm only when `fill_sub`) and both record the
/// part's length and offset in the view slots, then converge on
/// `body_blk` / `after`. On return `ctx.cur_block` is unspecified —
/// the caller repositions to `body_blk`.
pub(crate) fn emit_versioned_header(ctx: &mut LowerCtx, sl: &SplitLoop) -> SplitView {
    let SplitLoop {
        parent_op,
        sep_op,
        iter_slot,
        sub_slot,
        v_slot,
        fill_sub,
        target,
        header,
        body_blk,
        after,
    } = sl;
    let (iter_slot, sub_slot, v_slot, fill_sub, target) =
        (*iter_slot, *sub_slot, *v_slot, *fill_sub, *target);
    let (header, body_blk, after) = (*header, *body_blk, *after);
    let cur = ctx.cur_block;
    // Hoisted per-loop invariants (entry side, before the header).
    let hdr = ctx.f.append_inst(
        cur,
        InstKind::Load(Type::I64, *parent_op, 0),
        Type::I64,
        None,
    );
    let flag = ctx.f.append_inst(
        cur,
        InstKind::BinOp(
            SsaBinOp::And,
            Operand::Value(hdr),
            Operand::ConstI64(HDR_MASK_IS_LATIN1),
        ),
        Type::I64,
        None,
    );
    let is_lat = ctx.f.append_inst(
        cur,
        InstKind::ICmp(IPred::Ne, Operand::Value(flag), Operand::ConstI64(0)),
        Type::Bool,
        None,
    );
    // `length u32 @8` of the Str header, the code-unit count.
    let plen32 = ctx.f.append_inst(
        cur,
        InstKind::Load(Type::I32, *parent_op, 8),
        Type::I32,
        None,
    );
    let plen = ctx.f.append_inst(
        cur,
        InstKind::ZExtI32ToI64(Operand::Value(plen32)),
        Type::I64,
        None,
    );
    let p_int = ctx
        .f
        .append_inst(cur, InstKind::PtrToInt(*parent_op), Type::I64, None);
    let pbase_i = ctx.f.append_inst(
        cur,
        InstKind::BinOp(
            SsaBinOp::Add,
            Operand::Value(p_int),
            Operand::ConstI64(STR_DATA_OFF),
        ),
        Type::I64,
        None,
    );
    let pbase = ctx.f.append_inst(
        cur,
        InstKind::IntToPtr(Operand::Value(pbase_i)),
        Type::Ptr,
        None,
    );
    // Fast-arm loop state (mem2reg-liftable scalar slots).
    let pos_slot = ctx
        .f
        .append_inst(cur, InstKind::Alloca(Type::I64), Type::Ptr, None);
    let done_slot = ctx
        .f
        .append_inst(cur, InstKind::Alloca(Type::Bool), Type::Ptr, None);
    let k_slot = ctx
        .f
        .append_inst(cur, InstKind::Alloca(Type::I64), Type::Ptr, None);
    let len_slot = ctx
        .f
        .append_inst(cur, InstKind::Alloca(Type::I64), Type::Ptr, None);
    let off_slot = ctx
        .f
        .append_inst(cur, InstKind::Alloca(Type::I64), Type::Ptr, None);
    ctx.f.append_void(
        cur,
        InstKind::Store(Operand::ConstI64(0), Operand::Value(pos_slot), 0),
    );
    ctx.f.append_void(
        cur,
        InstKind::Store(Operand::ConstBool(false), Operand::Value(done_slot), 0),
    );
    // Only the slow arm reads the iter struct.
    let slow_init = ctx.f.add_block();
    ctx.f.set_term(
        cur,
        Terminator::CondBr {
            cond: Operand::Value(is_lat),
            then_blk: header,
            else_blk: slow_init,
        },
    );
    ctx.f.append_void(
        slow_init,
        InstKind::Call(
            ctx.intrinsics.split_iter_init,
            vec![Operand::Value(iter_slot), *parent_op, *sep_op],
        ),
    );
    ctx.f.set_term(slow_init, Terminator::Br(header));

    let fast_head = ctx.f.add_block();
    let slow_head = ctx.f.add_block();
    ctx.f.set_term(
        header,
        Terminator::CondBr {
            cond: Operand::Value(is_lat),
            then_blk: fast_head,
            else_blk: slow_head,
        },
    );

    // Slow arm — the pre-existing kernel call.
    let ok = ctx.f.append_inst(
        slow_head,
        InstKind::Call(
            ctx.intrinsics.split_iter_next,
            vec![Operand::Value(iter_slot), Operand::Value(sub_slot)],
        ),
        Type::Bool,
        None,
    );
    let slow_yield = ctx.f.add_block();
    ctx.f.set_term(
        slow_head,
        Terminator::CondBr {
            cond: Operand::Value(ok),
            then_blk: slow_yield,
            else_blk: after,
        },
    );
    for (off, slot) in [(8, len_slot), (24, off_slot)] {
        let v = ctx.f.append_inst(
            slow_yield,
            InstKind::Load(Type::I64, Operand::Value(sub_slot), off),
            Type::I64,
            None,
        );
        ctx.f.append_void(
            slow_yield,
            InstKind::Store(Operand::Value(v), Operand::Value(slot), 0),
        );
    }
    ctx.f.set_term(slow_yield, Terminator::Br(body_blk));
    emit_fast_next(
        ctx,
        &FastScan {
            parent_op: *parent_op,
            sub_slot,
            target,
            plen,
            pbase,
            pos_slot,
            done_slot,
            k_slot,
            len_slot,
            off_slot,
            fill_sub,
            fast_head,
            body_blk,
            after,
        },
    );
    SplitView {
        v_slot,
        len_slot,
        off_slot,
        pbase,
        is_lat,
    }
}

/// Everything the fast-arm inline `next` needs — the hoisted loop
/// invariants and the pre-created blocks it wires into.
struct FastScan {
    parent_op: Operand,
    sub_slot: ValueId,
    target: u8,
    /// Hoisted `parent_len` (loaded once from iter+8).
    plen: ValueId,
    /// Hoisted `parent + STR_DATA_OFF` payload base.
    pbase: ValueId,
    pos_slot: ValueId,
    done_slot: ValueId,
    k_slot: ValueId,
    len_slot: ValueId,
    off_slot: ValueId,
    fill_sub: bool,
    fast_head: BlockId,
    body_blk: BlockId,
    after: BlockId,
}

/// Emit the fast-arm inline `next` (the S7-r2 byte scan): exhausted
/// check, separator scan, Substr yield into `sub_slot`, and the
/// advance/exhaust bookkeeping — the register-resident mirror of the
/// kernel's `sep_len == 1` Latin-1 branch.
fn emit_fast_next(ctx: &mut LowerCtx, fs: &FastScan) {
    let FastScan {
        parent_op,
        sub_slot,
        target,
        plen,
        pbase,
        pos_slot,
        done_slot,
        k_slot,
        len_slot,
        off_slot,
        fill_sub,
        fast_head,
        body_blk,
        after,
    } = fs;
    let (sub_slot, target, plen, pbase, pos_slot, done_slot, k_slot) = (
        *sub_slot, *target, *plen, *pbase, *pos_slot, *done_slot, *k_slot,
    );
    let (len_slot, off_slot, fill_sub) = (*len_slot, *off_slot, *fill_sub);
    let (fast_head, body_blk, after) = (*fast_head, *body_blk, *after);
    // Fast arm — inline `next`.
    let scan_init = ctx.f.add_block();
    let scan_head = ctx.f.add_block();
    let scan_probe = ctx.f.add_block();
    let scan_next = ctx.f.add_block();
    let scan_emit = ctx.f.add_block();
    let mark_done = ctx.f.add_block();
    let advance = ctx.f.add_block();

    let done = ctx.f.append_inst(
        fast_head,
        InstKind::Load(Type::Bool, Operand::Value(done_slot), 0),
        Type::Bool,
        None,
    );
    ctx.f.set_term(
        fast_head,
        Terminator::CondBr {
            cond: Operand::Value(done),
            then_blk: after,
            else_blk: scan_init,
        },
    );

    let pos = ctx.f.append_inst(
        scan_init,
        InstKind::Load(Type::I64, Operand::Value(pos_slot), 0),
        Type::I64,
        None,
    );
    ctx.f.append_void(
        scan_init,
        InstKind::Store(Operand::Value(pos), Operand::Value(k_slot), 0),
    );
    ctx.f.set_term(scan_init, Terminator::Br(scan_head));

    let k = ctx.f.append_inst(
        scan_head,
        InstKind::Load(Type::I64, Operand::Value(k_slot), 0),
        Type::I64,
        None,
    );
    let in_range = ctx.f.append_inst(
        scan_head,
        InstKind::ICmp(IPred::Slt, Operand::Value(k), Operand::Value(plen)),
        Type::Bool,
        None,
    );
    ctx.f.set_term(
        scan_head,
        Terminator::CondBr {
            cond: Operand::Value(in_range),
            then_blk: scan_probe,
            else_blk: scan_emit,
        },
    );

    let b = ctx.f.append_inst(
        scan_probe,
        InstKind::LoadU8Dyn(Operand::Value(pbase), Operand::Value(k)),
        Type::I64,
        None,
    );
    let is_sep = ctx.f.append_inst(
        scan_probe,
        InstKind::ICmp(
            IPred::Eq,
            Operand::Value(b),
            Operand::ConstI64(target as i64),
        ),
        Type::Bool,
        None,
    );
    ctx.f.set_term(
        scan_probe,
        Terminator::CondBr {
            cond: Operand::Value(is_sep),
            then_blk: scan_emit,
            else_blk: scan_next,
        },
    );

    let k1 = ctx.f.append_inst(
        scan_next,
        InstKind::BinOp(SsaBinOp::Add, Operand::Value(k), Operand::ConstI64(1)),
        Type::I64,
        None,
    );
    ctx.f.append_void(
        scan_next,
        InstKind::Store(Operand::Value(k1), Operand::Value(k_slot), 0),
    );
    ctx.f.set_term(scan_next, Terminator::Br(scan_head));

    // Yield: record the part in the view slots and, unless the body
    // reads it only through the view, write the 32-byte Substr borrow
    // into sub_slot.
    let k2 = ctx.f.append_inst(
        scan_emit,
        InstKind::Load(Type::I64, Operand::Value(k_slot), 0),
        Type::I64,
        None,
    );
    let len = ctx.f.append_inst(
        scan_emit,
        InstKind::BinOp(SsaBinOp::Sub, Operand::Value(k2), Operand::Value(pos)),
        Type::I64,
        None,
    );
    ctx.f.append_void(
        scan_emit,
        InstKind::Store(Operand::Value(len), Operand::Value(len_slot), 0),
    );
    ctx.f.append_void(
        scan_emit,
        InstKind::Store(Operand::Value(pos), Operand::Value(off_slot), 0),
    );
    if fill_sub {
        let fields = [
            (Operand::ConstI64(HDR_STATIC_LITERAL), 0),
            (Operand::Value(len), 8),
            (*parent_op, 16),
            (Operand::Value(pos), 24),
        ];
        for (v, off) in fields {
            ctx.f
                .append_void(scan_emit, InstKind::Store(v, Operand::Value(sub_slot), off));
        }
    }
    let at_end = ctx.f.append_inst(
        scan_emit,
        InstKind::ICmp(IPred::Eq, Operand::Value(k2), Operand::Value(plen)),
        Type::Bool,
        None,
    );
    ctx.f.set_term(
        scan_emit,
        Terminator::CondBr {
            cond: Operand::Value(at_end),
            then_blk: mark_done,
            else_blk: advance,
        },
    );

    ctx.f.append_void(
        mark_done,
        InstKind::Store(Operand::ConstBool(true), Operand::Value(done_slot), 0),
    );
    ctx.f.set_term(mark_done, Terminator::Br(body_blk));

    let k3 = ctx.f.append_inst(
        advance,
        InstKind::BinOp(SsaBinOp::Add, Operand::Value(k2), Operand::ConstI64(1)),
        Type::I64,
        None,
    );
    ctx.f.append_void(
        advance,
        InstKind::Store(Operand::Value(k3), Operand::Value(pos_slot), 0),
    );
    ctx.f.set_term(advance, Terminator::Br(body_blk));
}
