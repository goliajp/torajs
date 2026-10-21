//! S7-r2 fast-arm machinery for [`crate::ssa_lower_stmt_for_of_split_iter`]
//! — the hoisted-invariant header dispatch and the inline Latin-1
//! `next` (the word-at-a-time scan itself is in [`word`]). Split out as a sibling when the versioned-loop
//! emission pushed the parent past the 500-line file cap; the
//! parent answers "what loop shape does ForOfSplitIter lower to",
//! this file answers "what does one fast-arm iteration emit".
//! Rationale for the versioning itself lives in the parent's
//! module doc (## S7-r2 section).

mod word;

use crate::ssa::{BinOp as SsaBinOp, BlockId, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_stmt_for_of_split_iter::SplitView;

/// `flags u16 @6` of the packed Str header u64 — mirrored here as
/// header-word masks (`flag << 48`). Values are wire-format
/// (`torajs-str/src/layout.rs` STR_FLAG_IS_LATIN1 = 0x0002,
/// `torajs-rc/src/flags.rs` FLAG_STATIC_LITERAL = 0x0004).
const HDR_MASK_IS_LATIN1: i64 = 0x0002i64 << 48;
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
    let Hoisted {
        is_lat,
        plen,
        pbase,
    } = hoist_invariants(ctx, cur, *parent_op);
    let word = word::state(ctx, target);
    // Fast-arm loop state (mem2reg-liftable scalar slots).
    let pos_slot = ctx
        .f
        .append_inst(cur, InstKind::Alloca(Type::I64), Type::Ptr, None);
    let done_slot = ctx
        .f
        .append_inst(cur, InstKind::Alloca(Type::Bool), Type::Ptr, None);
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
    // The Substr's header and parent words are the same for every part
    // the fast arm yields; the slow arm's kernel writes all four.
    if fill_sub {
        for (val, off) in [
            (Operand::ConstI64(word::HDR_STATIC_LITERAL), 0),
            (*parent_op, 16),
        ] {
            ctx.f
                .append_void(cur, InstKind::Store(val, Operand::Value(sub_slot), off));
        }
    }
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
    // the slow arm never runs the fast one: it starts done
    ctx.f.append_void(
        slow_init,
        InstKind::Store(Operand::ConstBool(true), Operand::Value(done_slot), 0),
    );
    ctx.f.set_term(slow_init, Terminator::Br(header));

    // Per part the fast arm tests only its done flag; a done loop asks
    // the encoding again, which tells the exhausted fast arm from the
    // slow one. That second test is the Latin-1 flag guard the block
    // frequencies know, with the kernel call on one arm only, so the
    // slow arm stays cold (`ssa::branch_prob`).
    let fast_head = ctx.f.add_block();
    let slow_head = ctx.f.add_block();
    let not_fast = ctx.f.add_block();
    let exhausted = ctx.f.add_block();
    emit_dispatch(
        ctx,
        &Dispatch {
            header,
            done_slot,
            is_lat,
            not_fast,
            exhausted,
            fast_head,
            slow_head,
            after,
        },
    );

    emit_slow_arm(
        ctx,
        &SlowArm {
            head: slow_head,
            iter_slot,
            sub_slot,
            len_slot,
            off_slot,
            body_blk,
            after,
        },
    );
    emit_fast_next(
        ctx,
        &FastScan {
            sub_slot,
            plen,
            pbase,
            word,
            pos_slot,
            done_slot,
            len_slot,
            off_slot,
            fill_sub,
            fast_head,
            body_blk,
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

/// The loop invariants read once on the entry side, before the header.
struct Hoisted {
    /// Whether the parent is Latin-1: the fast arm's guard.
    is_lat: ValueId,
    /// The parent's code-unit count.
    plen: ValueId,
    /// The parent's payload base.
    pbase: ValueId,
}

fn hoist_invariants(ctx: &mut LowerCtx, cur: BlockId, parent_op: Operand) -> Hoisted {
    let hdr = ctx.f.append_inst(
        cur,
        InstKind::Load(Type::I64, parent_op, 0),
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
        InstKind::Load(Type::I32, parent_op, 8),
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
        .append_inst(cur, InstKind::PtrToInt(parent_op), Type::I64, None);
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
    Hoisted {
        is_lat,
        plen,
        pbase,
    }
}

/// The header's per-part test and the blocks it picks between.
struct Dispatch {
    header: BlockId,
    done_slot: ValueId,
    is_lat: ValueId,
    not_fast: BlockId,
    exhausted: BlockId,
    fast_head: BlockId,
    slow_head: BlockId,
    after: BlockId,
}

fn emit_dispatch(ctx: &mut LowerCtx, d: &Dispatch) {
    let done = ctx.f.append_inst(
        d.header,
        InstKind::Load(Type::Bool, Operand::Value(d.done_slot), 0),
        Type::Bool,
        None,
    );
    ctx.f.set_term(
        d.header,
        Terminator::CondBr {
            cond: Operand::Value(done),
            then_blk: d.not_fast,
            else_blk: d.fast_head,
        },
    );
    ctx.f.set_term(
        d.not_fast,
        Terminator::CondBr {
            cond: Operand::Value(d.is_lat),
            then_blk: d.exhausted,
            else_blk: d.slow_head,
        },
    );
    ctx.f.set_term(d.exhausted, Terminator::Br(d.after));
}

/// The slow arm's blocks and the slots it reads and fills.
struct SlowArm {
    head: BlockId,
    iter_slot: ValueId,
    sub_slot: ValueId,
    len_slot: ValueId,
    off_slot: ValueId,
    body_blk: BlockId,
    after: BlockId,
}

/// The kernel call, and on a part its length and offset into the view
/// slots.
fn emit_slow_arm(ctx: &mut LowerCtx, a: &SlowArm) {
    let ok = ctx.f.append_inst(
        a.head,
        InstKind::Call(
            ctx.intrinsics.split_iter_next,
            vec![Operand::Value(a.iter_slot), Operand::Value(a.sub_slot)],
        ),
        Type::Bool,
        None,
    );
    let slow_yield = ctx.f.add_block();
    ctx.f.set_term(
        a.head,
        Terminator::CondBr {
            cond: Operand::Value(ok),
            then_blk: slow_yield,
            else_blk: a.after,
        },
    );
    for (off, slot) in [(8, a.len_slot), (24, a.off_slot)] {
        let v = ctx.f.append_inst(
            slow_yield,
            InstKind::Load(Type::I64, Operand::Value(a.sub_slot), off),
            Type::I64,
            None,
        );
        ctx.f.append_void(
            slow_yield,
            InstKind::Store(Operand::Value(v), Operand::Value(slot), 0),
        );
    }
    ctx.f.set_term(slow_yield, Terminator::Br(a.body_blk));
}

/// Everything the fast-arm inline `next` needs — the hoisted loop
/// invariants and the pre-created blocks it wires into.
struct FastScan {
    sub_slot: ValueId,
    /// Hoisted `parent_len` (loaded once from the Str header).
    plen: ValueId,
    /// Hoisted `parent + STR_DATA_OFF` payload base.
    pbase: ValueId,
    /// The word-at-a-time separator scan's invariants and state.
    word: word::State,
    pos_slot: ValueId,
    done_slot: ValueId,
    len_slot: ValueId,
    off_slot: ValueId,
    fill_sub: bool,
    fast_head: BlockId,
    body_blk: BlockId,
}

/// Emit the fast-arm inline `next` (the header has already found the
/// arm not done): the word-at-a-time separator scan, which yields
/// the part itself (view slots, the Substr's length and offset in
/// `sub_slot` when the body reads it, and the advance / exhaust
/// bookkeeping).
fn emit_fast_next(ctx: &mut LowerCtx, fs: &FastScan) {
    let scan_init = fs.fast_head;
    let pos = ctx.f.append_inst(
        scan_init,
        InstKind::Load(Type::I64, Operand::Value(fs.pos_slot), 0),
        Type::I64,
        None,
    );
    word::emit(
        ctx,
        scan_init,
        &word::Scan {
            pbase: fs.pbase,
            plen: fs.plen,
            pos,
            state: fs.word,
        },
        &word::Yield {
            sub_slot: fs.sub_slot,
            fill_sub: fs.fill_sub,
            pos_slot: fs.pos_slot,
            done_slot: fs.done_slot,
            len_slot: fs.len_slot,
            off_slot: fs.off_slot,
            body_blk: fs.body_blk,
        },
    );
}
