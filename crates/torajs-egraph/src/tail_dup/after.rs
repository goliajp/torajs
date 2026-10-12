//! The dominators and loops of a function after tail duplication, read
//! off the ones before it instead of computed again.
//!
//! Copying a tail moves an arm's edge from the join to a chain of
//! copies that ends in the same back edge the tail ends in. A path that
//! ran through the original tail runs through a copy instead, so no
//! block outside the copied tails gains or loses a dominator, and no
//! loop gains or loses a block other than the copies. A copy's first
//! block is entered from its arm only, and each later one from the
//! block before it; a tail block is dominated by what dominates all
//! the edges it still has.

use super::{Copied, targets};
use crate::block_chain::{BlockSet, DomView};

use torajs_core::ssa::dominator::DominatorTree;
use torajs_core::ssa::{BlockId, Function};

/// The new immediate dominator of every block copying gave one, by
/// block id: each copied block and each block of a copied tail. `None`
/// when a tail block has a predecessor the old tree's reverse postorder
/// meets after it, which only an irreducible graph has, or when an arm
/// that got a copy is unreachable (a loop body holds the unreachable
/// blocks that branch into it, and their copies would be unreachable
/// too); the caller then computes the tree afresh.
///
/// Only edges from blocks the old tree reaches count, as only those
/// count in a tree computed afresh: copying leaves every original
/// block as reachable as it was, and no copy branches into a tail.
pub(crate) fn idoms_after(
    func: &Function,
    dom: &DominatorTree,
    copied: &Copied,
) -> Option<Vec<Option<u32>>> {
    let n = func.blocks.len();
    let mut idoms: Vec<Option<u32>> = vec![None; n];
    let mut in_tail = vec![false; n];
    for join in &copied.joins {
        for &t in &join.tail {
            in_tail[t as usize] = true;
        }
        for &(arm, first) in &join.copies {
            if !dom.is_reachable(BlockId(arm)) {
                return None;
            }
            idoms[first as usize] = Some(arm);
            for k in 1..join.tail.len() as u32 {
                idoms[(first + k) as usize] = Some(first + k - 1);
            }
        }
    }
    // the edges into tail blocks; a copy never branches into one
    let mut into: Vec<(u32, u32)> = Vec::new();
    for b in func.blocks.iter().filter(|b| dom.is_reachable(b.id)) {
        for t in targets(&b.term).filter(|&t| in_tail[t as usize]) {
            into.push((t, b.id.0));
        }
    }
    let mut settled = vec![false; n];
    for &b in dom.reverse_postorder() {
        if !in_tail[b.0 as usize] {
            continue;
        }
        let view = DomView::patched(dom, &idoms);
        let mut idom: Option<BlockId> = None;
        for &(_, p) in into.iter().filter(|e| e.0 == b.0) {
            if in_tail[p as usize] && !settled[p as usize] {
                return None;
            }
            idom = match idom {
                None => Some(BlockId(p)),
                Some(q) => Some(view.common_dominator(BlockId(p), q)?),
            };
        }
        idoms[b.0 as usize] = Some(idom?.0);
        settled[b.0 as usize] = true;
    }
    Some(idoms)
}

/// Put the copies into the loops, given as (body, header, back-edge
/// sources) the way they were before copying: a copy belongs to every
/// loop that holds its join's loop header, and its last block is one
/// more back-edge source of that header's loop. Copies come after
/// every original, so the sources stay in id order.
pub(crate) fn add_to_loops(loops: &mut [(BlockSet, u32, Vec<u32>)], copied: &Copied) {
    for join in &copied.joins {
        let k = join.tail.len() as u32;
        for &(_, first) in &join.copies {
            for (body, header, latches) in loops.iter_mut() {
                if !body.contains(join.header) {
                    continue;
                }
                for b in first..first + k {
                    body.insert(b);
                }
                if *header == join.header {
                    latches.push(first + k - 1);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
