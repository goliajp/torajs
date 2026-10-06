//! Where a site's int-to-float converts go: an operand the loop never
//! redefines converts once, at the end of the loop's preheader.

use crate::id_hash::HashSet;

use torajs_core::ssa::loop_analysis::NaturalLoop;
use torajs_core::ssa::{BlockId, Function, InstKind, Operand, Terminator, ValueId};

use super::Uses;

fn succs(term: &Terminator) -> Vec<BlockId> {
    match term {
        Terminator::Br(t) => vec![*t],
        Terminator::CondBr {
            then_blk, else_blk, ..
        } => vec![*then_blk, *else_blk],
        Terminator::Ret(_) | Terminator::Unreachable => vec![],
    }
}

/// The block that every entry into `lp` passes last when `x` holds, in
/// the whole loop, what it holds on entry: no block of the loop defines
/// `x`, the header has one predecessor outside the loop, and every path
/// from the function entry to that predecessor's end defines `x`.
pub(super) fn invariant_preheader(
    func: &Function,
    lp: &NaturalLoop,
    x: ValueId,
) -> Option<BlockId> {
    let defines = |b: BlockId| {
        func.blocks[b.0 as usize]
            .insts
            .iter()
            .any(|i| i.result == Some(x))
    };
    if lp.body.iter().any(|&b| defines(b)) {
        return None;
    }
    let mut outside = (0..func.blocks.len() as u32)
        .map(BlockId)
        .filter(|p| !lp.contains(p) && succs(&func.blocks[p.0 as usize].term).contains(&lp.header));
    let (Some(pre), None) = (outside.next(), outside.next()) else {
        return None;
    };
    if func.params.contains(&x) {
        return Some(pre);
    }
    // a path from the entry that reaches `pre` without passing a def
    let mut seen = HashSet::default();
    let mut stack = vec![BlockId(0)];
    while let Some(b) = stack.pop() {
        if !seen.insert(b) || defines(b) {
            continue;
        }
        if b == pre {
            return None;
        }
        stack.extend(succs(&func.blocks[b.0 as usize].term));
    }
    Some(pre)
}

/// What `v` holds at `(bi, at)` when it is the one-def copy of a value
/// earlier in the block that nothing redefines before `at` (a loop
/// body's `%x = copy %cell` of an outer counter); `v` otherwise.
pub(super) fn copy_source(
    func: &Function,
    uses: &Uses,
    bi: usize,
    at: usize,
    v: ValueId,
) -> ValueId {
    let insts = &func.blocks[bi].insts[..at];
    let Some(k) = insts.iter().position(|i| i.result == Some(v)) else {
        return v;
    };
    let InstKind::Copy(_, Operand::Value(src)) = insts[k].kind else {
        return v;
    };
    let kept = !insts[k + 1..].iter().any(|i| i.result == Some(src));
    if uses.one_def(v) && kept { src } else { v }
}
