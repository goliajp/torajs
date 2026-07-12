//! Code-point range set — the v-flag (unicodeSets) class algebra.
//!
//! RFC 20260712 chunk B1. A `ClassSetExpression` under the `v` flag
//! composes operands with union (adjacency), intersection (`&&`) and
//! difference (`--`); operands are single characters, ranges, class
//! escapes, `\p{}` properties and nested classes. This module holds
//! the set representation those operators run on: an owned, sorted,
//! disjoint list of inclusive cp ranges over `0..=0x10FFFF`.
//!
//! The parser (`parser/class_v.rs`) folds a finished set into a
//! [`crate::charclass::CharClass`] — `cp < 0x100` into the byte
//! bitmap, the rest into `CharClass::owned_ranges` — so the matcher
//! core (`test_cp` shape) is untouched by v-mode.

use alloc::vec::Vec;

pub const CP_MAX: u32 = 0x10_FFFF;

/// Sorted, disjoint, non-adjacent inclusive cp ranges.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CpRangeSet {
    ranges: Vec<(u32, u32)>,
}

impl CpRangeSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.ranges.is_empty()
    }

    pub fn ranges(&self) -> &[(u32, u32)] {
        &self.ranges
    }

    pub fn contains(&self, cp: u32) -> bool {
        self.ranges
            .binary_search_by(|&(lo, hi)| {
                if cp < lo {
                    core::cmp::Ordering::Greater
                } else if cp > hi {
                    core::cmp::Ordering::Less
                } else {
                    core::cmp::Ordering::Equal
                }
            })
            .is_ok()
    }

    /// Insert one inclusive range, merging overlaps/adjacency.
    pub fn insert(&mut self, lo: u32, hi: u32) {
        debug_assert!(lo <= hi && hi <= CP_MAX);
        // Find the insertion window: every existing range that
        // overlaps or is adjacent to [lo, hi] merges into it.
        let mut new_lo = lo;
        let mut new_hi = hi;
        let mut i = 0;
        let mut out: Vec<(u32, u32)> = Vec::with_capacity(self.ranges.len() + 1);
        while i < self.ranges.len() && self.ranges[i].1 + 1 < lo.max(1) {
            out.push(self.ranges[i]);
            i += 1;
        }
        while i < self.ranges.len() && self.ranges[i].0 <= hi.saturating_add(1) {
            new_lo = new_lo.min(self.ranges[i].0);
            new_hi = new_hi.max(self.ranges[i].1);
            i += 1;
        }
        out.push((new_lo, new_hi));
        out.extend_from_slice(&self.ranges[i..]);
        self.ranges = out;
    }

    pub fn insert_cp(&mut self, cp: u32) {
        self.insert(cp, cp);
    }

    pub fn union(&self, other: &Self) -> Self {
        let mut out = self.clone();
        for &(lo, hi) in &other.ranges {
            out.insert(lo, hi);
        }
        out
    }

    pub fn intersect(&self, other: &Self) -> Self {
        let mut out = Vec::new();
        let (mut i, mut j) = (0, 0);
        while i < self.ranges.len() && j < other.ranges.len() {
            let (a_lo, a_hi) = self.ranges[i];
            let (b_lo, b_hi) = other.ranges[j];
            let lo = a_lo.max(b_lo);
            let hi = a_hi.min(b_hi);
            if lo <= hi {
                out.push((lo, hi));
            }
            if a_hi < b_hi {
                i += 1;
            } else {
                j += 1;
            }
        }
        Self { ranges: out }
    }

    pub fn difference(&self, other: &Self) -> Self {
        self.intersect(&other.complement())
    }

    /// Complement over the full cp domain `0..=CP_MAX`.
    pub fn complement(&self) -> Self {
        let mut out = Vec::with_capacity(self.ranges.len() + 1);
        let mut next = 0u32;
        for &(lo, hi) in &self.ranges {
            if lo > next {
                out.push((next, lo - 1));
            }
            next = hi + 1;
            if next > CP_MAX {
                return Self { ranges: out };
            }
        }
        out.push((next, CP_MAX));
        Self { ranges: out }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn set(rs: &[(u32, u32)]) -> CpRangeSet {
        let mut s = CpRangeSet::new();
        for &(lo, hi) in rs {
            s.insert(lo, hi);
        }
        s
    }

    #[test]
    fn insert_merges_overlap_and_adjacency() {
        let mut s = CpRangeSet::new();
        s.insert(10, 20);
        s.insert(30, 40);
        s.insert(21, 29); // bridges both
        assert_eq!(s.ranges(), &[(10, 40)]);
        s.insert(5, 9); // adjacent below
        assert_eq!(s.ranges(), &[(5, 40)]);
        s.insert(100, 200);
        assert_eq!(s.ranges(), &[(5, 40), (100, 200)]);
    }

    #[test]
    fn insert_at_zero_boundary() {
        let mut s = CpRangeSet::new();
        s.insert(0, 0);
        s.insert(2, 3);
        assert_eq!(s.ranges(), &[(0, 0), (2, 3)]);
        s.insert(1, 1);
        assert_eq!(s.ranges(), &[(0, 3)]);
    }

    #[test]
    fn contains_binary_search() {
        let s = set(&[(10, 20), (0x1F600, 0x1F64F)]);
        assert!(s.contains(10) && s.contains(20) && s.contains(0x1F600));
        assert!(!s.contains(9) && !s.contains(21) && !s.contains(0x1F5FF));
    }

    #[test]
    fn union_intersect_difference() {
        let a = set(&[(0, 9), (20, 29)]);
        let b = set(&[(5, 24)]);
        assert_eq!(a.union(&b).ranges(), &[(0, 29)]);
        assert_eq!(a.intersect(&b).ranges(), &[(5, 9), (20, 24)]);
        assert_eq!(a.difference(&b).ranges(), &[(0, 4), (25, 29)]);
        assert_eq!(b.difference(&a).ranges(), &[(10, 19)]);
    }

    #[test]
    fn complement_round_trips() {
        let a = set(&[(0, 9), (0x100, 0x1FF)]);
        let c = a.complement();
        assert_eq!(c.ranges(), &[(10, 0xFF), (0x200, CP_MAX)]);
        assert_eq!(c.complement(), a);
        // Full / empty domain.
        assert_eq!(CpRangeSet::new().complement().ranges(), &[(0, CP_MAX)]);
        assert!(set(&[(0, CP_MAX)]).complement().is_empty());
    }

    #[test]
    fn intersect_empty_is_empty() {
        let a = set(&[(0, 9)]);
        assert!(a.intersect(&CpRangeSet::new()).is_empty());
        assert_eq!(a.difference(&CpRangeSet::new()), a);
    }

    #[test]
    fn insert_lo_zero_adjacency_guard() {
        // lo = 0: the `hi + 1 < lo.max(1)` skip-window must not
        // underflow or skip a range adjacent to 0.
        let mut s = set(&[(1, 5)]);
        s.insert(0, 0);
        assert_eq!(s.ranges(), &[(0, 5)]);
        let mut s = vec![(3u32, 5u32)]
            .into_iter()
            .fold(CpRangeSet::new(), |mut s, (l, h)| {
                s.insert(l, h);
                s
            });
        s.insert(0, 1);
        assert_eq!(s.ranges(), &[(0, 1), (3, 5)]);
    }
}
