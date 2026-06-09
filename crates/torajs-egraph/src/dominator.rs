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

use torajs_core::ssa::{BlockId, Function, Terminator};

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
    children: Vec<Vec<BlockId>>,
}

impl DominatorTree {
    /// Compute the domtree for `func`. Entry is assumed to be
    /// `BlockId(0)`; if the function has no blocks this returns an
    /// empty tree (every query returns `None`).
    pub fn compute(func: &Function) -> Self {
        let n = func.blocks.len();
        if n == 0 {
            return Self {
                idom: Vec::new(),
                rpo: Vec::new(),
                rpo_index: Vec::new(),
                children: Vec::new(),
            };
        }
        let entry = BlockId(0);
        let rpo = reverse_postorder(func, entry);
        let mut rpo_index = vec![usize::MAX; n];
        for (pos, blk) in rpo.iter().enumerate() {
            rpo_index[blk.0 as usize] = pos;
        }
        let preds = predecessors(func, n);
        let idom = compute_idoms(&rpo, &rpo_index, &preds, entry);
        let children = build_children(&idom);
        Self {
            idom,
            rpo,
            rpo_index,
            children,
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
        // Walk up `b`'s idom chain looking for `a`. Bounded by tree
        // depth (≤ block count); typical fast termination because the
        // entry's idom is itself.
        let mut cur = b;
        loop {
            let i = cur.0 as usize;
            if i >= self.idom.len() {
                return false;
            }
            match self.idom[i] {
                None => return false,                // unreachable block — `a` cannot dominate
                Some(d) if d == cur => return false, // hit entry's self-loop without finding `a`
                Some(d) if d == a => return true,
                Some(d) => cur = d,
            }
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
        let i = b.0 as usize;
        if i < self.children.len() {
            &self.children[i]
        } else {
            &[]
        }
    }
}

// ---- algorithm internals --------------------------------------------------

/// Successor blocks reachable via `term`. Returns 0 (`Ret` /
/// `Unreachable`), 1 (`Br`), or 2 (`CondBr`).
fn successors(term: &Terminator) -> impl Iterator<Item = BlockId> + '_ {
    let mut out: [Option<BlockId>; 2] = [None, None];
    match term {
        Terminator::Br(b) => out[0] = Some(*b),
        Terminator::CondBr {
            then_blk, else_blk, ..
        } => {
            out[0] = Some(*then_blk);
            out[1] = Some(*else_blk);
        }
        Terminator::Ret(_) | Terminator::Unreachable => {}
    }
    out.into_iter().flatten()
}

/// Reverse postorder = postorder, then reverse. Standard iterative DFS
/// with a "first visit / finalise" two-state marker on each block so
/// we don't recurse into already-being-explored cycles.
fn reverse_postorder(func: &Function, entry: BlockId) -> Vec<BlockId> {
    let n = func.blocks.len();
    let mut postorder = Vec::with_capacity(n);
    let mut visited = vec![false; n];
    let mut stack: Vec<(BlockId, bool)> = Vec::with_capacity(n);
    stack.push((entry, false));
    while let Some((blk, finalised)) = stack.pop() {
        let i = blk.0 as usize;
        if finalised {
            postorder.push(blk);
            continue;
        }
        if visited[i] {
            continue;
        }
        visited[i] = true;
        // Schedule finalisation after children.
        stack.push((blk, true));
        let term = &func.blocks[i].term;
        for succ in successors(term) {
            let si = succ.0 as usize;
            if si < n && !visited[si] {
                stack.push((succ, false));
            }
        }
    }
    postorder.reverse();
    postorder
}

/// `preds[i]` = predecessor blocks of block `i`. Computed by walking
/// every block's terminator once.
fn predecessors(func: &Function, n: usize) -> Vec<Vec<BlockId>> {
    let mut preds: Vec<Vec<BlockId>> = vec![Vec::new(); n];
    for blk in &func.blocks {
        for succ in successors(&blk.term) {
            let si = succ.0 as usize;
            if si < n {
                preds[si].push(blk.id);
            }
        }
    }
    preds
}

/// Cooper-Harvey-Kennedy iterative-doms loop. Operates over the RPO
/// ordering so `intersect`'s finger walk converges quickly.
fn compute_idoms(
    rpo: &[BlockId],
    rpo_index: &[usize],
    preds: &[Vec<BlockId>],
    entry: BlockId,
) -> Vec<Option<BlockId>> {
    let n = preds.len();
    let mut doms: Vec<Option<BlockId>> = vec![None; n];
    // The CHK convention sets entry's idom to itself so `intersect`
    // can climb both fingers without a special root case.
    doms[entry.0 as usize] = Some(entry);

    let mut changed = true;
    while changed {
        changed = false;
        // Process RPO order, skipping the entry (it's fixed).
        for &b in rpo.iter().skip(1) {
            let bi = b.0 as usize;
            let mut new_idom: Option<BlockId> = None;
            for &p in &preds[bi] {
                if doms[p.0 as usize].is_none() {
                    continue; // predecessor not yet processed in this iteration
                }
                new_idom = Some(match new_idom {
                    None => p,
                    Some(prev) => intersect(p, prev, &doms, rpo_index),
                });
            }
            if doms[bi] != new_idom {
                doms[bi] = new_idom;
                changed = true;
            }
        }
    }
    doms
}

/// Reverse the `idom` table into a parent → children adjacency list.
/// The CHK convention makes the entry block its own immediate
/// dominator; that self-loop is suppressed so the entry is not listed
/// as its own child. Unreachable blocks (idom = `None`) contribute no
/// edge. Result is keyed by block index (matching `idom.len()`).
fn build_children(idom: &[Option<BlockId>]) -> Vec<Vec<BlockId>> {
    let n = idom.len();
    let mut children: Vec<Vec<BlockId>> = vec![Vec::new(); n];
    for (i, parent) in idom.iter().enumerate() {
        if let Some(p) = parent {
            let pi = p.0 as usize;
            if pi != i && pi < n {
                children[pi].push(BlockId(i as u32));
            }
        }
    }
    children
}

/// Climb two RPO-numbered fingers until they meet — the meeting point
/// is the nearest common dominator. RPO indices: higher = deeper in
/// the CFG, so we always advance the "deeper" finger toward the root.
fn intersect(
    mut b1: BlockId,
    mut b2: BlockId,
    doms: &[Option<BlockId>],
    rpo_index: &[usize],
) -> BlockId {
    while b1 != b2 {
        while rpo_index[b1.0 as usize] > rpo_index[b2.0 as usize] {
            // Advance b1 toward root.
            match doms[b1.0 as usize] {
                Some(d) => b1 = d,
                None => return b2, // shouldn't happen in a well-formed CHK pass
            }
        }
        while rpo_index[b2.0 as usize] > rpo_index[b1.0 as usize] {
            match doms[b2.0 as usize] {
                Some(d) => b2 = d,
                None => return b1,
            }
        }
    }
    b1
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{Block, Function, Operand, Terminator, Type};

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
}
