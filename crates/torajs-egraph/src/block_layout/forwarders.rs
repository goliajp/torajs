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
