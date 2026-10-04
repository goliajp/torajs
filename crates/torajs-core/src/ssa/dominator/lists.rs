//! The tree's per-block lists and its depth-first numbering.

use super::successors;
use crate::ssa::{BlockId, Function};

/// Per-block lists of blocks, all in one buffer: block `i`'s list is
/// `list[start[i]..start[i + 1]]`.
#[derive(Debug, Clone)]
pub(crate) struct Adjacency {
    start: Vec<u32>,
    list: Vec<BlockId>,
}

impl Adjacency {
    /// From `(owner, member)` pairs; each owner's list keeps the order
    /// the pairs give it.
    pub(super) fn from_pairs(n: usize, pairs: &[(usize, BlockId)]) -> Self {
        let mut start = vec![0u32; n + 1];
        for &(o, _) in pairs {
            start[o + 1] += 1;
        }
        for i in 0..n {
            start[i + 1] += start[i];
        }
        let mut fill = start.clone();
        let mut list = vec![BlockId(0); pairs.len()];
        for &(o, m) in pairs {
            list[fill[o] as usize] = m;
            fill[o] += 1;
        }
        Adjacency { start, list }
    }

    pub(crate) fn of(&self, i: usize) -> &[BlockId] {
        match (self.start.get(i), self.start.get(i + 1)) {
            (Some(&a), Some(&b)) => &self.list[a as usize..b as usize],
            _ => &[],
        }
    }
}

/// The predecessor blocks of each block, in layout order. Computed by
/// walking every block's terminator once.
pub(crate) fn predecessors(func: &Function, n: usize) -> Adjacency {
    let mut pairs: Vec<(usize, BlockId)> = Vec::with_capacity(func.blocks.len() * 2);
    for blk in &func.blocks {
        for succ in successors(&blk.term) {
            let si = succ.0 as usize;
            if si < n {
                pairs.push((si, blk.id));
            }
        }
    }
    Adjacency::from_pairs(n, &pairs)
}

/// Reverse the `idom` table into a parent → children adjacency list.
/// The CHK convention makes the entry block its own immediate
/// dominator; that self-loop is suppressed so the entry is not listed
/// as its own child. Unreachable blocks (idom = `None`) contribute no
/// edge. Result is keyed by block index (matching `idom.len()`).
pub(super) fn build_children(idom: &[Option<BlockId>]) -> Adjacency {
    let n = idom.len();
    let mut pairs: Vec<(usize, BlockId)> = Vec::with_capacity(n);
    for (i, parent) in idom.iter().enumerate() {
        if let Some(p) = parent {
            let pi = p.0 as usize;
            if pi != i && pi < n {
                pairs.push((pi, BlockId(i as u32)));
            }
        }
    }
    Adjacency::from_pairs(n, &pairs)
}

/// Depth-first entry and exit times over the tree rooted at `entry`.
pub(super) fn tree_intervals(
    children: &Adjacency,
    entry: BlockId,
    n: usize,
) -> (Vec<u32>, Vec<u32>) {
    let mut enter = vec![u32::MAX; n];
    let mut exit = vec![u32::MAX; n];
    let mut clock = 0u32;
    // (block, index of the next child to visit)
    let mut stack: Vec<(usize, usize)> = vec![(entry.0 as usize, 0)];
    enter[entry.0 as usize] = clock;
    while let Some(top) = stack.last_mut() {
        let (b, k) = *top;
        match children.of(b).get(k) {
            Some(c) => {
                top.1 += 1;
                clock += 1;
                enter[c.0 as usize] = clock;
                stack.push((c.0 as usize, 0));
            }
            None => {
                clock += 1;
                exit[b] = clock;
                stack.pop();
            }
        }
    }
    (enter, exit)
}
