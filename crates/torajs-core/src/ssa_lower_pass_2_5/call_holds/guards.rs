//! Settling the guards of a function's holds: a kept piece's branch
//! goes into it; a removed piece goes away with its blocks, and the
//! block before its guard runs on into the block after it, as if the
//! piece had never been lowered.

use crate::ssa::{BlockId, CallHold, Function, HoldGuard, Terminator};

use super::span::successors;

/// The blocks of `f` that run a piece of one of `holds`.
pub(super) fn piece_blocks(f: &Function, holds: &[CallHold]) -> Vec<bool> {
    let mut mask = vec![false; f.blocks.len()];
    for g in holds.iter().flat_map(pieces) {
        for b in g.body.0..g.body.1 {
            mask[b as usize] = true;
        }
    }
    mask
}

fn pieces(h: &CallHold) -> impl Iterator<Item = &HoldGuard> {
    std::iter::once(&h.take).chain(&h.drops)
}

/// Keeps the pieces of the holds `keep` marks and removes the rest.
pub(super) fn settle(f: &mut Function, holds: &[CallHold], keep: &[bool]) {
    let mut guards: Vec<(HoldGuard, bool)> = (holds.iter().zip(keep))
        .flat_map(|(h, k)| pieces(h).map(move |g| (*g, *k)))
        .collect();
    // in the order they were lowered: a guard's `at` may be the join
    // of the one before
    guards.sort_by_key(|(g, _)| g.body.0);
    let n = f.blocks.len();
    let mut preds = vec![0u32; n];
    for s in f.blocks.iter().flat_map(|b| successors(&b.term)) {
        preds[s] += 1;
    }
    // where the code of each block lives once joins merge into the
    // blocks before their guards
    let mut home: Vec<usize> = (0..n).collect();
    let mut dead = vec![false; n];
    for (g, keep) in guards {
        let at = home[g.at.0 as usize];
        if keep {
            f.set_term(BlockId(at as u32), Terminator::Br(BlockId(g.body.0)));
            continue;
        }
        for b in g.body.0..g.body.1 {
            dead[b as usize] = true;
        }
        let join = g.join.0 as usize;
        // the guard's skip edge and the piece's last block are the
        // join's only ways in unless later code branched to it
        if preds[join] != 2 {
            f.set_term(BlockId(at as u32), Terminator::Br(g.join));
            continue;
        }
        let insts = std::mem::take(&mut f.blocks[join].insts);
        let term = std::mem::replace(&mut f.blocks[join].term, Terminator::Unreachable);
        f.blocks[at].insts.extend(insts);
        f.blocks[at].term = term;
        dead[join] = true;
        home[join] = at;
    }
    if dead.contains(&true) {
        drop_blocks(f, &dead);
    }
}

/// Removes the blocks `dead` marks, which nothing live enters, and
/// numbers the rest in their order.
fn drop_blocks(f: &mut Function, dead: &[bool]) {
    let mut to = vec![u32::MAX; dead.len()];
    let mut next = 0;
    for (b, d) in dead.iter().enumerate() {
        if !d {
            to[b] = next;
            next += 1;
        }
    }
    let at = |b: &BlockId| BlockId(to[b.0 as usize]);
    let mut i = 0;
    f.blocks.retain(|_| {
        i += 1;
        !dead[i - 1]
    });
    for blk in &mut f.blocks {
        blk.id = at(&blk.id);
        match &mut blk.term {
            Terminator::Br(t) => *t = at(t),
            Terminator::CondBr {
                then_blk, else_blk, ..
            } => {
                *then_blk = at(then_blk);
                *else_blk = at(else_blk);
            }
            Terminator::Ret(_) | Terminator::Unreachable => {}
        }
    }
}
