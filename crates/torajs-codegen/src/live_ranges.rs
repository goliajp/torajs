//! Per-value live ranges with holes.
//!
//! A value's `Interval` is the hull of the slots where it is live; the
//! value itself is live only on some of them. Block layout puts code
//! between a value's def and its uses that never runs while the value
//! is live (the other arm of a branch, a loop copy reached on another
//! path), and the hull spans it anyway. `LiveRanges` keeps the slots
//! that are live, as sorted, pairwise disjoint inclusive ranges over
//! the global slot numbering of `liveness::compute_intervals`; the
//! sweep lets two values share a register when their ranges never
//! meet, even when their hulls do.
//!
//! Ranges come from one backward walk per block over the block live
//! sets, at half-slot precision (reads at `2p`, writes at `2p + 1`),
//! which copy coalescing reads directly. The same walk widens every
//! hull over the blocks the value is live into or out of. In slots, a
//! range live into a block starts one slot before the block's first
//! inst, as the hull does: a value live into a block that opens with a
//! call crosses that call.
//!
//! Parameters and alloca results keep their hull as a single range:
//! the hull of both is pinned past what the block live sets see.
//!
//! `TORAJS_HOLES_OFF=1` gives every value its hull as a single range
//! (bisect gate); the allocation is then the one without holes.

use std::cell::Cell;

use torajs_core::ssa::id_hash::HashMap;

use torajs_core::ssa::Function;

use crate::coalesce::Classes;
use crate::liveness::{Interval, visit_inst_operands, visit_terminator_operands};
use crate::liveness_blocks::BlockLiveness;

/// Per value, the slots where it is live, as sorted pairwise disjoint
/// inclusive ranges; its `Interval` stays the hull.
pub(crate) struct LiveRanges {
    /// The ranges of value `v` are `ranges[spans[v].0..spans[v].1]`.
    spans: Vec<(u32, u32)>,
    ranges: Vec<(u32, u32)>,
}

impl LiveRanges {
    /// The ranges of every value `hulls` knows (a hull starting at
    /// `u32::MAX` marks a value never defined) and `hull_only` leaves
    /// out, as `(value, range)` in ascending order per value, widening
    /// each known hull over its ranges and over every block the value
    /// is live into.
    pub(crate) fn walk(
        func: &Function,
        blocks: &BlockLiveness,
        hulls: &mut [Interval],
        hull_only: &[bool],
    ) -> Vec<(u32, (u32, u32))> {
        // per value, its hull and the range being extended, side by
        // side: the walk touches both for every live value of a block;
        // a value's ranges arrive in ascending order of start, so
        // merging the overlapping ones needs no sort
        let mut state: Vec<(Interval, (u32, u32))> =
            hulls.iter().map(|&h| (h, (u32::MAX, 0))).collect();
        let cells = Cell::from_mut(&mut state[..]).as_slice_of_cells();
        let mut pairs: Vec<(u32, (u32, u32))> = Vec::new();
        walk_half_slots(
            func,
            blocks,
            |v| cells[v as usize].get().0.start != u32::MAX,
            |b, block| {
                let (bs, be) = (blocks.start[b], blocks.end[b]);
                for &(v, lo, hi) in block.iter().rev() {
                    let (s, e) = to_slots(lo, hi);
                    let (mut h, mut c) = cells[v as usize].get();
                    h.start = h.start.min(s);
                    h.end = h.end.max(e);
                    if lo == 2 * bs {
                        // live in: the hull takes the whole block
                        h.end = h.end.max(be);
                    }
                    if c.0 != u32::MAX && s <= c.1 {
                        c.1 = c.1.max(e);
                    } else {
                        if c.0 != u32::MAX && !hull_only[v as usize] {
                            pairs.push((v, c));
                        }
                        c = (s, e);
                    }
                    cells[v as usize].set((h, c));
                }
            },
        );
        for (v, (h, c)) in state.into_iter().enumerate() {
            hulls[v] = h;
            if c.0 != u32::MAX && !hull_only[v] {
                pairs.push((v as u32, c));
            }
        }
        pairs
    }

    /// Group `(value, range)` pairs by value; each value keeps the
    /// order its pairs come in.
    pub(crate) fn from_pairs(values: usize, pairs: &[(u32, (u32, u32))]) -> Self {
        // counts first, then each value's run, filled forward
        let mut spans = vec![(0u32, 0u32); values];
        for &(v, _) in pairs {
            spans[v as usize].1 += 1;
        }
        let mut start = 0;
        for span in &mut spans {
            let count = span.1;
            *span = (start, start);
            start += count;
        }
        let mut ranges = vec![(0u32, 0u32); pairs.len()];
        for &(v, r) in pairs {
            let span = &mut spans[v as usize];
            ranges[span.1 as usize] = r;
            span.1 += 1;
        }
        LiveRanges { spans, ranges }
    }

    /// The ranges the sweep reads, once copy classes are folded into
    /// their leaders: a leader's are its members' (under
    /// `TORAJS_HOLES_OFF=1`, the class hull). A member keeps its own;
    /// the sweep only asks for the leader's.
    pub(crate) fn fold(mut self, intervals: &HashMap<u32, Interval>, classes: &Classes) -> Self {
        let off = holes_off();
        for (leader, members) in classes.iter() {
            let first = self.ranges.len();
            if off {
                let it = intervals[&leader];
                self.ranges.push((it.start, it.end));
            } else {
                let mut own: Vec<(u32, u32)> =
                    members.iter().flat_map(|&m| self.of(m)).copied().collect();
                own.sort_unstable();
                for (lo, hi) in own {
                    match self.ranges.len() > first && lo <= self.ranges[self.ranges.len() - 1].1 {
                        true => {
                            let last = self.ranges.len() - 1;
                            self.ranges[last].1 = self.ranges[last].1.max(hi);
                        }
                        false => self.ranges.push((lo, hi)),
                    }
                }
            }
            self.spans[leader as usize] = (first as u32, self.ranges.len() as u32);
        }
        self
    }

    /// Whether every value's ranges are sorted, disjoint, start where
    /// its hull starts and stay inside it: the sweep releases a
    /// register by the ranges and orders values by the hull.
    pub(crate) fn within(&self, intervals: &HashMap<u32, Interval>) -> bool {
        intervals.iter().all(|(&v, it)| {
            let r = self.of(v);
            r.first().is_some_and(|f| f.0 == it.start)
                && (r.iter()).all(|&(lo, hi)| it.start <= lo && lo <= hi && hi <= it.end)
                && r.windows(2).all(|w| w[0].1 < w[1].0)
        })
    }

    /// How many values the ranges are indexed by.
    pub(crate) fn values(&self) -> usize {
        self.spans.len()
    }

    /// The ranges of `v`, ascending.
    pub(crate) fn of(&self, v: u32) -> &[(u32, u32)] {
        let (start, end) = self.spans[v as usize];
        &self.ranges[start as usize..end as usize]
    }

    /// The first range of `v` from its `from`-th on that does not end
    /// before `pos`, with its index.
    pub(crate) fn next_from(&self, v: u32, pos: u32, from: usize) -> Option<(usize, (u32, u32))> {
        let r = self.of(v);
        (from..r.len())
            .map(|i| (i, r[i]))
            .find(|&(_, (_, hi))| hi >= pos)
    }

    /// Whether `a` and `b` share a slot.
    pub(crate) fn intersect(a: &[(u32, u32)], b: &[(u32, u32)]) -> bool {
        let (Some(&(a0, _)), Some(&(b0, _))) = (a.first(), b.first()) else {
            return false;
        };
        // both lists ascend, so start each at the first range that can
        // reach the other's first slot
        let mut i = a.partition_point(|&(_, hi)| hi < b0);
        let mut j = b.partition_point(|&(_, hi)| hi < a0);
        while i < a.len() && j < b.len() {
            if a[i].1 < b[j].0 {
                i += 1;
            } else if b[j].1 < a[i].0 {
                j += 1;
            } else {
                return true;
            }
        }
        false
    }
}

/// `TORAJS_HOLES_OFF=1`: every value's hull is its only range.
pub(crate) fn holes_off() -> bool {
    std::env::var_os("TORAJS_HOLES_OFF").is_some_and(|v| v == "1")
}

/// A half-slot range in slots. Only a range live into a block starts
/// on a read half (`2 * block start`); in slots it starts one earlier.
fn to_slots(lo: u32, hi: u32) -> (u32, u32) {
    let start = if lo.is_multiple_of(2) {
        (lo / 2).saturating_sub(1)
    } else {
        lo / 2
    };
    (start, hi / 2)
}

/// Call `emit(b, ranges)` once per block `b` with the inclusive
/// half-slot ranges `(v, lo, hi)` where a value `keep` admits holds a
/// live value, from one backward walk over the block: they come in
/// descending order of `lo`, and only a range live into the block
/// starts at `2 * block start`. A def with no later read still
/// occupies its write half: the register is clobbered there.
pub(crate) fn walk_half_slots(
    func: &Function,
    blocks: &BlockLiveness,
    keep: impl Fn(u32) -> bool,
    mut emit: impl FnMut(usize, &[(u32, u32, u32)]),
) {
    let mut open = Open {
        at: vec![(u32::MAX, 0); func.values.len()],
        list: Vec::new(),
        block: 0,
    };
    let mut out: Vec<(u32, u32, u32)> = Vec::new();
    for (b, block) in func.blocks.iter().enumerate() {
        let (bs, be) = (blocks.start[b], blocks.end[b]);
        open.block = b as u32;
        for v in blocks.live_out[b].iter().filter(|&v| keep(v)) {
            open.open(v, 2 * be + 1);
        }
        visit_terminator_operands(&block.term, |v| {
            if keep(v.0) {
                open.open(v.0, 2 * be);
            }
        });
        for (k, inst) in block.insts.iter().enumerate().rev() {
            let p = bs + k as u32;
            if let Some(r) = inst.result
                && keep(r.0)
            {
                let end = open.close(r.0).unwrap_or(2 * p + 1);
                out.push((r.0, 2 * p + 1, end));
            }
            visit_inst_operands(&inst.kind, |v| {
                if keep(v.0) {
                    open.open(v.0, 2 * p);
                }
            });
        }
        let list = std::mem::take(&mut open.list);
        for &v in &list {
            if let Some(end) = open.close(v) {
                out.push((v, 2 * bs, end));
            }
        }
        open.list = list;
        open.list.clear();
        emit(b, &out);
        out.clear();
    }
}

/// The values with an open range in the current block, by value index:
/// `at[v] == (block, end)` while `v` is open, its range ending at
/// `end`.
struct Open {
    at: Vec<(u32, u32)>,
    list: Vec<u32>,
    block: u32,
}

impl Open {
    fn open(&mut self, v: u32, end: u32) {
        let at = &mut self.at[v as usize];
        if at.0 != self.block {
            *at = (self.block, end);
            self.list.push(v);
        }
    }

    fn close(&mut self, v: u32) -> Option<u32> {
        let at = &mut self.at[v as usize];
        (at.0 == self.block).then(|| {
            at.0 = u32::MAX;
            at.1
        })
    }
}

#[cfg(test)]
mod tests;
