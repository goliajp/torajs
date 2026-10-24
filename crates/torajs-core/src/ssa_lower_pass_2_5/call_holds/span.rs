//! The blocks a hold spans.

use crate::ssa::{BlockId, Function, Terminator};

/// The predecessors of each block of `f`.
pub(super) fn preds(f: &Function) -> Vec<Vec<usize>> {
    let mut preds: Vec<Vec<usize>> = vec![Vec::new(); f.blocks.len()];
    for (b, blk) in f.blocks.iter().enumerate() {
        for s in successors(&blk.term) {
            preds[s].push(b);
        }
    }
    preds
}

/// The blocks of `f` on a path from `start` to `end` that passes
/// neither again, both included, leaving out the pieces `skip` marks;
/// `None` when no such path reaches `end`. `preds` are `f`'s.
pub(super) fn span(
    f: &Function,
    preds: &[Vec<usize>],
    skip: &[bool],
    (start, end): (BlockId, BlockId),
) -> Option<Vec<usize>> {
    let n = f.blocks.len();
    let ahead = walk(start.0 as usize, end.0 as usize, n, |b| {
        successors(&f.blocks[b].term)
            .filter(|s| !skip[*s])
            .collect()
    });
    if !ahead[end.0 as usize] {
        return None;
    }
    let behind = walk(end.0 as usize, start.0 as usize, n, |b| {
        preds[b].iter().copied().filter(|p| !skip[*p]).collect()
    });
    Some((0..n).filter(|b| ahead[*b] && behind[*b]).collect())
}

/// The blocks reachable from `from` along `next`, not going on past
/// `stop`.
fn walk(from: usize, stop: usize, n: usize, next: impl Fn(usize) -> Vec<usize>) -> Vec<bool> {
    let mut seen = vec![false; n];
    seen[from] = true;
    let mut work = vec![from];
    while let Some(b) = work.pop() {
        if b == stop {
            continue;
        }
        for s in next(b) {
            if !seen[s] {
                seen[s] = true;
                work.push(s);
            }
        }
    }
    seen
}

pub(super) fn successors(t: &Terminator) -> impl Iterator<Item = usize> {
    let (a, b) = match t {
        Terminator::Br(x) => (Some(x.0 as usize), None),
        Terminator::CondBr {
            then_blk, else_blk, ..
        } => (Some(then_blk.0 as usize), Some(else_blk.0 as usize)),
        Terminator::Ret(_) | Terminator::Unreachable => (None, None),
    };
    a.into_iter().chain(b)
}
