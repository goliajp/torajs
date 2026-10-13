//! The fast arm's separator scan, a word at a time. The parent is read
//! eight bytes at once at multiples of eight from its payload, and every
//! lane equal to the separator is marked: with `x = word ^ splat(sep)`,
//! `((x & 0x7f..) + 0x7f.. | x) & 0x80..` has the high bit of exactly
//! the lanes that are not zero, so its complement under `0x80..` is an
//! exact mark of the zero lanes. The marks of a word are kept across
//! iterations: each part whose separator sits in the word already read
//! is cut out of the mark word alone — the bit index of the lowest mark
//! over eight — and the next word is read only once the marks run out.
//! The marks and the index of the word they belong to are the whole
//! state across parts; the next unread word starts eight past it. Every
//! value kept live across the loop takes a callee-saved register from a
//! body that is already short of them (the loop has calls on its cold
//! paths), so no constant is hoisted: `0x80..` is a bitmask immediate,
//! `0x7f..` one `mov`, and the separator is cancelled one run of set
//! bits at a time, each run in every lane being a bitmask immediate
//! (`,` = 0x2c is `eor #0x2020..` then `eor #0x0c0c..`), where its
//! splat built whole is a `mov` and three `movk` on every word read.
//!
//! The last word of a parent whose length is not a multiple of eight is
//! read whole too, with the lanes past the end masked off. The read is
//! aligned (the payload sits at +16 of an 8-aligned cell, in the heap
//! and in `__TEXT` alike, and the word index is a multiple of eight), so
//! it never crosses a page and the bytes it covers past the end belong
//! to the cell's own padding or the next cell — the same aligned
//! over-read `strlen` and `memchr` rely on.
//!
//! A word costs one load and six ALU instructions and a part found in
//! the marks eight, against six instructions and a taken branch per byte
//! for a byte-wise probe; no part's scan waits on the previous part's
//! position (the only chain through the loop is the word index), and a
//! part cut from the marks knows it is not the last, so only the part
//! that runs to the end pays the exhausted check.

use crate::ssa::{BinOp, BlockId, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::LowerCtx;
use branch::{cond_br, non_zero};

mod branch;

/// One in every lane.
const LANE_ONES: i64 = 0x0101_0101_0101_0101;
/// The low seven bits of every lane.
const LOW7: i64 = 0x7f7f_7f7f_7f7f_7f7f;
/// The high bit of every lane.
const HIGHS: i64 = 0x8080_8080_8080_8080_u64 as i64;
/// `FLAG_STATIC_LITERAL` in the packed header word (flags u16 @6, bit 2):
/// a yielded Substr borrows the parent, so its drop is a no-op.
const HDR_STATIC_LITERAL: i64 = 0x0004i64 << 48;

/// The word scan's separator, its state across parts — the marks left
/// in the word last read and that word's index — and the word index
/// slot one part's word loop runs on (dead between parts).
#[derive(Clone, Copy)]
pub(super) struct State {
    sep: u8,
    marks_slot: ValueId,
    word_slot: ValueId,
    kw_slot: ValueId,
}

/// Set up the scan's state in `ctx.cur_block`, the loop's entry: no
/// marks yet, and the word before the first one read.
pub(super) fn state(ctx: &mut LowerCtx, target: u8) -> State {
    let blk = ctx.cur_block;
    let mut slot = |init: Option<i64>| {
        let s = ctx
            .f
            .append_inst(blk, InstKind::Alloca(Type::I64), Type::Ptr, None);
        if let Some(c) = init {
            ctx.f.append_void(
                blk,
                InstKind::Store(Operand::ConstI64(c), Operand::Value(s), 0),
            );
        }
        s
    };
    State {
        sep: target,
        marks_slot: slot(Some(0)),
        word_slot: slot(Some(-8)),
        kw_slot: slot(None),
    }
}

/// What one part's scan reads: the payload base and length hoisted by
/// the fast arm, the part's first byte, and the state.
pub(super) struct Scan {
    pub(super) pbase: ValueId,
    pub(super) plen: ValueId,
    pub(super) pos: ValueId,
    pub(super) state: State,
}

/// Where a part is handed to the body: the view slots both arms fill,
/// the Substr in `sub_slot` when the body reads the part through it,
/// and the loop bookkeeping.
pub(super) struct Yield {
    pub(super) parent_op: Operand,
    pub(super) sub_slot: ValueId,
    pub(super) fill_sub: bool,
    pub(super) pos_slot: ValueId,
    pub(super) done_slot: ValueId,
    pub(super) len_slot: ValueId,
    pub(super) off_slot: ValueId,
    pub(super) body_blk: BlockId,
}

/// From `from`, the part starting at `pos`: its separator comes from the
/// marks left over, from the whole words still unread, or from the last
/// partial word; failing all three the part runs to the end and the
/// loop is marked exhausted. Each case yields into `y.body_blk`.
pub(super) fn emit(ctx: &mut LowerCtx, from: BlockId, sc: &Scan, y: &Yield) {
    let Scan {
        pbase,
        plen,
        pos,
        state,
    } = *sc;
    let v = Operand::Value;
    let bin = |ctx: &mut LowerCtx, blk: BlockId, op: BinOp, a: Operand, b: Operand| {
        ctx.f
            .append_inst(blk, InstKind::BinOp(op, a, b), Type::I64, None)
    };
    let store = |ctx: &mut LowerCtx, blk: BlockId, val: Operand, slot: ValueId| {
        ctx.f.append_void(blk, InstKind::Store(val, v(slot), 0));
    };
    // the part `[pos, end)` handed to the body; a separator at `end`
    // starts the next part after it, the parent's end exhausts the loop
    let yield_part = |ctx: &mut LowerCtx, blk: BlockId, end: ValueId, last: bool| {
        let len = bin(ctx, blk, BinOp::Sub, v(end), v(pos));
        store(ctx, blk, v(len), y.len_slot);
        store(ctx, blk, v(pos), y.off_slot);
        if y.fill_sub {
            for (val, off) in [
                (Operand::ConstI64(HDR_STATIC_LITERAL), 0),
                (v(len), 8),
                (y.parent_op, 16),
                (v(pos), 24),
            ] {
                ctx.f
                    .append_void(blk, InstKind::Store(val, v(y.sub_slot), off));
            }
        }
        if last {
            store(ctx, blk, Operand::ConstBool(true), y.done_slot);
        } else {
            let next = bin(ctx, blk, BinOp::Add, v(end), Operand::ConstI64(1));
            store(ctx, blk, v(next), y.pos_slot);
        }
        ctx.f.set_term(blk, Terminator::Br(y.body_blk));
    };
    // the lowest mark of `marks` in the word at `wpos` is the part's
    // separator; the mark itself comes off the word, and a word just
    // read (`fresh`) becomes the one the marks belong to
    let cut = |ctx: &mut LowerCtx, blk: BlockId, marks: ValueId, wpos: ValueId, fresh: bool| {
        if fresh {
            store(ctx, blk, v(wpos), state.word_slot);
        }
        let tz = ctx
            .f
            .append_inst(blk, InstKind::Cttz(v(marks)), Type::I64, None);
        let lane = bin(ctx, blk, BinOp::LShr, v(tz), Operand::ConstI64(3));
        let at = bin(ctx, blk, BinOp::Add, v(wpos), v(lane));
        let below = bin(ctx, blk, BinOp::Sub, v(marks), Operand::ConstI64(1));
        let rest = bin(ctx, blk, BinOp::And, v(marks), v(below));
        store(ctx, blk, v(rest), state.marks_slot);
        yield_part(ctx, blk, at, false);
    };
    // the word at `k`, its separator lanes marked
    let marks_of = |ctx: &mut LowerCtx, blk: BlockId, k: ValueId| {
        let word = ctx.f.append_inst(
            blk,
            InstKind::LoadDyn(Type::I64, v(pbase), v(k)),
            Type::I64,
            None,
        );
        let x = sep_runs(state.sep).fold(word, |x, run| {
            bin(ctx, blk, BinOp::Xor, v(x), Operand::ConstI64(run))
        });
        let low = bin(ctx, blk, BinOp::And, v(x), Operand::ConstI64(LOW7));
        let sum = bin(ctx, blk, BinOp::Add, v(low), Operand::ConstI64(LOW7));
        let set = bin(ctx, blk, BinOp::Or, v(sum), v(x));
        let high = bin(ctx, blk, BinOp::And, v(set), Operand::ConstI64(HIGHS));
        bin(ctx, blk, BinOp::Xor, v(high), Operand::ConstI64(HIGHS))
    };
    // the word the marks belong to: -8 before the first part, so the
    // first word read starts at 0
    let last_word = ctx.f.append_inst(
        from,
        InstKind::Load(Type::I64, v(state.word_slot), 0),
        Type::I64,
        None,
    );

    let cut_marks = ctx.f.add_block();
    let head = ctx.f.add_block();
    let step = ctx.f.add_block();
    let fit = ctx.f.add_block();
    let read = ctx.f.add_block();
    let skip = ctx.f.add_block();
    let found = ctx.f.add_block();
    let part = ctx.f.add_block();
    let found_part = ctx.f.add_block();
    let end = ctx.f.add_block();

    let marks = ctx.f.append_inst(
        from,
        InstKind::Load(Type::I64, v(state.marks_slot), 0),
        Type::I64,
        None,
    );
    let has = non_zero(ctx, from, marks);
    cond_br(ctx, from, has, cut_marks, head);
    cut(ctx, cut_marks, marks, last_word, false);

    let k0 = bin(ctx, head, BinOp::Add, v(last_word), Operand::ConstI64(8));
    store(ctx, head, v(k0), state.kw_slot);
    ctx.f.set_term(head, Terminator::Br(step));

    let k = ctx.f.append_inst(
        step,
        InstKind::Load(Type::I64, v(state.kw_slot), 0),
        Type::I64,
        None,
    );
    // a whole word is asked first: it is the one every word of a part
    // but the parent's last takes
    let k8 = bin(ctx, step, BinOp::Add, v(k), Operand::ConstI64(8));
    let whole = ctx.f.append_inst(
        step,
        InstKind::ICmp(IPred::Sle, v(k8), v(plen)),
        Type::Bool,
        None,
    );
    cond_br(ctx, step, whole, read, fit);
    let past = ctx.f.append_inst(
        fit,
        InstKind::ICmp(IPred::Sge, v(k), v(plen)),
        Type::Bool,
        None,
    );
    cond_br(ctx, fit, past, end, part);

    let m = marks_of(ctx, read, k);
    let any = non_zero(ctx, read, m);
    cond_br(ctx, read, any, found, skip);
    store(ctx, skip, v(k8), state.kw_slot);
    ctx.f.set_term(skip, Terminator::Br(step));
    cut(ctx, found, m, k, true);

    // the last word, read whole: only the lanes inside the parent count
    let raw = marks_of(ctx, part, k);
    let left = bin(ctx, part, BinOp::Sub, v(plen), v(k));
    let bits = bin(ctx, part, BinOp::Shl, v(left), Operand::ConstI64(3));
    let over = bin(ctx, part, BinOp::Shl, Operand::ConstI64(1), v(bits));
    let keep = bin(ctx, part, BinOp::Sub, v(over), Operand::ConstI64(1));
    let mp = bin(ctx, part, BinOp::And, v(raw), v(keep));
    let any_part = non_zero(ctx, part, mp);
    cond_br(ctx, part, any_part, found_part, end);
    cut(ctx, found_part, mp, k, true);

    yield_part(ctx, end, plen, true);
}

/// `sep` in every lane, as the splats of its maximal runs of set bits:
/// their xor is the whole splat, and each is a logical immediate. All
/// eight bits are one run no immediate encodes, so 0xff is two.
fn sep_runs(sep: u8) -> impl Iterator<Item = i64> {
    let mut rest = sep;
    std::iter::from_fn(move || {
        if rest == 0 {
            return None;
        }
        let low = rest.isolate_lowest_one();
        let run = if rest == 0xff {
            0xfe
        } else {
            rest & !(rest.wrapping_add(low))
        };
        rest &= !run;
        Some(i64::from(run) * LANE_ONES)
    })
}
