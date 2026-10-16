//! Per-value live ranges with holes.
//!
//! A value's `Interval` is the hull of the slots where it is live; the
//! value itself is live only on some of them. Block layout puts code
//! between a value's def and its uses that never runs while the value
//! is live (the other arm of a branch, a loop copy reached on another
//! path), and the hull spans it anyway. `LiveRanges` keeps the slots
//! that are live, as sorted, pairwise disjoint inclusive ranges over
//! the global slot numbering of `liveness::compute_intervals`; the
//! sweep lets a value take a register its holders are dead in wherever
//! the value is live, even when their hulls meet.
//!
//! Ranges come from one backward walk per block over the block live
//! sets, and are kept at half-slot precision (reads at `2p`, writes at
//! `2p + 1`), which copy coalescing reads directly: a value read for
//! the last time by the instruction that defines another never meets
//! it, so the two may share a register, as a coalesced copy does.
//!
//! Parameters and alloca results keep their hull as a single range:
//! the hull of both is pinned past what the block live sets see.
//!
//! The sweep asks for ranges only once a pool runs dry (`LazyRanges`):
//! most functions never get there and never pay for the walk.

use std::cell::OnceCell;
use torajs_core::ssa::id_hash::HashMap;

use torajs_core::ssa::{Function, InstKind};

use crate::coalesce::Classes;
use crate::liveness::Interval;
use crate::liveness_blocks::BlockLiveness;

mod walk;
pub(crate) use walk::walk_half_slots;

/// Per value, the half-slots where it is live, as sorted pairwise
/// disjoint inclusive ranges; its `Interval` stays the hull.
pub(crate) struct LiveRanges {
    /// The ranges of value `v` are `ranges[spans[v].0..spans[v].1]`.
    spans: Vec<(u32, u32)>,
    ranges: Vec<(u32, u32)>,
}

/// The ranges the sweep reads, computed the first time it asks: those
/// of every value, with copy classes folded into their leaders.
pub(crate) struct LazyRanges<'a> {
    cell: OnceCell<LiveRanges>,
    func: &'a Function,
    blocks: &'a BlockLiveness,
    /// The intervals once `Classes::fold_into` has folded them.
    intervals: &'a HashMap<u32, Interval>,
    classes: &'a Classes,
}

impl<'a> LazyRanges<'a> {
    pub(crate) fn new(
        func: &'a Function,
        blocks: &'a BlockLiveness,
        intervals: &'a HashMap<u32, Interval>,
        classes: &'a Classes,
    ) -> Self {
        LazyRanges {
            cell: OnceCell::new(),
            func,
            blocks,
            intervals,
            classes,
        }
    }

    /// The ranges of `v`, ascending.
    pub(crate) fn of(&self, v: u32) -> &[(u32, u32)] {
        let ranges = self.cell.get_or_init(|| {
            let r = LiveRanges::compute(self.func, self.blocks, self.intervals, self.classes)
                .fold(self.classes);
            debug_assert!(
                r.within(self.intervals),
                "{}: live ranges leave their hull",
                self.func.name
            );
            r
        });
        ranges.of(v)
    }
}

impl LiveRanges {
    /// The ranges of every value `intervals` knows, or that is a member
    /// of a class of `classes` other than its leader (the intervals of
    /// those are folded away). A parameter or an alloca result gets its
    /// hull as its single range.
    pub(crate) fn compute(
        func: &Function,
        blocks: &BlockLiveness,
        intervals: &HashMap<u32, Interval>,
        classes: &Classes,
    ) -> Self {
        let mut hull_only = vec![false; func.values.len()];
        for p in &func.params {
            hull_only[p.0 as usize] = true;
        }
        for inst in func.blocks.iter().flat_map(|b| &b.insts) {
            if let (InstKind::Alloca(_) | InstKind::AllocaBytes(_), Some(r)) =
                (&inst.kind, inst.result)
            {
                hull_only[r.0 as usize] = true;
            }
        }
        // which values the walk follows, per value index: the walk asks
        // once per operand, too often for a hash lookup
        let mut keep = vec![false; func.values.len()];
        for &v in intervals.keys() {
            keep[v as usize] = true;
        }
        for (leader, members) in classes.iter() {
            for &m in members.iter().filter(|&&m| m != leader) {
                keep[m as usize] = true;
            }
        }
        for (k, &only) in keep.iter_mut().zip(&hull_only) {
            *k &= !only;
        }
        // per value, the range being extended: a value's ranges arrive
        // in ascending order of start, so merging the overlapping and
        // adjacent ones needs no sort
        let mut open: Vec<(u32, u32)> = vec![(u32::MAX, 0); func.values.len()];
        let mut pairs: Vec<(u32, (u32, u32))> = Vec::new();
        walk_half_slots(
            func,
            blocks,
            |v| keep[v as usize],
            |_, block| {
                for &(v, lo, hi) in block.iter().rev() {
                    let c = &mut open[v as usize];
                    if c.0 != u32::MAX && lo <= c.1 + 1 {
                        c.1 = c.1.max(hi);
                    } else {
                        if c.0 != u32::MAX {
                            pairs.push((v, *c));
                        }
                        *c = (lo, hi);
                    }
                }
            },
        );
        for (v, c) in open.into_iter().enumerate() {
            if c.0 != u32::MAX {
                pairs.push((v as u32, c));
            }
        }
        for (v, _) in hull_only.iter().enumerate().filter(|(_, only)| **only) {
            if let Some(it) = intervals.get(&(v as u32)) {
                pairs.push((v as u32, (2 * it.start, 2 * it.end + 1)));
            }
        }
        Self::from_pairs(func.values.len(), &pairs)
    }

    /// Group `(value, range)` pairs by value; each value keeps the
    /// order its pairs come in.
    fn from_pairs(values: usize, pairs: &[(u32, (u32, u32))]) -> Self {
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
    /// their leaders: a leader's are its members'. A member keeps its
    /// own; the sweep only asks for the leader's.
    pub(crate) fn fold(mut self, classes: &Classes) -> Self {
        for (leader, members) in classes.iter() {
            let first = self.ranges.len();
            let mut own: Vec<(u32, u32)> =
                members.iter().flat_map(|&m| self.of(m)).copied().collect();
            own.sort_unstable();
            for (lo, hi) in own {
                match self.ranges.len() > first && lo <= self.ranges[self.ranges.len() - 1].1 + 1 {
                    true => {
                        let last = self.ranges.len() - 1;
                        self.ranges[last].1 = self.ranges[last].1.max(hi);
                    }
                    false => self.ranges.push((lo, hi)),
                }
            }
            self.spans[leader as usize] = (first as u32, self.ranges.len() as u32);
        }
        self
    }

    /// Whether every value's ranges are sorted, disjoint and inside its
    /// hull (a hull is in slots, a range in half-slots).
    pub(crate) fn within(&self, intervals: &HashMap<u32, Interval>) -> bool {
        intervals.iter().all(|(&v, it)| {
            let r = self.of(v);
            (r.iter()).all(|&(lo, hi)| it.start <= lo / 2 && lo <= hi && hi / 2 <= it.end)
                && r.windows(2).all(|w| w[0].1 + 1 < w[1].0)
        })
    }

    /// The ranges of `v`, ascending.
    pub(crate) fn of(&self, v: u32) -> &[(u32, u32)] {
        let (start, end) = self.spans[v as usize];
        &self.ranges[start as usize..end as usize]
    }

    /// Whether `ranges` hold a value across the instruction at slot `p`:
    /// live where it reads (`2p`) and where it writes (`2p + 1`). A
    /// value read last by the instruction, or defined by it, is not.
    pub(crate) fn live_across(ranges: &[(u32, u32)], p: u32) -> bool {
        let i = ranges.partition_point(|&(_, hi)| hi < 2 * p + 1);
        ranges.get(i).is_some_and(|&(lo, _)| lo <= 2 * p)
    }

    /// The sorted union of `a` and `b`, overlapping and adjacent ranges
    /// merged, without the ranges of `a` that end before half-slot
    /// `from`.
    pub(crate) fn union_from(a: &[(u32, u32)], b: &[(u32, u32)], from: u32) -> Vec<(u32, u32)> {
        let a = &a[a.partition_point(|&(_, hi)| hi < from)..];
        let mut out: Vec<(u32, u32)> = Vec::with_capacity(a.len() + b.len());
        let (mut i, mut j) = (0, 0);
        while i < a.len() || j < b.len() {
            let next = if j == b.len() || (i < a.len() && a[i] < b[j]) {
                i += 1;
                a[i - 1]
            } else {
                j += 1;
                b[j - 1]
            };
            match out.last_mut() {
                Some(last) if next.0 <= last.1 + 1 => last.1 = last.1.max(next.1),
                _ => out.push(next),
            }
        }
        out
    }

    /// Whether `a` and `b` share a half-slot.
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

#[cfg(test)]
mod tests;
