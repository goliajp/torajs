//! Half-slot live segments of the copy candidates, and the interference
//! test over them.

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
