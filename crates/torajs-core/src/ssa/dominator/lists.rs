//! The tree's per-block lists and its depth-first numbering.

use super::targets;
use crate::ssa::{BlockId, Function};

/// Per-block lists of blocks, all in one buffer: block `i`'s list is
/// `list[start[i]..start[i + 1]]`.
#[derive(Debug, Clone)]
pub(crate) struct Adjacency {
    start: Vec<u32>,
    list: Vec<BlockId>,
}

/// An [`Adjacency`] built in two walks over the same `(owner, member)`
/// pairs: [`count`](Self::count) each pair, then [`fill`](Self::fill)
/// them in the order each owner's list keeps. The walks are plain
/// loops: an iterator chain over the terminators cost several times
/// more per block.
pub(super) struct Counted {
    start: Vec<u32>,
}

pub(super) struct Filling {
    start: Vec<u32>,
    fill: Vec<u32>,
    list: Vec<BlockId>,
}

impl Counted {
    pub(super) fn new(n: usize) -> Self {
        Counted {
            start: vec![0u32; n + 1],
        }
    }

    pub(super) fn count(&mut self, owner: usize) {
        self.start[owner + 1] += 1;
    }

    pub(super) fn into_filling(mut self) -> Filling {
        for i in 1..self.start.len() {
            self.start[i] += self.start[i - 1];
        }
        let total = self.start[self.start.len() - 1] as usize;
        Filling {
            fill: self.start.clone(),
            start: self.start,
            list: vec![BlockId(0); total],
        }
    }
}

impl Filling {
    pub(super) fn fill(&mut self, owner: usize, member: BlockId) {
        let at = &mut self.fill[owner];
        self.list[*at as usize] = member;
        *at += 1;
    }

    pub(super) fn done(self) -> Adjacency {
        Adjacency {
            start: self.start,
            list: self.list,
        }
    }
}

impl Adjacency {
    pub(super) fn empty() -> Self {
        Counted::new(0).into_filling().done()
    }

    pub(crate) fn of(&self, i: usize) -> &[BlockId] {
        match (self.start.get(i), self.start.get(i + 1)) {
            (Some(&a), Some(&b)) => &self.list[a as usize..b as usize],
            _ => &[],
        }
    }
}

/// The predecessor blocks of each block, in layout order. Computed by
/// walking every block's terminator twice (count, then fill).
pub(crate) fn predecessors(func: &Function, n: usize) -> Adjacency {
    let mut counted = Counted::new(n);
    for blk in &func.blocks {
        let (ts, k) = targets(&blk.term);
        for t in &ts[..k] {
            if (t.0 as usize) < n {
                counted.count(t.0 as usize);
            }
        }
    }
    let mut filling = counted.into_filling();
    for blk in &func.blocks {
        let (ts, k) = targets(&blk.term);
        for t in &ts[..k] {
            if (t.0 as usize) < n {
                filling.fill(t.0 as usize, blk.id);
            }
        }
    }
    filling.done()
}

/// Reverse the `idom` table into a parent → children adjacency list.
/// The CHK convention makes the entry block its own immediate
/// dominator; that self-loop is suppressed so the entry is not listed
/// as its own child. Unreachable blocks (idom = `None`) contribute no
/// edge. Result is keyed by block index (matching `idom.len()`).
pub(super) fn build_children(idom: &[Option<BlockId>]) -> Adjacency {
    let n = idom.len();
    let parent = |i: usize| match idom[i] {
        Some(p) if p.0 as usize != i && (p.0 as usize) < n => Some(p.0 as usize),
        _ => None,
    };
    let mut counted = Counted::new(n);
    for i in 0..n {
        if let Some(p) = parent(i) {
            counted.count(p);
        }
    }
    let mut filling = counted.into_filling();
    for i in 0..n {
        if let Some(p) = parent(i) {
            filling.fill(p, BlockId(i as u32));
        }
    }
    filling.done()
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
