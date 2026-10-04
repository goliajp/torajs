//! The dominated region a constraint is set over, and the blocks a
//! def inside it reaches.

use torajs_core::ssa::dominator::DominatorTree;
use torajs_core::ssa::{BlockId, Function};

use crate::branch_fold::point_eval::successors;

/// `a` and every block it dominates, `a` first.
pub(super) fn dom_subtree(dom: &DominatorTree, a: BlockId) -> Vec<BlockId> {
    let mut region = vec![a];
    let mut k = 0;
    while k < region.len() {
        region.extend_from_slice(dom.children(region[k]));
        k += 1;
    }
    region
}

/// Marks in `dead` every block reached from `work` without passing
/// `a`; returns the blocks it marked.
pub(super) fn mark_reached(
    func: &Function,
    a: BlockId,
    mut work: Vec<BlockId>,
    dead: &mut [bool],
) -> Vec<usize> {
    let mut touched: Vec<usize> = Vec::new();
    while let Some(s) = work.pop() {
        let si = s.0 as usize;
        if s == a || dead[si] {
            continue;
        }
        dead[si] = true;
        touched.push(si);
        work.extend(successors(&func.blocks[si].term));
    }
    touched
}
