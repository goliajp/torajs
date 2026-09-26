//! Hot-successor chaining inside a loop's block range — the part of
//! LLVM MachineBlockPlacement that [`crate::block_layout`] left out.
//!
//! Codegen elides a `b` only when a branch target is the next block,
//! and flips a conditional's polarity only when one arm is. Lowering
//! order puts the arm it created first next, which is often the cold
//! one: a bounds check's grow call, a guard's kernel fallback, a slow
//! decode arm. Every such arm costs the hot path a taken branch per
//! iteration. Walking the loop from its header and placing, at each
//! step, the successor the path most likely continues into makes the
//! common path fall through.
//!
//! "Cold" is decided statically and conservatively: an edge that
//! leaves the loop, a block that ends the function (`ret` /
//! `unreachable`), a block that makes a call, or an arm the lowerer's
//! own guard marks rare (`torajs_core::ssa::branch_prob::cold_arms`:
//! the out-of-bounds side of a bounds check, a header-flag slow path,
//! a throw probe). Of two warm arms, the
//! one leading into a nested loop goes first; where nothing ranks the
//! arms, the one already placed first keeps its place, so such shapes
//! lay out exactly as before. (The opcode heuristic — equality is
//! usually false — was tried and dropped: collatz's `n % 2 === 0` is
//! true two times in three and the cell lost 14%.) Nested loops are
//! already contiguous when their parent is chained and move as one
//! unit, entered at their first block.

use std::collections::HashSet;

use torajs_core::ssa::{BlockId, Function, InstKind, Terminator};

use torajs_core::ssa::branch_prob::cold_arms;
use torajs_core::ssa::dominator::DominatorTree;

/// What the chainer knows about the whole function.
pub(crate) struct ChainFacts<'a> {
    pub(crate) dom: &'a DominatorTree,
    /// Edges `(from, to)` into an arm the lowerer's guards mark cold.
    cold_edges: HashSet<(u32, u32)>,
}

impl<'a> ChainFacts<'a> {
    pub(crate) fn new(func: &Function, dom: &'a DominatorTree) -> Self {
        let cold_edges = cold_arms(func)
            .into_iter()
            .map(|(from, to, _)| (from.0, to.0))
            .collect();
        ChainFacts { dom, cold_edges }
    }
}

/// Reorder `order[span]` (the loop's contiguous range) into
/// hot-successor chains starting at `header`. `children` are the bodies of the
/// loops nested in this one, each already contiguous. Answers whether
/// the order changed.
pub(crate) fn chain_loop_range(
    func: &Function,
    order: &mut [u32],
    span: std::ops::Range<usize>,
    header: u32,
    body: &HashSet<u32>,
    children: &[(&HashSet<u32>, u32)],
    facts: &ChainFacts,
) -> bool {
    let range = &order[span.clone()];
    // Units in current order: a child loop's run of blocks, or one block.
    let mut units: Vec<Vec<u32>> = Vec::new();
    for &id in range {
        let owner = children.iter().position(|(c, _)| c.contains(&id));
        let same = units.last().is_some_and(|u| {
            owner.is_some() && children.iter().position(|(c, _)| c.contains(&u[0])) == owner
        });
        if same {
            units.last_mut().expect("unit").push(id);
        } else {
            units.push(vec![id]);
        }
    }
    // A block on every path into a nested loop (its entry, a preheader,
    // a rotation guard) leads toward the loop.
    let leads_to_loop = |b: u32| {
        children
            .iter()
            .any(|(_, h)| facts.dom.dominates(BlockId(b), BlockId(*h)))
    };
    let unit_of = |id: u32| units.iter().position(|u| u.contains(&id));
    let mut placed = vec![false; units.len()];
    let mut chain: Vec<usize> = Vec::with_capacity(units.len());
    let mut cur = unit_of(header).expect("header in its loop range");
    loop {
        placed[cur] = true;
        chain.push(cur);
        let next = hot_successor(
            func,
            facts,
            &units[cur],
            body,
            &unit_of,
            &leads_to_loop,
            &placed,
        )
        .or_else(|| placed.iter().position(|p| !p));
        match next {
            Some(n) => cur = n,
            None => break,
        }
    }
    let new: Vec<u32> = chain
        .iter()
        .flat_map(|&u| units[u].iter().copied())
        .collect();
    if new.as_slice() == range {
        return false;
    }
    order[span].copy_from_slice(&new);
    true
}

/// The unplaced unit the path through `unit` most likely continues
/// into: the exits of its last block first (that is the branch a
/// fall-through can save), then those of its other blocks.
fn hot_successor(
    func: &Function,
    facts: &ChainFacts,
    unit: &[u32],
    body: &HashSet<u32>,
    unit_of: &dyn Fn(u32) -> Option<usize>,
    leads_to_loop: &dyn Fn(u32) -> bool,
    placed: &[bool],
) -> Option<usize> {
    let last = *unit.last().expect("non-empty unit");
    let blocks = std::iter::once(last).chain(unit.iter().rev().skip(1).copied());
    for b in blocks {
        let targets: Vec<u32> = match &func.blocks[b as usize].term {
            Terminator::Br(t) => vec![t.0],
            Terminator::CondBr {
                then_blk, else_blk, ..
            } => vec![then_blk.0, else_blk.0],
            Terminator::Ret(_) | Terminator::Unreachable => Vec::new(),
        };
        let open: Vec<u32> = targets
            .into_iter()
            .filter(|t| !unit.contains(t) && body.contains(t))
            .filter(|t| unit_of(*t).is_some_and(|u| !placed[u]))
            .collect();
        // Among warm arms, the one entering a nested loop first: the
        // loop-branch heuristic of static prediction (an edge into a
        // loop is taken far more often than the one bypassing it).
        let pick = open
            .iter()
            .copied()
            .filter(|t| !is_cold(func, *t) && !facts.cold_edges.contains(&(b, *t)))
            .min_by_key(|t| (!leads_to_loop(*t), unit_of(*t)))
            .or_else(|| open.iter().copied().min_by_key(|t| unit_of(*t)));
        if let Some(t) = pick {
            return unit_of(t);
        }
    }
    None
}

fn is_cold(func: &Function, id: u32) -> bool {
    let block = &func.blocks[id as usize];
    matches!(block.term, Terminator::Ret(_) | Terminator::Unreachable)
        || block
            .insts
            .iter()
            .any(|i| matches!(i.kind, InstKind::Call(..) | InstKind::CallIndirect(..)))
}
