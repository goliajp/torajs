//! Half-slot live segments and def sites of the copy candidates, and
//! the interference tests over them.

use torajs_core::ssa::Function;
use torajs_core::ssa::id_hash::{HashMap, HashSet};

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

/// Half-slot ranges where each candidate holds a live value.
pub(super) fn live_segments(
    func: &Function,
    blocks: &BlockLiveness,
    cands: &HashSet<u32>,
) -> HashMap<u32, Segments> {
    let mut segs: HashMap<u32, Segments> = HashMap::default();
    // the walk asks once per operand and live-out member
    let mut keep = vec![false; func.values.len()];
    for &v in cands {
        keep[v as usize] = true;
    }
    walk_half_slots(
        func,
        blocks,
        |v| keep[v as usize],
        |_, block| {
            for &(v, lo, hi) in block {
                segs.entry(v).or_default().push((lo, hi));
            }
        },
    );
    segs
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

/// Write half-slots of every def of the candidates.
pub(super) fn def_sites(
    func: &Function,
    blocks: &BlockLiveness,
    cands: &HashSet<u32>,
) -> HashMap<u32, Vec<u32>> {
    let mut defs: HashMap<u32, Vec<u32>> = HashMap::default();
    for (b, block) in func.blocks.iter().enumerate() {
        for (k, inst) in block.insts.iter().enumerate() {
            if let Some(r) = inst.result
                && cands.contains(&r.0)
            {
                defs.entry(r.0)
                    .or_default()
                    .push(2 * (blocks.start[b] + k as u32) + 1);
            }
        }
    }
    defs
}
