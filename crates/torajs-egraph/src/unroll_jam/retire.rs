//! Where a masked lane goes once its group lets it go (`R`), and which
//! of its cells it keeps from the step it leaves the inner loop in.
//!
//! The plain form keeps every cell the inner loop writes as it stood
//! when that step began, and the tail reruns the step from them. Where
//! the code the exits lead to reads fewer of those cells, a lane keeps
//! only those (an escape-time loop keeps its count, not its point), so
//! the step cannot be rerun:
//!
//! ```text
//! R:   read cells the inner loop leaves alone, kept cells ;
//!      cond_br act_0, RA, RX
//! RA:  the other cells as slot 0 holds them ; br T (still stepping)
//! RX:  the exit tests ahead of the last exit, from the kept cells:
//!      one firing → its exit in T ; none → the last exit's target in T
//! ```
//!
//! Why the result cannot differ:
//! - a kept cell is one some exit's target may read before writing it,
//!   and no step writes it ahead of that exit's test, so its value at
//!   the step's start is its value at the exit; a cell not kept is read
//!   by no exit's code before it is written again.
//! - the tests ahead of the last exit read only kept cells, cells the
//!   inner loop leaves alone and values from outside it, so `RX` takes
//!   them as the leaving step did; the step left by the first test that
//!   failed, or, with none failed, by the last.

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{
    BlockId, Function, Inst, InstKind, Operand, Terminator, ValueId, visit_value_operands,
};

use super::Plan;
use super::clone::{clone_region, push, tail_clone};
use super::plan::{successors, term_value};

type Lane = HashMap<ValueId, ValueId>;

/// Slot 0's lane as `R` reads it: its renaming at the group's start,
/// its snapshots, and its flag.
pub(super) struct Slot0<'a> {
    pub first: &'a Lane,
    pub snap: &'a Lane,
    pub act: ValueId,
}

/// The cells a masked lane keeps a snapshot of: `exit_cells` when the
/// plan has them, else every cell the inner loop writes.
pub(super) fn snapped(func: &Function, p: &Plan) -> Vec<ValueId> {
    p.exit_cells
        .clone()
        .unwrap_or_else(|| inner_writes(func, p))
}

/// The lane cells the inner loop writes.
pub(super) fn inner_writes(func: &Function, p: &Plan) -> Vec<ValueId> {
    let mut out: Vec<ValueId> = p
        .inner_order()
        .iter()
        .flat_map(|b| func.blocks[b.0 as usize].insts.iter())
        .filter_map(|i| i.result)
        .filter(|r| p.lane_cells.contains(r) && !p.shared.contains(r))
        .collect();
    out.sort_by_key(|v| v.0);
    out.dedup();
    out
}

/// Each exit of the inner loop in step order: the position of its
/// block in `inner` and the edge's target.
pub(super) fn exits(func: &Function, inner: &[BlockId]) -> Vec<(usize, BlockId)> {
    let mut out = Vec::new();
    for (at, b) in inner.iter().enumerate() {
        if let Terminator::CondBr {
            then_blk, else_blk, ..
        } = func.blocks[b.0 as usize].term
        {
            let leaves = if inner.contains(&then_blk) {
                else_blk
            } else {
                then_blk
            };
            out.push((at, leaves));
        }
    }
    out
}

/// The cells live at some exit's target, when they are fewer than the
/// cells the inner loop writes and a lane can retire from them alone:
/// each is a cell the inner loop writes (not a value of the step it
/// leaves in), no step writes it ahead of the exit, and the tests
/// ahead of the last exit read none of the cells left out.
pub(super) fn exit_cells(func: &Function, p: &Plan) -> Option<Vec<ValueId>> {
    let inner = p.inner_order();
    let written = inner_writes(func, p);
    let insts = |bs: &[BlockId]| -> Vec<Inst> {
        bs.iter()
            .flat_map(|b| func.blocks[b.0 as usize].insts.iter().cloned())
            .collect()
    };
    let defined: HashSet<ValueId> = insts(&inner).iter().filter_map(|i| i.result).collect();
    let live = live_in(func, &defined);
    let exits = exits(func, &inner);
    let mut keep: HashSet<ValueId> = HashSet::default();
    for &(at, target) in &exits {
        let before: Vec<ValueId> = insts(&inner[..=at])
            .iter()
            .filter_map(|i| i.result)
            .collect();
        for &v in &live[target.0 as usize] {
            if !written.contains(&v) || before.contains(&v) {
                return None;
            }
            keep.insert(v);
        }
    }
    let &(last, _) = exits.last()?;
    if keep.len() >= written.len() || !tests_read_kept(func, &inner[..last], &defined, &keep) {
        return None;
    }
    let mut out: Vec<ValueId> = keep.into_iter().collect();
    out.sort_by_key(|v| v.0);
    Some(out)
}

/// Every branch in `blocks`, run from the kept cells, tests a value
/// that reads none of the other values the inner loop defines as the
/// step starts.
fn tests_read_kept(
    func: &Function,
    blocks: &[BlockId],
    defined: &HashSet<ValueId>,
    keep: &HashSet<ValueId>,
) -> bool {
    let mut unknown: HashSet<ValueId> = defined.difference(keep).copied().collect();
    for b in blocks {
        let block = &func.blocks[b.0 as usize];
        for inst in &block.insts {
            let mut reads = false;
            visit_value_operands(&inst.kind, |v| reads |= unknown.contains(&v));
            let r = inst.result.expect("planned regions define values");
            if reads {
                unknown.insert(r);
            } else {
                unknown.remove(&r);
            }
        }
        if let Terminator::CondBr { cond, .. } = &block.term
            && matches!(cond, Operand::Value(c) if unknown.contains(c))
        {
            return false;
        }
    }
    true
}

/// Of `of`, the values live into each block (by block index): read on
/// some path from its start before any def of them.
fn live_in(func: &Function, of: &HashSet<ValueId>) -> Vec<HashSet<ValueId>> {
    let n = func.blocks.len();
    let mut live: Vec<HashSet<ValueId>> = vec![HashSet::default(); n];
    let mut defs: Vec<HashSet<ValueId>> = vec![HashSet::default(); n];
    for (i, block) in func.blocks.iter().enumerate() {
        let (used, def) = (&mut live[i], &mut defs[i]);
        for inst in &block.insts {
            visit_value_operands(&inst.kind, |v| {
                if of.contains(&v) && !def.contains(&v) {
                    used.insert(v);
                }
            });
            if let Some(r) = inst.result.filter(|r| of.contains(r)) {
                def.insert(r);
            }
        }
        if let Some(v) = term_value(&block.term).filter(|v| of.contains(v) && !def.contains(v)) {
            used.insert(v);
        }
    }
    loop {
        let mut grew = false;
        for i in (0..n).rev() {
            for s in successors(&func.blocks[i].term) {
                let more: Vec<ValueId> = live[s.0 as usize]
                    .iter()
                    .filter(|v| !defs[i].contains(v) && !live[i].contains(v))
                    .copied()
                    .collect();
                grew |= !more.is_empty();
                live[i].extend(more);
            }
        }
        if !grew {
            return live;
        }
    }
}

/// `R` and the tail slot 0's lane finishes in, the tail's latch going
/// to `n`.
pub(super) fn retire(
    func: &mut Function,
    p: &Plan,
    cells: &HashSet<ValueId>,
    read_after_pre: &HashSet<ValueId>,
    slot0: Slot0,
    r: BlockId,
    n: BlockId,
) {
    let tail = tail_clone(func, p, cells, slot0.first, n);
    let read: Vec<ValueId> = p
        .lane_cells
        .iter()
        .copied()
        .filter(|c| read_after_pre.contains(c))
        .collect();
    let copy_in = |func: &mut Function, b: BlockId, c: ValueId, from: ValueId| {
        let ty = func.values[c.0 as usize].ty;
        push(func, b, c, InstKind::Copy(ty, Operand::Value(from)));
    };
    let entry = tail[&p.inner_header];
    if p.exit_cells.is_none() {
        for &c in &read {
            copy_in(func, r, c, *slot0.snap.get(&c).unwrap_or(&slot0.first[&c]));
        }
        func.blocks[r.0 as usize].term = Terminator::Br(entry);
        return;
    }
    let written = inner_writes(func, p);
    let ra = func.add_block();
    for &c in &read {
        match slot0.snap.get(&c) {
            Some(&s) => copy_in(func, r, c, s),
            None if written.contains(&c) => copy_in(func, ra, c, slot0.first[&c]),
            None => copy_in(func, r, c, slot0.first[&c]),
        }
    }
    func.blocks[ra.0 as usize].term = Terminator::Br(entry);
    let rx = replay(func, p, cells, slot0.first, &tail);
    func.blocks[r.0 as usize].term = Terminator::CondBr {
        cond: Operand::Value(slot0.act),
        then_blk: ra,
        else_blk: rx,
    };
}

/// `RX`: the inner blocks ahead of the last exit's, cloned as the tail
/// clones them; an exit goes to its target in the tail, and where the
/// step would reach the last exit's block it takes that exit. Returns
/// its entry.
fn replay(
    func: &mut Function,
    p: &Plan,
    cells: &HashSet<ValueId>,
    first: &Lane,
    tail: &HashMap<BlockId, BlockId>,
) -> BlockId {
    let inner = p.inner_order();
    let &(last, target) = exits(func, &inner)
        .last()
        .expect("a masked inner loop has exits");
    let in_tail = |t: BlockId| tail.get(&t).copied().unwrap_or(t);
    let goal = in_tail(target);
    let edge = |t: BlockId, own: &HashMap<BlockId, BlockId>| match own.get(&t) {
        Some(&c) => c,
        None if t == inner[last] => goal,
        None => in_tail(t),
    };
    let own = clone_region(func, p, cells, first, &inner[..last], edge);
    own.get(&inner[0]).copied().unwrap_or(goal)
}
