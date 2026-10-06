//! Reading the unit a state dispatches on, stepping the start scan by
//! the same unit, and the search over a state's edges when there are
//! too many for a compare chain.

use crate::ssa::{BinOp, BlockId, IPred, Operand, Terminator, ValueId};
use torajs_regex::onepass::Edge;

use super::Emit;

/// Most edges a state tests one after the other; past this the edges
/// are searched as a balanced tree of compares, so a property class's
/// hundreds of ranges cost a logarithmic number of branches.
pub(super) const CHAIN_MAX: usize = 4;

impl Emit<'_> {
    /// Read the unit at `pos` from `blk`. Answers the block the walk
    /// continues in, the unit, and how many code units it spans.
    /// With `pair`, a high surrogate followed by a low one reads as
    /// the code point the two spell and spans both.
    pub(super) fn read_unit(
        &mut self,
        blk: BlockId,
        pos: ValueId,
        pair: bool,
    ) -> (BlockId, ValueId, Operand) {
        let u = self.unit(blk, pos);
        let Some((cp_slot, step_slot)) = self.pair_slots.filter(|_| pair) else {
            return (blk, u, Operand::ConstI64(1));
        };
        let check = self.f.add_block();
        let read = self.f.add_block();
        let single = self.f.add_block();
        let both = self.f.add_block();
        let join = self.f.add_block();
        let is_high = self.in_range(blk, u, 0xD800, 0xDBFF);
        self.cond_br(blk, is_high, check, single);
        let next = self.binop(check, BinOp::Add, pos, Operand::ConstI64(1));
        let at_end = self.icmp(check, IPred::Eq, next, Operand::Value(self.len));
        self.cond_br(check, at_end, single, read);
        let low = self.unit(read, next);
        let is_low = self.in_range(read, low, 0xDC00, 0xDFFF);
        self.cond_br(read, is_low, both, single);

        self.store(single, Operand::Value(u), cp_slot, 0);
        self.store(single, Operand::ConstI64(1), step_slot, 0);
        self.f.set_term(single, Terminator::Br(join));

        // 0x10000 + ((high - 0xD800) << 10) + (low - 0xDC00)
        let high = self.binop(both, BinOp::Shl, u, Operand::ConstI64(10));
        let sum = self.binop(both, BinOp::Add, high, Operand::Value(low));
        let bias = 0x10000 - (0xD800 << 10) - 0xDC00;
        let cp = self.binop(both, BinOp::Add, sum, Operand::ConstI64(bias));
        self.store(both, Operand::Value(cp), cp_slot, 0);
        self.store(both, Operand::ConstI64(2), step_slot, 0);
        self.f.set_term(both, Terminator::Br(join));

        let cp = self.load(join, cp_slot, 0);
        let step = self.load(join, step_slot, 0);
        (join, cp, Operand::Value(step))
    }

    /// Store `pos + 1` — or `pos + 2` when the unit `u` read at `pos`
    /// is a high surrogate followed by a low one — into `slot`, then
    /// branch to `target`: one step of the start scan. Answers the
    /// block that stores the single step, for a caller that already
    /// knows it is at the end.
    fn pair_advance(
        &mut self,
        blk: BlockId,
        pos: ValueId,
        u: ValueId,
        slot: ValueId,
        target: BlockId,
    ) -> BlockId {
        let one = self.f.add_block();
        let next1 = self.binop(one, BinOp::Add, pos, Operand::ConstI64(1));
        self.store(one, Operand::Value(next1), slot, 0);
        self.f.set_term(one, Terminator::Br(target));
        let two = self.f.add_block();
        let next2 = self.binop(two, BinOp::Add, pos, Operand::ConstI64(2));
        self.store(two, Operand::Value(next2), slot, 0);
        self.f.set_term(two, Terminator::Br(target));
        let check = self.f.add_block();
        let read = self.f.add_block();
        let is_high = self.in_range(blk, u, 0xD800, 0xDBFF);
        self.cond_br(blk, is_high, check, one);
        let next1 = self.binop(check, BinOp::Add, pos, Operand::ConstI64(1));
        let at_end = self.icmp(check, IPred::Eq, next1, Operand::Value(self.len));
        self.cond_br(check, at_end, one, read);
        let u2 = self.unit(read, next1);
        let is_low = self.in_range(read, u2, 0xDC00, 0xDFFF);
        self.cond_br(read, is_low, two, one);
        one
    }

    /// `lo <= u <= hi`
    pub(super) fn in_range(&mut self, blk: BlockId, u: ValueId, lo: u32, hi: u32) -> ValueId {
        let span = hi as i64 - lo as i64 + 1;
        if span == 1 {
            return self.icmp(blk, IPred::Eq, u, Operand::ConstI64(lo as i64));
        }
        let rel = if lo == 0 {
            u
        } else {
            self.binop(blk, BinOp::Sub, u, Operand::ConstI64(lo as i64))
        };
        self.icmp(blk, IPred::Ult, rel, Operand::ConstI64(span))
    }

    /// `next_st`: the next start position of the scan
    pub(super) fn scan_advance(&mut self, scan: BlockId) {
        let next_st = self.next_st;
        let st = self.load(next_st, self.st_slot, 0);
        if !self.ir.pairs {
            let bumped = self.binop(next_st, BinOp::Add, st, Operand::ConstI64(1));
            self.store(next_st, Operand::Value(bumped), self.st_slot, 0);
            self.f.set_term(next_st, Terminator::Br(scan));
            return;
        }
        let read = self.f.add_block();
        let u = self.unit(read, st);
        let one = self.pair_advance(read, st, u, self.st_slot, scan);
        let at_end = self.icmp(next_st, IPred::Eq, st, Operand::Value(self.len));
        self.cond_br(next_st, at_end, one, read);
    }

    /// From `blk`, branch to the block of the edge whose range holds
    /// `u` (`taken[k]` for `edges[k]`), or to `fail` when none does.
    ///
    /// `edges` are disjoint and sorted, so they and the gaps between
    /// them cut the unit space into intervals, and the search is a
    /// balanced tree over the interval bounds, one compare a level.
    /// Past Latin-1 the tree is two-level — the unit's page
    /// (`u >> 8`), then its low byte inside a page that holds a bound
    /// — so every bound compared is small enough for a compare's
    /// immediate.
    pub(super) fn search(
        &mut self,
        blk: BlockId,
        u: ValueId,
        edges: &[Edge],
        taken: &[BlockId],
        fail: BlockId,
    ) {
        let mut intervals: Vec<(u32, BlockId)> = Vec::with_capacity(2 * edges.len() + 1);
        let mut next = 0;
        for (edge, &dest) in edges.iter().zip(taken) {
            if edge.lo > next {
                push_interval(&mut intervals, next, fail);
            }
            push_interval(&mut intervals, edge.lo, dest);
            next = edge.hi + 1;
        }
        let unit_max = self.ir.unit_max();
        if next <= unit_max {
            push_interval(&mut intervals, next, fail);
        }
        if unit_max <= 0xFF {
            self.bisect(u, &intervals, Some(blk));
            return;
        }
        let page = self.binop(blk, BinOp::LShr, u, Operand::ConstI64(8));
        let low = self.binop(blk, BinOp::And, u, Operand::ConstI64(0xFF));
        let pages = pages(&intervals, unit_max >> 8);
        self.bisect_pages(page, low, &pages, Some(blk));
    }

    /// The block that routes `u` to the interval holding it: the
    /// interval's own destination when there is one interval, else a
    /// compare against the middle bound (emitted into `into`, or a
    /// fresh block).
    fn bisect(
        &mut self,
        u: ValueId,
        intervals: &[(u32, BlockId)],
        into: Option<BlockId>,
    ) -> BlockId {
        if let [(_, dest)] = intervals {
            let Some(blk) = into else { return *dest };
            self.f.set_term(blk, Terminator::Br(*dest));
            return blk;
        }
        let blk = into.unwrap_or_else(|| self.f.add_block());
        let mid = intervals.len() / 2;
        let bound = Operand::ConstI64(intervals[mid].0 as i64);
        let below = self.icmp(blk, IPred::Slt, u, bound);
        let lower = self.bisect(u, &intervals[..mid], None);
        let upper = self.bisect(u, &intervals[mid..], None);
        self.cond_br(blk, below, lower, upper);
        blk
    }

    /// [`Self::bisect`] over page runs; a page that holds a bound is
    /// searched by the unit's low byte.
    fn bisect_pages(
        &mut self,
        page: ValueId,
        low: ValueId,
        pages: &[(u32, Page)],
        into: Option<BlockId>,
    ) -> BlockId {
        if let [(_, only)] = pages {
            return match only {
                Page::Split(intervals) => self.bisect(low, intervals, into),
                Page::Whole(dest) => {
                    let Some(blk) = into else { return *dest };
                    self.f.set_term(blk, Terminator::Br(*dest));
                    blk
                }
            };
        }
        let blk = into.unwrap_or_else(|| self.f.add_block());
        let mid = pages.len() / 2;
        let bound = Operand::ConstI64(pages[mid].0 as i64);
        let below = self.icmp(blk, IPred::Slt, page, bound);
        let lower = self.bisect_pages(page, low, &pages[..mid], None);
        let upper = self.bisect_pages(page, low, &pages[mid..], None);
        self.cond_br(blk, below, lower, upper);
        blk
    }
}

/// What the units of one run of 256-unit pages map to.
enum Page {
    /// every unit of every page in the run goes to one block
    Whole(BlockId),
    /// one page, cut by low byte
    Split(Vec<(u32, BlockId)>),
}

/// Start an interval at `lo` unless the one before it already leads
/// to `dest`.
fn push_interval(intervals: &mut Vec<(u32, BlockId)>, lo: u32, dest: BlockId) {
    if intervals.last().is_none_or(|&(_, d)| d != dest) {
        intervals.push((lo, dest));
    }
}

/// `intervals` (sorted by start, the first at 0) regrouped by page,
/// up to `max_page`: each entry starts a run of pages.
fn pages(intervals: &[(u32, BlockId)], max_page: u32) -> Vec<(u32, Page)> {
    let dest_at = |unit: u32| intervals[intervals.partition_point(|&(lo, _)| lo <= unit) - 1].1;
    let is_split = |page: u32| {
        let first = intervals.partition_point(|&(lo, _)| lo <= page << 8);
        intervals.get(first).is_some_and(|&(lo, _)| lo >> 8 == page)
    };
    let mut starts: Vec<u32> = Vec::with_capacity(2 * intervals.len());
    for &(lo, _) in intervals {
        starts.push(lo >> 8);
        if lo & 0xFF != 0 {
            starts.push((lo >> 8) + 1);
        }
    }
    starts.retain(|&p| p <= max_page);
    starts.sort_unstable();
    starts.dedup();
    let mut out: Vec<(u32, Page)> = Vec::with_capacity(starts.len());
    for page in starts {
        if is_split(page) {
            let mut cut = Vec::new();
            push_interval(&mut cut, 0, dest_at(page << 8));
            for &(lo, dest) in intervals.iter().filter(|&&(lo, _)| lo >> 8 == page) {
                push_interval(&mut cut, lo & 0xFF, dest);
            }
            out.push((page, Page::Split(cut)));
            continue;
        }
        let dest = dest_at(page << 8);
        if !matches!(out.last(), Some((_, Page::Whole(d))) if *d == dest) {
            out.push((page, Page::Whole(dest)));
        }
    }
    out
}
