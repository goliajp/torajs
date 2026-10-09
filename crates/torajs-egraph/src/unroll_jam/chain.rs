//! The regions of a planned loop between its header and the inner
//! loop's back edge: the straight run into the inner header, the inner
//! header's exit, and the inner body.

use crate::id_hash::HashSet;

use torajs_core::ssa::loop_analysis::NaturalLoop;
use torajs_core::ssa::{BlockId, Function, Operand, Terminator};

use super::MAX_CHAIN_BLOCKS;
use super::plan::{Ctx, blk};

/// The straight run of blocks from the header's body entry to the
/// inner header; returns it and the inner header's entry predecessor.
pub(super) fn pre_chain(
    func: &Function,
    ctx: &Ctx,
    body: &HashSet<BlockId>,
    h: BlockId,
    p0: BlockId,
    ih: BlockId,
) -> Option<(Vec<BlockId>, BlockId)> {
    let mut blocks = Vec::new();
    let (mut prev, mut cur) = (h, p0);
    while cur != ih {
        if !body.contains(&cur)
            || blocks.len() >= MAX_CHAIN_BLOCKS
            || ctx.preds.get(&cur).map(Vec::as_slice) != Some(&[prev])
        {
            return None;
        }
        let next = guarded_next(&blk(func, cur).term, |b| body.contains(&b), body)?;
        blocks.push(cur);
        (prev, cur) = (cur, next);
    }
    Some((blocks, prev))
}

/// `(stays-on-true, continue target, exit target)` of the inner header.
pub(super) fn inner_exit(
    func: &Function,
    inner: &NaturalLoop,
    ih: BlockId,
) -> Option<(bool, BlockId, BlockId)> {
    let Terminator::CondBr {
        cond: Operand::Value(_),
        then_blk,
        else_blk,
    } = blk(func, ih).term
    else {
        return None;
    };
    let inside = |b: BlockId| inner.contains(&b);
    match (inside(then_blk), inside(else_blk)) {
        (true, false) => Some((true, then_blk, else_blk)),
        (false, true) => Some((false, else_blk, then_blk)),
        _ => None,
    }
}

/// Where a chain block goes on: its `br` target, or the target `keep` accepts of a guard
/// whose other edge leaves the outer loop (an overflow bail). A lane whose guard fires before
/// every earlier lane has finished sends its group back to the original loop; nothing has
/// been written by then.
fn guarded_next(
    term: &Terminator,
    keep: impl Fn(BlockId) -> bool,
    outer: &HashSet<BlockId>,
) -> Option<BlockId> {
    match *term {
        Terminator::Br(next) => Some(next),
        Terminator::CondBr {
            cond: Operand::Value(_),
            then_blk,
            else_blk,
        } => {
            if keep(then_blk) && !outer.contains(&else_blk) {
                Some(then_blk)
            } else if keep(else_blk) && !outer.contains(&then_blk) {
                Some(else_blk)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// The inner body as a straight run of blocks back to its header
/// (empty when the header loops on itself).
pub(super) fn inner_chain(
    func: &Function,
    ctx: &Ctx,
    body: &HashSet<BlockId>,
    inner: &NaturalLoop,
    ih: BlockId,
    cont: BlockId,
) -> Option<Vec<BlockId>> {
    let mut chain = Vec::new();
    let (mut prev, mut cur) = (ih, cont);
    while cur != ih {
        if chain.len() >= MAX_CHAIN_BLOCKS
            || ctx.preds.get(&cur).map(Vec::as_slice) != Some(&[prev])
        {
            return None;
        }
        let next = guarded_next(&blk(func, cur).term, |b| inner.contains(&b), body)?;
        chain.push(cur);
        (prev, cur) = (cur, next);
    }
    (inner.body.len() == chain.len() + 1).then_some(chain)
}
