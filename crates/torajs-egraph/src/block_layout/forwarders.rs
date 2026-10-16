//! Threading edges through empty forwarding blocks.

use torajs_core::ssa::{Function, Terminator};

use crate::float_demote::cfg::successors;

/// Retarget every edge that lands on an empty `br`-only block to
/// whatever that block forwards to, then drop the blocks nothing
/// reaches any more. Answers how many blocks went away.
///
/// Safe as a pure edge rewrite because the IR has no phi nodes: an
/// edge carries nothing but control. A forwarder chain resolves in
/// one walk, and a cycle of forwarders (an empty infinite loop)
/// stops at the first block seen twice — it is real control flow and
/// must keep a block to spin on.
pub(super) fn thread_empty_forwarders(func: &mut Function) -> u32 {
    let forwards: Vec<Option<u32>> = func
        .blocks
        .iter()
        .map(|b| match (&b.insts.is_empty(), &b.term) {
            (true, Terminator::Br(t)) if t.0 != b.id.0 => Some(t.0),
            _ => None,
        })
        .collect();
    if forwards.iter().all(|f| f.is_none()) {
        return 0;
    }
    // the walk that last met each block: a walk stops at the first
    // block it meets twice
    let mut seen = vec![0u32; forwards.len()];
    let mut walk = 0u32;
    let mut resolve = |start: u32| -> u32 {
        walk += 1;
        let mut cur = start;
        while let Some(s) = seen.get_mut(cur as usize)
            && *s != walk
        {
            *s = walk;
            match forwards[cur as usize] {
                Some(next) => cur = next,
                None => break,
            }
        }
        cur
    };
    for block in func.blocks.iter_mut() {
        match &mut block.term {
            Terminator::Br(t) => t.0 = resolve(t.0),
            Terminator::CondBr {
                then_blk, else_blk, ..
            } => {
                then_blk.0 = resolve(then_blk.0);
                else_blk.0 = resolve(else_blk.0);
            }
            _ => {}
        }
    }
    crate::branch_fold::sweep_unreachable_blocks(func)
}

/// Move the branch of an instruction-free `cond_br` block up into the
/// one block that reaches it, by a bare `br`, then drop the emptied
/// block. Select formation leaves this shape where it if-converts the
/// two tests of `a || b` that feed a branch: the folded compare ends
/// the header, its branch waits in the join, and codegen, which fuses
/// a compare only into its own block's branch, pays `cset` + `cbz`
/// for it. Answers how many blocks went away.
pub(super) fn fold_bare_branches(func: &mut Function) -> u32 {
    let mut preds = vec![0u32; func.blocks.len()];
    for block in &func.blocks {
        for s in successors(&block.term) {
            preds[s.0 as usize] += 1;
        }
    }
    let mut folded = 0;
    for b in 0..func.blocks.len() {
        let Terminator::Br(t) = func.blocks[b].term else {
            continue;
        };
        let t = t.0 as usize;
        let target = &func.blocks[t];
        if t == b
            || t == 0
            || preds[t] != 1
            || !target.insts.is_empty()
            || !matches!(target.term, Terminator::CondBr { .. })
        {
            continue;
        }
        let term = std::mem::replace(&mut func.blocks[t].term, Terminator::Unreachable);
        func.blocks[b].term = term;
        preds[t] = 0;
        folded += 1;
    }
    if folded == 0 {
        return 0;
    }
    crate::branch_fold::sweep_unreachable_blocks(func)
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{BinOp, Block, BlockId, IPred, Inst, InstKind, Operand, Type, ValueId};

    fn block(id: u32, insts: Vec<Inst>, term: Terminator) -> Block {
        Block {
            id: BlockId(id),
            insts,
            term,
        }
    }

    fn inst(result: u32, kind: InstKind) -> Inst {
        Inst {
            result: Some(ValueId(result)),
            kind,
            origin: None,
        }
    }

    fn void_fn(blocks: Vec<Block>) -> Function {
        Function {
            name: "f".into(),
            params: vec![],
            ret: Type::Void,
            values: vec![],
            blocks,
            current_origin: None,
        }
    }

    /// The 507-04 shape: `br` lands on an empty block that only
    /// forwards, so codegen threads past it and emits the block as a
    /// `b` nothing jumps to. Threading the edge lets the sweep drop it.
    ///
    ///   b0 → b1 ; b1(empty) → b2 ; b2: ret   ==>   b0 → b1(ret)
    #[test]
    fn an_empty_forwarder_is_threaded_away() {
        let mut f = void_fn(vec![
            block(0, vec![], Terminator::Br(BlockId(1))),
            block(1, vec![], Terminator::Br(BlockId(2))),
            block(2, vec![], Terminator::Ret(None)),
        ]);
        // b0 is itself an empty forwarder, but it is the entry: no
        // edge lands on it, so it stays and takes the resolved target.
        assert_eq!(thread_empty_forwarders(&mut f), 1);
        assert_eq!(f.blocks.len(), 2);
        assert!(matches!(f.blocks[0].term, Terminator::Br(BlockId(1))));
        assert!(matches!(f.blocks[1].term, Terminator::Ret(None)));
    }

    /// A chain resolves in one walk, and both arms of a CondBr take
    /// the resolution.
    #[test]
    fn a_forwarder_chain_resolves_through() {
        let mut f = void_fn(vec![
            block(
                0,
                vec![inst(
                    0,
                    InstKind::BinOp(BinOp::Add, Operand::ConstI64(1), Operand::ConstI64(2)),
                )],
                Terminator::CondBr {
                    cond: Operand::Value(ValueId(0)),
                    then_blk: BlockId(1),
                    else_blk: BlockId(3),
                },
            ),
            block(1, vec![], Terminator::Br(BlockId(2))),
            block(2, vec![], Terminator::Br(BlockId(4))),
            block(3, vec![], Terminator::Br(BlockId(4))),
            block(4, vec![], Terminator::Ret(None)),
        ]);
        assert_eq!(thread_empty_forwarders(&mut f), 3);
        assert_eq!(f.blocks.len(), 2);
        match f.blocks[0].term {
            Terminator::CondBr {
                then_blk, else_blk, ..
            } => assert_eq!((then_blk.0, else_blk.0), (1, 1)),
            ref other => panic!("expected a cond_br, got {other:?}"),
        }
    }

    /// An empty infinite loop is real control flow — it forwards to
    /// itself, so there is nothing to thread and the block stays.
    #[test]
    fn a_self_forwarding_block_stays() {
        let mut f = void_fn(vec![
            block(0, vec![], Terminator::Br(BlockId(1))),
            block(1, vec![], Terminator::Br(BlockId(1))),
        ]);
        assert_eq!(thread_empty_forwarders(&mut f), 0);
        assert_eq!(f.blocks.len(), 2);
    }

    /// The `a || b` loop test select formation leaves: the compare ends
    /// b1, its branch waits alone in b2. The branch moves up so the
    /// compare is its own block's tail; b4 → b2 has two predecessors
    /// in the second function and stays.
    ///
    ///   b0 → b1 ; b1: %0 = cmp → b2 ; b2: %0 ? b3 : b4 ; b3: ret ; b4 → b1
    #[test]
    fn a_bare_branch_moves_into_its_only_predecessor() {
        let cbr = Terminator::CondBr {
            cond: Operand::Value(ValueId(0)),
            then_blk: BlockId(3),
            else_blk: BlockId(4),
        };
        let cmp = || {
            inst(
                0,
                InstKind::ICmp(IPred::Ult, Operand::ConstI64(1), Operand::ConstI64(2)),
            )
        };
        let mut f = void_fn(vec![
            block(0, vec![], Terminator::Br(BlockId(1))),
            block(1, vec![cmp()], Terminator::Br(BlockId(2))),
            block(2, vec![], cbr.clone()),
            block(3, vec![], Terminator::Ret(None)),
            block(4, vec![], Terminator::Br(BlockId(1))),
        ]);
        assert_eq!(fold_bare_branches(&mut f), 1);
        assert_eq!(f.blocks.len(), 4);
        match f.blocks[1].term {
            Terminator::CondBr {
                then_blk, else_blk, ..
            } => assert_eq!((then_blk.0, else_blk.0), (2, 3)),
            ref other => panic!("expected a cond_br, got {other:?}"),
        }
        let mut g = void_fn(vec![
            block(0, vec![cmp()], Terminator::Br(BlockId(2))),
            block(1, vec![], Terminator::Br(BlockId(2))),
            block(2, vec![], cbr),
            block(3, vec![], Terminator::Ret(None)),
            block(4, vec![], Terminator::Br(BlockId(1))),
        ]);
        assert_eq!(fold_bare_branches(&mut g), 0);
    }
}
