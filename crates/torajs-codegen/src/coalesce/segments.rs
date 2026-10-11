//! Half-slot live segments of the copy candidates, and the interference
//! test over them.

use torajs_core::ssa::Function;
use torajs_core::ssa::id_hash::{HashMap, HashSet};

use super::Segments;
use crate::liveness::{visit_inst_operands, visit_terminator_operands};
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

/// Half-slot ranges where each candidate holds a live value, from one
/// backward walk per block. A def with no later read still occupies
/// its write half: the register is clobbered there.
pub(super) fn live_segments(
    func: &Function,
    blocks: &BlockLiveness,
    cands: &HashSet<u32>,
) -> HashMap<u32, Segments> {
    let mut segs: HashMap<u32, Segments> = HashMap::default();
    for (b, block) in func.blocks.iter().enumerate() {
        let (bs, be) = (blocks.start[b], blocks.end[b]);
        // live candidate → end of its currently open segment
        let mut open: HashMap<u32, u32> = blocks.live_out[b]
            .iter()
            .filter(|v| cands.contains(v))
            .map(|v| (v, 2 * be + 1))
            .collect();
        visit_terminator_operands(&block.term, |v| {
            if cands.contains(&v.0) {
                open.entry(v.0).or_insert(2 * be);
            }
        });
        for (k, inst) in block.insts.iter().enumerate().rev() {
            let p = bs + k as u32;
            if let Some(r) = inst.result
                && cands.contains(&r.0)
            {
                let end = open.remove(&r.0).unwrap_or(2 * p + 1);
                segs.entry(r.0).or_default().push((2 * p + 1, end));
            }
            visit_inst_operands(&inst.kind, |v| {
                if cands.contains(&v.0) {
                    open.entry(v.0).or_insert(2 * p);
                }
            });
        }
        for (v, end) in open {
            segs.entry(v).or_default().push((2 * bs, end));
        }
    }
    segs
}
