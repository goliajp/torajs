//! The last dominator tree and loop analysis built for each function,
//! handed out again while the function's control flow is unchanged.
//!
//! Passes build both analyses for themselves, and most passes leave
//! the blocks and branches alone: on a 214-block `main` the pipeline
//! asked for its dominator tree 25 times and the function had 6
//! different control flows in all. Like an analysis manager's cached
//! results, an entry is kept per function; unlike one, nothing has to
//! say which passes preserve it, because the entry is checked against
//! everything the analysis reads: the block ids in layout order and
//! each block's branch targets ([`CfgKey`]), and for the loops also
//! the dominator tree they were given.

use std::cell::RefCell;

use super::{DominatorTree, targets};
use crate::ssa::Function;
use crate::ssa::id_hash::HashMap;
use crate::ssa::loop_analysis::LoopAnalysis;

/// Everything of a function the two analyses read: per block in
/// layout order, its id, its number of branch targets and the targets.
#[derive(PartialEq, Eq)]
struct CfgKey(Vec<u32>);

impl CfgKey {
    fn of(func: &Function) -> Self {
        let mut key = Vec::with_capacity(func.blocks.len() * 4);
        for b in &func.blocks {
            let (ts, k) = targets(&b.term);
            key.push(b.id.0);
            key.push(k as u32);
            for t in &ts[..k] {
                key.push(t.0);
            }
        }
        CfgKey(key)
    }
}

struct LoopEntry {
    key: CfgKey,
    /// the intervals of the tree the loops were found with: they are
    /// all of it `dominates` reads
    enter: Vec<u32>,
    exit: Vec<u32>,
    loops: LoopAnalysis,
}

thread_local! {
    static TREES: RefCell<HashMap<String, (CfgKey, DominatorTree)>> =
        RefCell::new(HashMap::default());
    static LOOPS: RefCell<HashMap<String, LoopEntry>> = RefCell::new(HashMap::default());
}

/// The dominator tree of `func`: the one built last for a function of
/// this name when its key matches, else `build()`'s, kept for next time.
pub(super) fn tree(func: &Function, build: impl FnOnce() -> DominatorTree) -> DominatorTree {
    let key = CfgKey::of(func);
    let kept = TREES.with_borrow(|m| {
        m.get(func.name.as_str())
            .filter(|(k, _)| *k == key)
            .map(|(_, t)| t.clone())
    });
    if let Some(t) = kept {
        return t;
    }
    let t = build();
    TREES.with_borrow_mut(|m| m.insert(func.name.clone(), (key, t.clone())));
    t
}

/// The loops of `func` under `dom`, kept like [`tree`]; `dom` must also
/// be the tree they were found with.
pub(crate) fn loops(
    func: &Function,
    dom: &DominatorTree,
    build: impl FnOnce() -> LoopAnalysis,
) -> LoopAnalysis {
    let key = CfgKey::of(func);
    let kept = LOOPS.with_borrow(|m| {
        m.get(func.name.as_str())
            .filter(|e| e.key == key && e.enter == dom.enter && e.exit == dom.exit)
            .map(|e| e.loops.clone())
    });
    if let Some(l) = kept {
        return l;
    }
    let l = build();
    let entry = LoopEntry {
        key,
        enter: dom.enter.clone(),
        exit: dom.exit.clone(),
        loops: l.clone(),
    };
    LOOPS.with_borrow_mut(|m| m.insert(func.name.clone(), entry));
    l
}

#[cfg(test)]
mod tests {
    use crate::ssa::dominator::DominatorTree;
    use crate::ssa::loop_analysis::LoopAnalysis;
    use crate::ssa::{Block, BlockId, Function, Operand, Terminator, Type};

    fn func(name: &str, terms: Vec<Terminator>) -> Function {
        Function {
            name: name.to_string(),
            params: vec![],
            ret: Type::Void,
            blocks: terms
                .into_iter()
                .enumerate()
                .map(|(i, term)| Block {
                    id: BlockId(i as u32),
                    insts: vec![],
                    term,
                })
                .collect(),
            values: vec![],
            current_origin: None,
        }
    }

    fn cond(t: u32, e: u32) -> Terminator {
        Terminator::CondBr {
            cond: Operand::ConstBool(true),
            then_blk: BlockId(t),
            else_blk: BlockId(e),
        }
    }

    /// A branch retargeted between two builds of the same function is
    /// seen: the second tree and loops are the new control flow's.
    #[test]
    fn an_edited_function_gets_fresh_analyses() {
        let br = |t| Terminator::Br(BlockId(t));
        let mut f = func(
            "memo_edit",
            vec![br(1), cond(2, 3), br(1), Terminator::Ret(None)],
        );
        let dom = DominatorTree::compute(&f);
        assert_eq!(LoopAnalysis::compute(&f, &dom).loops().len(), 1);
        assert_eq!(dom.immediate_dominator(BlockId(3)), Some(BlockId(1)));
        f.blocks[2].term = br(3);
        let dom = DominatorTree::compute(&f);
        assert_eq!(dom.immediate_dominator(BlockId(3)), Some(BlockId(1)));
        assert!(LoopAnalysis::compute(&f, &dom).loops().is_empty());
        f.blocks[0].term = br(3);
        let dom = DominatorTree::compute(&f);
        assert_eq!(dom.immediate_dominator(BlockId(3)), Some(BlockId(0)));
        assert!(!dom.is_reachable(BlockId(1)));
    }

    /// Loops asked for with a tree of an older control flow are found
    /// with that tree, as a fresh build would, not handed the loops
    /// kept for the current one.
    #[test]
    fn the_loops_follow_the_tree_they_are_given() {
        let br = |t| Terminator::Br(BlockId(t));
        let mut f = func(
            "memo_stale_tree",
            vec![br(1), cond(2, 3), br(1), Terminator::Ret(None)],
        );
        let old = DominatorTree::compute(&f);
        f.blocks[0].term = cond(1, 2);
        let new = DominatorTree::compute(&f);
        let with_new = LoopAnalysis::compute(&f, &new);
        let with_old = LoopAnalysis::compute(&f, &old);
        assert!(with_new.loops().is_empty());
        assert_eq!(with_old.loops().len(), 1);
        assert_eq!(with_old.loops()[0].header, BlockId(1));
    }
}
