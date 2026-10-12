//! Half-slot live segments and def sites of the copy candidates, and
//! the interference tests over them.

use torajs_core::ssa::Function;

use super::Segments;
use crate::live_ranges::walk_half_slots;
use crate::liveness_blocks::BlockLiveness;

pub(super) fn overlaps(a: &Segments, b: &Segments) -> bool {
    let (mut i, mut j) = (0, 0);
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

/// The candidates' live segments and def sites, indexed like the
/// candidates: `slot[v]` is value `v`'s candidate index, `NONE` for a
/// value that is not one.
pub(super) struct Live {
    /// Half-slot ranges where the candidate holds a live value,
    /// ascending.
    pub(super) segs: Vec<Segments>,
    /// Write half-slots of the candidate's defs, ascending.
    pub(super) defs: Vec<Vec<u32>>,
}

pub(super) const NONE: u32 = u32::MAX;

/// One backward walk gives both: every def of a candidate opens a range
/// at its write half-slot `2p + 1`, the only odd start a range can have
/// (a range live into a block starts at the block's read half-slot).
/// The walk hands each block's ranges in descending order of start and
/// the blocks in ascending slot order, so reading each block backward
/// leaves every list ascending.
pub(super) fn live_segments(
    func: &Function,
    blocks: &BlockLiveness,
    slot: &[u32],
    cands: usize,
) -> Live {
    let mut live = Live {
        segs: vec![Vec::new(); cands],
        defs: vec![Vec::new(); cands],
    };
    walk_half_slots(
        func,
        blocks,
        |v| slot[v as usize] != NONE,
        |_, block| {
            for &(v, lo, hi) in block.iter().rev() {
                let i = slot[v as usize] as usize;
                live.segs[i].push((lo, hi));
                if lo % 2 == 1 {
                    live.defs[i].push(lo);
                }
            }
        },
    );
    live
}

/// Whether some position of `points` (ascending) falls inside a range
/// of `segs` (sorted, disjoint).
pub(super) fn any_inside(points: &[u32], segs: &Segments) -> bool {
    let mut j = 0;
    for &p in points {
        while j < segs.len() && segs[j].1 < p {
            j += 1;
        }
        if j < segs.len() && segs[j].0 <= p {
            return true;
        }
    }
    false
}

/// The sorted union of two segment lists, overlapping and adjacent
/// ranges merged.
pub(super) fn union(a: &Segments, b: &Segments) -> Segments {
    let mut out: Segments = Vec::with_capacity(a.len() + b.len());
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
