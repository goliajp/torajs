//! Memory epochs — a number for every program point such that two
//! points carrying the same number, one dominating the other, see the
//! same memory.
//!
//! A number is minted in exactly one place: at an instruction that may
//! store, or at the top of a block whose predecessors do not all end
//! on one number (a loop header always: its back edge is not numbered
//! yet when the header is). A block otherwise starts on the number all
//! its predecessors ended on. So a point carries number E only when
//! every path to it from where E was minted is free of stores, and a
//! path between two points carrying E that stored in between would
//! have to reach the second point through a block that starts on some
//! other number.

use torajs_core::ssa::{BlockId, Function, InstKind, Terminator};

use super::keys::{is_read, is_register_only};
use crate::optimize::classify_pure;

pub(super) struct Epochs {
    /// The number each block starts on, by position.
    pub(super) entry: Vec<u32>,
    /// The numbers minted inside each block, in instruction order.
    pub(super) minted: Vec<Vec<u32>>,
}

/// May change what a later load reads.
pub(super) fn may_store(kind: &InstKind) -> bool {
    !classify_pure(kind) && !is_read(kind) && !is_register_only(kind)
}

fn successors(term: &Terminator) -> [Option<BlockId>; 2] {
    match term {
        Terminator::Br(t) => [Some(*t), None],
        Terminator::CondBr {
            then_blk, else_blk, ..
        } => [Some(*then_blk), Some(*else_blk)],
        Terminator::Ret(_) | Terminator::Unreachable => [None, None],
    }
}

/// Reachable blocks, every block after its forward predecessors.
fn reverse_postorder(func: &Function) -> Vec<usize> {
    let n = func.blocks.len();
    let mut seen = vec![false; n];
    let mut post = Vec::with_capacity(n);
    // (block, next successor to look at)
    let mut stack = vec![(0usize, 0usize)];
    seen[0] = true;
    while let Some((b, next)) = stack.pop() {
        let succ = successors(&func.blocks[b].term);
        match succ.get(next).copied().flatten() {
            Some(s) => {
                stack.push((b, next + 1));
                let s = s.0 as usize;
                if s < n && !std::mem::replace(&mut seen[s], true) {
                    stack.push((s, 0));
                }
            }
            None if next < succ.len() => stack.push((b, next + 1)),
            None => post.push(b),
        }
    }
    post.reverse();
    post
}

pub(super) fn number(func: &Function, preds: &[Vec<BlockId>]) -> Epochs {
    let n = func.blocks.len();
    let mut entry = vec![0u32; n];
    let mut minted = vec![Vec::new(); n];
    let mut end: Vec<Option<u32>> = vec![None; n];
    let mut next = 0u32;
    let mut mint = || {
        next += 1;
        next
    };
    for b in reverse_postorder(func) {
        let mut ends = preds[b].iter().map(|p| end[p.0 as usize]);
        let mut e = match ends.next() {
            Some(Some(first)) if ends.all(|x| x == Some(first)) => first,
            _ => mint(),
        };
        entry[b] = e;
        for inst in &func.blocks[b].insts {
            if may_store(&inst.kind) {
                e = mint();
                minted[b].push(e);
            }
        }
        end[b] = Some(e);
    }
    Epochs { entry, minted }
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{Block, Inst, Operand, Type, ValueId, ValueInfo};

    fn store() -> Inst {
        Inst {
            result: None,
            kind: InstKind::Store(Operand::ConstI64(0), Operand::Value(ValueId(0)), 8),
            origin: None,
        }
    }
    fn blk(id: u32, insts: Vec<Inst>, term: Terminator) -> Block {
        Block {
            id: BlockId(id),
            insts,
            term,
        }
    }
    fn cond_br(t: u32, e: u32) -> Terminator {
        Terminator::CondBr {
            cond: Operand::ConstBool(true),
            then_blk: BlockId(t),
            else_blk: BlockId(e),
        }
    }
    fn epochs(blocks: Vec<Block>) -> Epochs {
        let n = blocks.len();
        let func = Function {
            name: "f".into(),
            params: vec![],
            ret: Type::Void,
            values: vec![ValueInfo {
                ty: Type::Ptr,
                name: None,
            }],
            blocks,
            current_origin: None,
        };
        let mut preds: Vec<Vec<BlockId>> = vec![Vec::new(); n];
        for b in &func.blocks {
            for s in successors(&b.term).into_iter().flatten() {
                if !preds[s.0 as usize].contains(&b.id) {
                    preds[s.0 as usize].push(b.id);
                }
            }
        }
        number(&func, &preds)
    }

    /// b0 → (b1 | b2) → b3, with `arm` in b1.
    fn diamond(arm: Vec<Inst>) -> Epochs {
        epochs(vec![
            blk(0, vec![], cond_br(1, 2)),
            blk(1, arm, Terminator::Br(BlockId(3))),
            blk(2, vec![], Terminator::Br(BlockId(3))),
            blk(3, vec![], Terminator::Ret(None)),
        ])
    }

    #[test]
    fn a_join_of_arms_that_store_nothing_keeps_the_number() {
        let e = diamond(vec![]);
        assert_eq!(e.entry[3], e.entry[0]);
    }

    #[test]
    fn a_join_below_a_storing_arm_starts_a_new_number() {
        let e = diamond(vec![store()]);
        assert_eq!(e.entry[2], e.entry[0]);
        assert_eq!(e.minted[1].len(), 1);
        assert_ne!(e.entry[3], e.entry[0]);
        assert_ne!(e.entry[3], e.minted[1][0]);
    }

    /// b0 → b1 (header) → (b2 → b1 | b3): the header cannot know what
    /// the latch ends on.
    #[test]
    fn a_loop_header_starts_a_new_number() {
        let e = epochs(vec![
            blk(0, vec![], Terminator::Br(BlockId(1))),
            blk(1, vec![], cond_br(2, 3)),
            blk(2, vec![], Terminator::Br(BlockId(1))),
            blk(3, vec![], Terminator::Ret(None)),
        ]);
        assert_ne!(e.entry[1], e.entry[0]);
        assert_eq!(e.entry[2], e.entry[1]);
        assert_eq!(e.entry[3], e.entry[1]);
    }
}
