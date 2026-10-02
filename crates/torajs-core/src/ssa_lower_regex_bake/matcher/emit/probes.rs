//! The decision tree a probing state opens with: one test per probed
//! context bit, lowest bit first, so the leaf reached is the variant
//! at the index `onepass::ir::pack` gives that assignment. A test
//! branches without materializing a boolean — the unit before or at
//! the position is read once on the path that needs it.

use crate::ssa::{BinOp, BlockId, IPred, Operand, ValueId};
use torajs_regex::onepass::ir::{
    INPUT_END, INPUT_START, LINE_END, LINE_START, WORD_LEFT, WORD_LEFT_FOLD, WORD_RIGHT,
    WORD_RIGHT_FOLD,
};

use super::Emit;

/// §12.3 LineTerminator; the last two are 16-bit units.
const LINE_TERMINATORS: [(u32, u32); 4] = [
    (0x0A, 0x0A),
    (0x0D, 0x0D),
    (0x2028, 0x2028),
    (0x2029, 0x2029),
];
/// §22.2.2.9.1 WordCharacters, ASCII.
const WORD: [(u32, u32); 4] = [(0x30, 0x39), (0x41, 0x5A), (0x61, 0x7A), (0x5F, 0x5F)];
/// The two units that join them under `iu`.
const WORD_FOLD: [(u32, u32); 2] = [(0x17F, 0x17F), (0x212A, 0x212A)];

impl Emit<'_> {
    /// From `blk`, branch on the bits of `remaining` (bit `k` of the
    /// leaf index is the `k`-th probed bit) down to `leaves[index]`.
    pub(super) fn probe_tree(
        &mut self,
        blk: BlockId,
        pos: ValueId,
        remaining: u8,
        k: usize,
        index: usize,
        leaves: &[BlockId],
    ) {
        if remaining == 0 {
            self.f
                .set_term(blk, crate::ssa::Terminator::Br(leaves[index]));
            return;
        }
        let bit = remaining.isolate_lowest_one();
        let yes = self.f.add_block();
        let no = self.f.add_block();
        self.probe(blk, pos, bit, yes, no);
        self.probe_tree(yes, pos, remaining & !bit, k + 1, index | (1 << k), leaves);
        self.probe_tree(no, pos, remaining & !bit, k + 1, index, leaves);
    }

    /// From `blk`, branch to `yes` when the context bit `bit` holds at
    /// `pos`, else to `no`.
    fn probe(&mut self, blk: BlockId, pos: ValueId, bit: u8, yes: BlockId, no: BlockId) {
        let len = self.len;
        match bit {
            INPUT_START => {
                let c = self.icmp(blk, IPred::Eq, pos, Operand::ConstI64(0));
                self.cond_br(blk, c, yes, no);
            }
            INPUT_END => {
                let c = self.icmp(blk, IPred::Eq, pos, Operand::Value(len));
                self.cond_br(blk, c, yes, no);
            }
            LINE_START => {
                let read = self.f.add_block();
                let at_start = self.icmp(blk, IPred::Eq, pos, Operand::ConstI64(0));
                self.cond_br(blk, at_start, yes, read);
                let u = self.unit_before(read, pos);
                self.any_of(read, u, &self.line_terminators(), yes, no);
            }
            LINE_END => {
                let read = self.f.add_block();
                let at_end = self.icmp(blk, IPred::Eq, pos, Operand::Value(len));
                self.cond_br(blk, at_end, yes, read);
                let u = self.unit(read, pos);
                self.any_of(read, u, &self.line_terminators(), yes, no);
            }
            WORD_LEFT | WORD_LEFT_FOLD => {
                let read = self.f.add_block();
                let at_start = self.icmp(blk, IPred::Eq, pos, Operand::ConstI64(0));
                self.cond_br(blk, at_start, no, read);
                let u = self.unit_before(read, pos);
                self.any_of(read, u, &self.word(bit == WORD_LEFT_FOLD), yes, no);
            }
            WORD_RIGHT | WORD_RIGHT_FOLD => {
                let read = self.f.add_block();
                let at_end = self.icmp(blk, IPred::Eq, pos, Operand::Value(len));
                self.cond_br(blk, at_end, no, read);
                let u = self.unit(read, pos);
                self.any_of(read, u, &self.word(bit == WORD_RIGHT_FOLD), yes, no);
            }
            _ => unreachable!("one probe bit"),
        }
    }

    fn unit_before(&mut self, blk: BlockId, pos: ValueId) -> ValueId {
        let prev = self.binop(blk, BinOp::Sub, pos, Operand::ConstI64(1));
        self.unit(blk, prev)
    }

    fn line_terminators(&self) -> Vec<(u32, u32)> {
        let n = if self.ir.wide { 4 } else { 2 };
        LINE_TERMINATORS[..n].to_vec()
    }

    fn word(&self, fold: bool) -> Vec<(u32, u32)> {
        let mut ranges = WORD.to_vec();
        if fold && self.ir.wide {
            ranges.extend_from_slice(&WORD_FOLD);
        }
        ranges
    }

    /// From `blk`, branch to `yes` when `u` lies in one of `ranges`,
    /// else to `no`.
    fn any_of(
        &mut self,
        blk: BlockId,
        u: ValueId,
        ranges: &[(u32, u32)],
        yes: BlockId,
        no: BlockId,
    ) {
        let mut cur = blk;
        for (j, &(lo, hi)) in ranges.iter().enumerate() {
            let next = if j + 1 == ranges.len() {
                no
            } else {
                self.f.add_block()
            };
            let c = self.in_range(cur, u, lo, hi);
            self.cond_br(cur, c, yes, next);
            cur = next;
        }
    }
}
