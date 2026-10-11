//! The backward walk per block that both copy coalescing and the live
//! ranges read their half-slot ranges from.

use torajs_core::ssa::Function;

use crate::liveness::{visit_inst_operands, visit_terminator_operands};
use crate::liveness_blocks::BlockLiveness;

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
