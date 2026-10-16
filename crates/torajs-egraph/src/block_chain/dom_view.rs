//! The dominator tree as the chainer reads it: a computed tree, or one
//! with the immediate dominators of a few blocks replaced — the blocks
//! tail duplication copied or took edges from, which hang below the
//! rest of the tree and dominate nothing outside themselves.

use torajs_core::ssa::BlockId;
use torajs_core::ssa::dominator::DominatorTree;

#[derive(Clone, Copy)]
pub(crate) struct DomView<'a> {
    tree: &'a DominatorTree,
    /// Per block id, its replaced immediate dominator, if any. Every
    /// ancestor of a block not replaced is not replaced either.
    moved: &'a [Option<u32>],
}

impl<'a> DomView<'a> {
    pub(crate) fn whole(tree: &'a DominatorTree) -> Self {
        DomView { tree, moved: &[] }
    }

    /// `tree` with block `b`'s immediate dominator replaced by
    /// `moved[b]` wherever that is not `None`.
    pub(crate) fn patched(tree: &'a DominatorTree, moved: &'a [Option<u32>]) -> Self {
        DomView { tree, moved }
    }

    /// The tree, when nothing of it is replaced.
    pub(crate) fn whole_tree(&self) -> Option<&'a DominatorTree> {
        self.moved.is_empty().then_some(self.tree)
    }

    fn moved(&self, b: u32) -> Option<u32> {
        self.moved.get(b as usize).copied().flatten()
    }

    pub(crate) fn immediate_dominator(&self, b: BlockId) -> Option<BlockId> {
        match self.moved(b.0) {
            Some(d) => Some(BlockId(d)),
            None => self.tree.immediate_dominator(b),
        }
    }

    pub(crate) fn dominates(&self, a: BlockId, b: BlockId) -> bool {
        if a == b {
            return true;
        }
        // climb the replaced part of `b`'s chain: a replaced block
        // dominates only blocks on that part below it
        let mut cur = b.0;
        while let Some(d) = self.moved(cur) {
            if d == a.0 {
                return true;
            }
            cur = d;
        }
        self.moved(a.0).is_none() && self.tree.dominates(a, BlockId(cur))
    }

    /// The nearest block that dominates both `a` and `b`.
    pub(crate) fn common_dominator(&self, a: BlockId, b: BlockId) -> Option<BlockId> {
        let mut cur = Some(a);
        while let Some(c) = cur {
            if self.dominates(c, b) {
                return Some(c);
            }
            cur = self.immediate_dominator(c);
        }
        None
    }
}
