//! Dominator tree for the SSA CFG — Cooper, Harvey, Kennedy (2001):
//! "A Simple, Fast Dominance Algorithm". The reference for production
//! SSA compilers (LLVM `lib/Support/GenericDomTreeConstruction.h` and
//! Cranelift `cranelift/codegen/src/dominator_tree.rs` both ship this).
//!
//! Why this matters for the egraph optimizer: scoped elaboration walks
//! the domtree in preorder, pushing/popping a `ScopedHashMap` per scope.
//! That single traversal pattern is what makes GVN, CSE, and LICM fall
//! out automatically — values computed in a dominator are reused in
//! every descendant block, while siblings see independent values.
//!
//! Complexity: O(N · α(N)) almost-linear in number of edges, with the
//! tight inner loop being `intersect` (climbing two RPO-numbered
//! finger pointers until they meet). Good enough for whole-function
//! optimization; significantly faster than Lengauer-Tarjan for the
//! function sizes torajs sees in practice (≤ a few thousand blocks).

use crate::ssa::{BlockId, Function, Terminator};

mod lists;

pub(crate) use lists::{Adjacency, predecessors};
use lists::{build_children, tree_intervals};

/// Immediate-dominator table + reverse-postorder traversal of the CFG.
///
/// Built once per function and consumed by both the optimize phase
/// (`OptimizeCtx`'s scoped-GVN walk) and the elaboration phase
/// (`Elaborator`'s domtree preorder visit).
#[derive(Debug, Clone)]
pub struct DominatorTree {
    /// `idom[i]` = the immediate dominator of block `i`, or `None` if
    /// block `i` is unreachable from the entry. The entry block's
    /// idom is itself (Cooper-Harvey-Kennedy convention) so that
    /// `intersect` terminates.
    idom: Vec<Option<BlockId>>,
    /// Reverse postorder of reachable blocks — entry first, then each
    /// block before any of its CFG successors when no back-edge is
    /// present. Required input ordering for `compute_idoms`'s
    /// iteration; also useful to callers needing a forward traversal
    /// order. Length matches the number of *reachable* blocks (may be
    /// less than `function.blocks.len()` if unreachable code exists).
    rpo: Vec<BlockId>,
    /// Position of each `BlockId.0` in `rpo`, or `usize::MAX` if the
    /// block was unreachable. Used by `intersect` to climb finger
    /// pointers in O(1) per step.
    rpo_index: Vec<usize>,
    /// Reverse map of `idom`: `children[i]` = blocks whose immediate
    /// dominator is block `i`. The entry block's CHK self-loop is
    /// suppressed (entry is not listed as its own child). Empty
    /// `Vec` for leaf / unreachable blocks. Drives domtree preorder
    /// DFS in scoped passes (e.g. `optimize::remove_pure_and_optimize`'s
    /// dom-nested GVN walk) so values inserted in a dominator are
    /// visible across every descendant subtree while siblings stay
    /// isolated.
    children: Adjacency,
    /// Entry and exit times of each reachable block in a depth-first
    /// walk of the tree: `a` is an ancestor of `b` exactly when
    /// `a`'s interval encloses `b`'s. `u32::MAX` for unreachable blocks.
    enter: Vec<u32>,
    exit: Vec<u32>,
}

impl DominatorTree {
    /// Compute the domtree for `func`. The entry is the first block in
    /// layout order, whatever its id; if the function has no blocks
    /// this returns an empty tree (every query returns `None`).
    pub fn compute(func: &Function) -> Self {
        let n = id_span(func);
        if n == 0 {
            return Self {
                idom: Vec::new(),
                rpo: Vec::new(),
                rpo_index: Vec::new(),
                children: Adjacency::empty(),
                enter: Vec::new(),
                exit: Vec::new(),
            };
        }
        let entry = func.blocks[0].id;
        let succ = successor_table(func, n);
        let rpo = reverse_postorder(&succ, entry);
        let mut rpo_index = vec![usize::MAX; n];
        for (pos, blk) in rpo.iter().enumerate() {
            rpo_index[blk.0 as usize] = pos;
        }
        let preds = predecessors(func, n);
        let idom = compute_idoms(&rpo, &rpo_index, &preds);
        let children = build_children(&idom);
        let (enter, exit) = tree_intervals(&children, entry, n);
        Self {
            idom,
            rpo,
            rpo_index,
            children,
            enter,
            exit,
        }
    }

    /// Immediate dominator of `b`, or `None` if `b` is the entry block
    /// or is unreachable. The entry's idom is conceptually itself in
    /// the CHK algorithm; this getter normalises it to `None` so the
    /// caller sees a clean tree root.
    pub fn immediate_dominator(&self, b: BlockId) -> Option<BlockId> {
        let i = b.0 as usize;
        if i >= self.idom.len() {
            return None;
        }
        match self.idom[i] {
            Some(d) if d == b => None, // entry's self-loop normalised
            other => other,
        }
    }

    /// Reverse postorder traversal of reachable blocks. Entry first.
    pub fn reverse_postorder(&self) -> &[BlockId] {
        &self.rpo
    }

    /// True iff every path from entry to `b` passes through `a`. By
    /// convention every block dominates itself.
    pub fn dominates(&self, a: BlockId, b: BlockId) -> bool {
        if a == b {
            return true;
        }
        // an unreachable block has no interval, so it neither
        // dominates nor is dominated
        match (self.enter.get(a.0 as usize), self.enter.get(b.0 as usize)) {
            (Some(&ea), Some(&eb)) if ea != u32::MAX && eb != u32::MAX => {
                ea < eb && self.exit[b.0 as usize] < self.exit[a.0 as usize]
            }
            _ => false,
        }
    }

    /// True iff `b` is reachable from the entry block.
    pub fn is_reachable(&self, b: BlockId) -> bool {
        let i = b.0 as usize;
        i < self.rpo_index.len() && self.rpo_index[i] != usize::MAX
    }

    /// Blocks whose immediate dominator is `b` — i.e. the children of
    /// `b` in the dominator tree. Returns `&[]` for leaves and for any
    /// out-of-range / unreachable block. Drives domtree preorder DFS
    /// in scoped analyses that need dom-nested visibility (entries
    /// inserted while visiting `b` reachable to every descendant via
    /// recursion, isolated from siblings via push/pop on the visit
    /// boundary).
    pub fn children(&self, b: BlockId) -> &[BlockId] {
        self.children.of(b.0 as usize)
    }
}

// ---- algorithm internals --------------------------------------------------

/// The blocks `term` branches to, `then` before `else`: `ts[..k]` of
/// `(ts, k)`. 0 (`Ret` / `Unreachable`), 1 (`Br`), or 2 (`CondBr`).
pub(crate) fn targets(term: &Terminator) -> ([BlockId; 2], usize) {
    match term {
        Terminator::Br(b) => ([*b, *b], 1),
        Terminator::CondBr {
            then_blk, else_blk, ..
        } => ([*then_blk, *else_blk], 2),
        Terminator::Ret(_) | Terminator::Unreachable => ([BlockId(0); 2], 0),
    }
}

/// No successor in a [`successor_table`] slot.
const NO: u32 = u32::MAX;

/// Each block's successors by id, `then` before `else`; [`NO`] in an
/// unused slot and for an id no block has. A target outside the id
/// span is left out.
fn successor_table(func: &Function, n: usize) -> Vec<[u32; 2]> {
    let mut succ = vec![[NO; 2]; n];
    for b in &func.blocks {
        let (ts, k) = targets(&b.term);
        let slot = &mut succ[b.id.0 as usize];
        for (j, t) in ts[..k].iter().enumerate() {
            if (t.0 as usize) < n {
                slot[j] = t.0;
            }
        }
    }
    succ
}

/// Reverse postorder = postorder, then reverse. Standard iterative DFS
/// with a "first visit / finalise" two-state marker on each block so
/// we don't recurse into already-being-explored cycles (the marker is
/// the top bit of the stacked id).
fn reverse_postorder(succ: &[[u32; 2]], entry: BlockId) -> Vec<BlockId> {
    const FINAL: u32 = 1 << 31;
    let n = succ.len();
    let mut postorder = Vec::with_capacity(n);
    let mut visited = vec![false; n];
    let mut stack: Vec<u32> = Vec::with_capacity(n);
    stack.push(entry.0);
    while let Some(top) = stack.pop() {
        if top & FINAL != 0 {
            postorder.push(BlockId(top & !FINAL));
            continue;
        }
        let i = top as usize;
        if visited[i] {
            continue;
        }
        visited[i] = true;
        // Schedule finalisation after children.
        stack.push(top | FINAL);
        for t in succ[i] {
            if t != NO && !visited[t as usize] {
                stack.push(t);
            }
        }
    }
    postorder.reverse();
    postorder
}

/// One past the largest block id: the length of every per-block
/// table. Passes may leave ids out of layout order or with gaps, so
/// blocks are looked up by id, never by position.
pub(crate) fn id_span(func: &Function) -> usize {
    func.blocks
        .iter()
        .map(|b| b.id.0 as usize + 1)
        .max()
        .unwrap_or(0)
}

/// Cooper-Harvey-Kennedy iterative-doms loop. Operates over the RPO
/// ordering so `intersect`'s finger walk converges quickly, and on
/// RPO positions rather than ids, so a finger compares and climbs
/// without a lookup.
fn compute_idoms(rpo: &[BlockId], rpo_index: &[usize], preds: &Adjacency) -> Vec<Option<BlockId>> {
    const UNSET: u32 = u32::MAX;
    // `doms[k]` = position of the idom of the block at position `k`.
    // The CHK convention sets the entry's idom to itself so
    // `intersect` can climb both fingers without a special root case.
    let mut doms = vec![UNSET; rpo.len()];
    doms[0] = 0;
    let mut changed = true;
    while changed {
        changed = false;
        // Process RPO order, skipping the entry (it's fixed).
        for (k, b) in rpo.iter().enumerate().skip(1) {
            let mut new_idom = UNSET;
            for p in preds.of(b.0 as usize) {
                let q = rpo_index[p.0 as usize];
                // unreachable, or not yet processed in this iteration
                if q == usize::MAX || doms[q] == UNSET {
                    continue;
                }
                new_idom = match new_idom {
                    UNSET => q as u32,
                    prev => intersect(q as u32, prev, &doms),
                };
            }
            if doms[k] != new_idom {
                doms[k] = new_idom;
                changed = true;
            }
        }
    }
    let mut idom = vec![None; rpo_index.len()];
    for (k, b) in rpo.iter().enumerate() {
        idom[b.0 as usize] = Some(rpo[doms[k] as usize]);
    }
    idom
}

/// Climb two RPO-position fingers until they meet — the meeting point
/// is the nearest common dominator. Higher = deeper in the CFG, so we
/// always advance the "deeper" finger toward the root. Every block a
/// finger reaches was processed: a processed block's idom sits at a
/// lower position (its DFS parent precedes it and is processed first
/// in each sweep), so the climb ends at the entry at the latest.
fn intersect(mut a: u32, mut b: u32, doms: &[u32]) -> u32 {
    while a != b {
        while a > b {
            a = doms[a as usize];
        }
        while b > a {
            b = doms[b as usize];
        }
    }
    a
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssa::{Block, Function, Operand, Terminator, Type};

    /// Build a Function with the given blocks (skipping all instruction
    /// details — domtree only inspects `Block.id` and `Block.term`).
    fn fixture(blocks: Vec<Block>) -> Function {
        Function {
            name: "test".to_string(),
            params: vec![],
            ret: Type::Void,
            blocks,
            values: vec![],
            current_origin: None,
        }
    }

    fn block(id: u32, term: Terminator) -> Block {
        Block {
            id: BlockId(id),
            insts: vec![],
            term,
        }
    }

    #[test]
    fn single_block_function() {
        let func = fixture(vec![block(0, Terminator::Ret(None))]);
        let dt = DominatorTree::compute(&func);
        assert_eq!(dt.reverse_postorder(), &[BlockId(0)]);
        // Entry's idom normalises to None even though the CHK
        // internal convention is self-loop.
        assert_eq!(dt.immediate_dominator(BlockId(0)), None);
        assert!(dt.dominates(BlockId(0), BlockId(0)));
        assert!(dt.is_reachable(BlockId(0)));
    }

    #[test]
    fn linear_chain_dominators() {
        // 0 -> 1 -> 2 -> 3 (ret)
        let func = fixture(vec![
            block(0, Terminator::Br(BlockId(1))),
            block(1, Terminator::Br(BlockId(2))),
            block(2, Terminator::Br(BlockId(3))),
            block(3, Terminator::Ret(None)),
        ]);
        let dt = DominatorTree::compute(&func);
        assert_eq!(
            dt.reverse_postorder(),
            &[BlockId(0), BlockId(1), BlockId(2), BlockId(3)]
        );
        assert_eq!(dt.immediate_dominator(BlockId(0)), None);
        assert_eq!(dt.immediate_dominator(BlockId(1)), Some(BlockId(0)));
        assert_eq!(dt.immediate_dominator(BlockId(2)), Some(BlockId(1)));
        assert_eq!(dt.immediate_dominator(BlockId(3)), Some(BlockId(2)));
        // Transitive dominance.
        assert!(dt.dominates(BlockId(0), BlockId(3)));
        assert!(dt.dominates(BlockId(1), BlockId(3)));
        assert!(!dt.dominates(BlockId(3), BlockId(0)));
        assert!(!dt.dominates(BlockId(2), BlockId(1)));
    }

    #[test]
    fn if_else_diamond() {
        // 0 (cond) -> 1, 2;  1 -> 3;  2 -> 3 (ret)
        let func = fixture(vec![
            block(
                0,
                Terminator::CondBr {
                    cond: Operand::ConstBool(true),
                    then_blk: BlockId(1),
                    else_blk: BlockId(2),
                },
            ),
            block(1, Terminator::Br(BlockId(3))),
            block(2, Terminator::Br(BlockId(3))),
            block(3, Terminator::Ret(None)),
        ]);
        let dt = DominatorTree::compute(&func);
        // 0 is entry; 1 and 2 each idom = 0; 3's idom is 0 (the join).
        assert_eq!(dt.immediate_dominator(BlockId(0)), None);
        assert_eq!(dt.immediate_dominator(BlockId(1)), Some(BlockId(0)));
        assert_eq!(dt.immediate_dominator(BlockId(2)), Some(BlockId(0)));
        assert_eq!(dt.immediate_dominator(BlockId(3)), Some(BlockId(0)));
        // 1 does NOT dominate 3 (paths through 2 skip 1).
        assert!(!dt.dominates(BlockId(1), BlockId(3)));
        assert!(!dt.dominates(BlockId(2), BlockId(3)));
        // 0 dominates everything reachable.
        for b in 0..4 {
            assert!(dt.dominates(BlockId(0), BlockId(b)));
        }
    }

    #[test]
    fn natural_loop_back_edge() {
        // 0 -> 1 (header); 1 (cond) -> 2 (body), 3 (exit); 2 -> 1 (back)
        let func = fixture(vec![
            block(0, Terminator::Br(BlockId(1))),
            block(
                1,
                Terminator::CondBr {
                    cond: Operand::ConstBool(true),
                    then_blk: BlockId(2),
                    else_blk: BlockId(3),
                },
            ),
            block(2, Terminator::Br(BlockId(1))),
            block(3, Terminator::Ret(None)),
        ]);
        let dt = DominatorTree::compute(&func);
        assert_eq!(dt.immediate_dominator(BlockId(1)), Some(BlockId(0)));
        // Header dominates body and exit (loop structure → header
        // is the only entry to both 2 and 3).
        assert_eq!(dt.immediate_dominator(BlockId(2)), Some(BlockId(1)));
        assert_eq!(dt.immediate_dominator(BlockId(3)), Some(BlockId(1)));
        assert!(dt.dominates(BlockId(1), BlockId(2)));
        assert!(dt.dominates(BlockId(1), BlockId(3)));
    }

    #[test]
    fn unreachable_block_marked() {
        // 0 -> 1 (ret); 2 is unreachable from entry
        let func = fixture(vec![
            block(0, Terminator::Br(BlockId(1))),
            block(1, Terminator::Ret(None)),
            block(2, Terminator::Ret(None)),
        ]);
        let dt = DominatorTree::compute(&func);
        assert!(dt.is_reachable(BlockId(0)));
        assert!(dt.is_reachable(BlockId(1)));
        assert!(!dt.is_reachable(BlockId(2)));
        assert_eq!(dt.immediate_dominator(BlockId(2)), None);
        // Unreachable block is not dominated by anyone reachable.
        assert!(!dt.dominates(BlockId(0), BlockId(2)));
    }

    #[test]
    fn rpo_visits_entry_first_then_children() {
        // 0 (cond) -> 1, 2; both -> 3 (ret)
        let func = fixture(vec![
            block(
                0,
                Terminator::CondBr {
                    cond: Operand::ConstBool(true),
                    then_blk: BlockId(1),
                    else_blk: BlockId(2),
                },
            ),
            block(1, Terminator::Br(BlockId(3))),
            block(2, Terminator::Br(BlockId(3))),
            block(3, Terminator::Ret(None)),
        ]);
        let dt = DominatorTree::compute(&func);
        let rpo = dt.reverse_postorder();
        assert_eq!(rpo[0], BlockId(0), "entry must come first");
        assert_eq!(rpo[rpo.len() - 1], BlockId(3), "join must come last");
        // Position of 1 and 2 is impl-defined (depends on DFS child
        // order), but both must precede 3 and follow 0.
        let pos = |b: BlockId| rpo.iter().position(|x| *x == b).unwrap();
        assert!(pos(BlockId(0)) < pos(BlockId(1)));
        assert!(pos(BlockId(0)) < pos(BlockId(2)));
        assert!(pos(BlockId(1)) < pos(BlockId(3)));
        assert!(pos(BlockId(2)) < pos(BlockId(3)));
    }

    #[test]
    fn empty_function() {
        let func = fixture(vec![]);
        let dt = DominatorTree::compute(&func);
        assert!(dt.reverse_postorder().is_empty());
        assert_eq!(dt.immediate_dominator(BlockId(0)), None);
        assert!(!dt.is_reachable(BlockId(0)));
        // Out-of-range children query degrades to empty slice rather
        // than panicking — domtree consumers can call children on any
        // BlockId without prior bounds checking.
        assert!(dt.children(BlockId(0)).is_empty());
    }

    #[test]
    fn children_reverse_idom_with_entry_self_loop_suppressed() {
        // diamond: 0 (cond) → 1, 2; both → 3 (ret). idom is:
        //   0: self-loop (entry, CHK convention)
        //   1: 0
        //   2: 0
        //   3: 0  (the immediate post-dominator join)
        // children inverse:
        //   0: [1, 2, 3]   ← self-loop NOT included
        //   1: []   2: []   3: []
        let func = fixture(vec![
            block(
                0,
                Terminator::CondBr {
                    cond: Operand::ConstBool(true),
                    then_blk: BlockId(1),
                    else_blk: BlockId(2),
                },
            ),
            block(1, Terminator::Br(BlockId(3))),
            block(2, Terminator::Br(BlockId(3))),
            block(3, Terminator::Ret(None)),
        ]);
        let dt = DominatorTree::compute(&func);
        let mut kids: Vec<u32> = dt.children(BlockId(0)).iter().map(|b| b.0).collect();
        kids.sort();
        assert_eq!(kids, vec![1, 2, 3]);
        // Entry's self-loop must NOT leak into `children[0]`.
        assert!(!dt.children(BlockId(0)).contains(&BlockId(0)));
        // Leaves stay empty.
        assert!(dt.children(BlockId(1)).is_empty());
        assert!(dt.children(BlockId(2)).is_empty());
        assert!(dt.children(BlockId(3)).is_empty());
    }

    #[test]
    fn children_linear_chain_walks_root_to_leaf() {
        // 0 → 1 → 2 → 3 (ret). idom chain produces a single spine;
        // children list is each block's lone successor on the chain.
        let func = fixture(vec![
            block(0, Terminator::Br(BlockId(1))),
            block(1, Terminator::Br(BlockId(2))),
            block(2, Terminator::Br(BlockId(3))),
            block(3, Terminator::Ret(None)),
        ]);
        let dt = DominatorTree::compute(&func);
        assert_eq!(dt.children(BlockId(0)), &[BlockId(1)]);
        assert_eq!(dt.children(BlockId(1)), &[BlockId(2)]);
        assert_eq!(dt.children(BlockId(2)), &[BlockId(3)]);
        assert!(dt.children(BlockId(3)).is_empty());
    }

    #[test]
    fn children_omits_unreachable_blocks() {
        // 0 → 1 (ret); 2 unreachable. children must not list 2 anywhere.
        let func = fixture(vec![
            block(0, Terminator::Br(BlockId(1))),
            block(1, Terminator::Ret(None)),
            block(2, Terminator::Ret(None)),
        ]);
        let dt = DominatorTree::compute(&func);
        assert_eq!(dt.children(BlockId(0)), &[BlockId(1)]);
        // Block 2 is unreachable (idom None) → never appears as a
        // child anywhere in the tree.
        for b in 0..3 {
            assert!(
                !dt.children(BlockId(b)).contains(&BlockId(2)),
                "block {b} children must not list unreachable block 2"
            );
        }
    }

    #[test]
    fn blocks_are_found_by_id_not_by_position() {
        // laid out 0, 1, 3, 4 with 0 -> 3 -> 1 -> 4
        let func = fixture(vec![
            block(0, Terminator::Br(BlockId(3))),
            block(1, Terminator::Br(BlockId(4))),
            block(3, Terminator::Br(BlockId(1))),
            block(4, Terminator::Ret(None)),
        ]);
        let dt = DominatorTree::compute(&func);
        assert_eq!(
            dt.reverse_postorder(),
            &[BlockId(0), BlockId(3), BlockId(1), BlockId(4)]
        );
        assert_eq!(dt.immediate_dominator(BlockId(1)), Some(BlockId(3)));
        assert_eq!(dt.immediate_dominator(BlockId(4)), Some(BlockId(1)));
    }
}
