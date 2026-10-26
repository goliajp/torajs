//! The byte range of every loop of a compiled function, for the
//! linker's placement.

use torajs_core::ssa::id_hash::HashMap;
use torajs_core::ssa::{BlockId, Function, Terminator};

use crate::reloc::Reloc;

/// A function's emitted code, its relocations and its loop byte ranges.
pub(super) type EmittedBody = (Vec<u8>, Vec<Reloc>, Vec<(u32, u32)>);

/// `bytes` and `relocs` of `func`, emitted with its blocks starting at
/// `block_byte_starts`, with the byte ranges of its loops.
pub(super) fn emitted(
    func: &Function,
    bytes: Vec<u8>,
    relocs: Vec<Reloc>,
    block_byte_starts: &HashMap<u32, u32>,
) -> EmittedBody {
    // nothing is emitted after the last block, so its end is the code's end
    let loops = loop_ranges(func, block_byte_starts, bytes.len() as u32);
    (bytes, relocs, loops)
}

/// The byte ranges of the loops: each back edge ([`back_edges`])
/// closes a range from its target's first byte to the end of the
/// branching block (the next block's start, or `end` for the last
/// block; for a latch the emitter left out, [`latch_ends`]). Each
/// edge back to one header keeps its own range: block layout puts a loop's cold arms after its hot latch, and one range
/// over every edge would take them in, past what a region holds, and
/// leave the hot path unguarded. A branch that only jumps backwards in
/// the emitted order (a cold block at the end returning to a join)
/// closes nothing. Read off the walk and the emitted order rather than
/// from a dominator tree: block layout has already made every loop
/// body contiguous from its header, and this runs on every function of
/// every build. That holds for a natural loop (one header dominating
/// its body); in an irreducible cycle the edge the walk calls back
/// depends on the walk's order, and its range covers what was emitted
/// between its two ends, not necessarily the whole cycle. Shortest
/// ranges first, so a placement that cannot keep every range in one
/// region keeps the inner ones.
fn loop_ranges(
    func: &Function,
    block_byte_starts: &HashMap<u32, u32>,
    end: u32,
) -> Vec<(u32, u32)> {
    // this runs on every function of every build, and most have no
    // loop: a cycle needs a branch to a block at or before its own in
    // the emitted order, which is the block order, read off the ids
    // alone when they are the positions (they are, for almost every
    // function); a cold block at the end branching back to a join
    // passes this and is turned away by the walk, before any byte
    // offset is looked up
    let pos = Positions::of(func);
    if !pos.any_backward(func) {
        return Vec::new();
    }
    let edges = back_edges(func, &pos);
    if edges.is_empty() {
        return Vec::new();
    }
    let start = |b: BlockId| block_byte_starts.get(&b.0).copied();
    // the end of the block at position `p`: the start of the next
    // block the emitter laid down, or the function's end
    let block_end = |p: usize| {
        (func.blocks[p + 1..].iter())
            .find_map(|b| start(b.id))
            .unwrap_or(end)
    };
    // shortest first, as (length, start) pairs, a total order a plain
    // tuple sort keeps; two edges from one block close one range
    let mut by_len: Vec<(u32, u32)> = edges
        .into_iter()
        .flat_map(|(from, to)| {
            let (from, head) = (pos.of_block(from), pos.of_block(to));
            // block layout lays a loop out from its header and never
            // rotates it (egraph `block_layout`), so a back edge's
            // target is laid out first; one that is not names no
            // contiguous range
            let to_at = start(to).filter(|_| head <= from);
            let ends = match to_at {
                Some(_) => latch_ends(func, from, head, &start, &block_end),
                None => Vec::new(),
            };
            (ends.into_iter()).flat_map(move |e| to_at.map(|at| (e - at, at)))
        })
        .collect();
    by_len.sort_unstable();
    by_len.dedup();
    by_len.into_iter().map(|(len, lo)| (lo, lo + len)).collect()
}

/// The ends of the code that branches back from the latch at
/// position `p` to the header at position `head`. An emitted latch
/// ends at `block_end(p)`. A latch the emitter left out is an empty
/// forwarder whose predecessors branch to the header themselves
/// (through any further forwarders): each emitted one laid out from
/// the header on ends a range of its own, as a second edge back would,
/// so a cold arm laid out past the loop's exit stretches only its own
/// range. One laid out ahead of the header enters the loop and closes
/// nothing.
fn latch_ends(
    func: &Function,
    p: usize,
    head: usize,
    start: &impl Fn(BlockId) -> Option<u32>,
    block_end: &impl Fn(usize) -> u32,
) -> Vec<u32> {
    let latch = func.blocks[p].id;
    if start(latch).is_some() {
        return vec![block_end(p)];
    }
    let mut ends = Vec::new();
    // the forwarders still to read, and every one ever queued
    let mut todo = vec![latch];
    let mut seen = vec![latch];
    while let Some(h) = todo.pop() {
        for (i, b) in func.blocks.iter().enumerate() {
            if !branches_to(&b.term, h) {
                continue;
            }
            if start(b.id).is_some() {
                if i >= head {
                    ends.push(block_end(i));
                }
            } else if !seen.contains(&b.id) {
                seen.push(b.id);
                todo.push(b.id);
            }
        }
    }
    ends
}

fn branches_to(term: &Terminator, b: BlockId) -> bool {
    match term {
        Terminator::Br(t) => *t == b,
        Terminator::CondBr {
            then_blk, else_blk, ..
        } => *then_blk == b || *else_blk == b,
        Terminator::Ret(_) | Terminator::Unreachable => false,
    }
}

/// Where each block sits in `blocks`, by id: no table when every
/// block's id is its position (`Dense` holds the block count).
enum Positions {
    Dense(usize),
    Table(Vec<u32>),
}

impl Positions {
    fn of(func: &Function) -> Positions {
        let dense = (func.blocks.iter().enumerate()).all(|(i, b)| b.id.0 as usize == i);
        if dense {
            return Positions::Dense(func.blocks.len());
        }
        let span = (func.blocks.iter())
            .map(|b| b.id.0 as usize + 1)
            .max()
            .unwrap_or(0);
        let mut table = vec![u32::MAX; span];
        for (i, block) in func.blocks.iter().enumerate() {
            table[block.id.0 as usize] = i as u32;
        }
        Positions::Table(table)
    }

    /// The position of block `b`; None for an id no block has.
    fn get(&self, b: BlockId) -> Option<usize> {
        match self {
            Positions::Dense(n) => Some(b.0 as usize).filter(|p| p < n),
            Positions::Table(t) => (t.get(b.0 as usize).copied())
                .filter(|&p| p != u32::MAX)
                .map(|p| p as usize),
        }
    }

    fn of_block(&self, b: BlockId) -> usize {
        self.get(b).expect("a block of the function")
    }

    /// Whether any block branches to one at or before its own position.
    fn any_backward(&self, func: &Function) -> bool {
        let back = |i: usize, t: BlockId| self.get(t).is_some_and(|p| p <= i);
        (func.blocks.iter().enumerate()).any(|(i, block)| match &block.term {
            Terminator::Br(t) => back(i, *t),
            Terminator::CondBr {
                then_blk, else_blk, ..
            } => back(i, *then_blk) || back(i, *else_blk),
            Terminator::Ret(_) | Terminator::Unreachable => false,
        })
    }
}

/// A block the walk has left.
const FINISHED: u32 = u32::MAX;

/// The branches `(from, to)` of `func` to a block still on the
/// depth-first walk from the entry, each closing a cycle, in the order
/// the walk meets them; a block branching twice to one such block
/// gives the edge twice.
fn back_edges(func: &Function, pos: &Positions) -> Vec<(BlockId, BlockId)> {
    let mut edges = Vec::new();
    // one allocation: per position, 0 unvisited, 1 + the branches
    // taken while on the walk, FINISHED; then the walk, which never
    // grows past the blocks
    let n = func.blocks.len();
    let mut mem = vec![0u32; 2 * n];
    let (state, walk) = mem.split_at_mut(n);
    let mut depth = 1;
    walk[0] = 0;
    state[0] = 1;
    while depth > 0 {
        let p = walk[depth - 1] as usize;
        let t = match (&func.blocks[p].term, state[p]) {
            (Terminator::Br(t), 1) => *t,
            (Terminator::CondBr { then_blk, .. }, 1) => *then_blk,
            (Terminator::CondBr { else_blk, .. }, 2) => *else_blk,
            (Terminator::Br(_), _)
            | (Terminator::CondBr { .. }, _)
            | (Terminator::Ret(_) | Terminator::Unreachable, _) => {
                state[p] = FINISHED;
                depth -= 1;
                continue;
            }
        };
        state[p] += 1;
        // a target no block has is not walked
        let Some(tp) = pos.get(t) else { continue };
        match state[tp] {
            0 => {
                state[tp] = 1;
                walk[depth] = tp as u32;
                depth += 1;
            }
            FINISHED => {}
            _ => edges.push((func.blocks[p].id, t)),
        }
    }
    edges
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{Block, Operand, Type, ValueInfo};

    fn blk(id: u32, term: Terminator) -> Block {
        Block {
            id: BlockId(id),
            insts: vec![],
            term,
        }
    }

    fn cond(then_blk: u32, else_blk: u32) -> Terminator {
        Terminator::CondBr {
            cond: Operand::Value(torajs_core::ssa::ValueId(0)),
            then_blk: BlockId(then_blk),
            else_blk: BlockId(else_blk),
        }
    }

    fn func(blocks: Vec<Block>) -> Function {
        Function {
            name: "f".into(),
            params: vec![],
            ret: Type::Void,
            blocks,
            values: vec![ValueInfo {
                ty: Type::Bool,
                name: None,
            }],
            current_origin: None,
        }
    }

    /// Block `i` starts at `16 * i`.
    fn every_16(n: u32) -> HashMap<u32, u32> {
        (0..n).map(|i| (i, 16 * i)).collect()
    }

    fn edges(f: &Function) -> Vec<(u32, u32)> {
        (back_edges(f, &Positions::of(f)).into_iter())
            .map(|(a, b)| (a.0, b.0))
            .collect()
    }

    /// b0 → b1 (outer header) → b2 (inner header) → b3 → b2 | b4 →
    /// b1 | b5 (ret).
    #[test]
    fn back_edges_close_loops_shortest_first() {
        let f = func(vec![
            blk(0, Terminator::Br(BlockId(1))),
            blk(1, Terminator::Br(BlockId(2))),
            blk(2, Terminator::Br(BlockId(3))),
            blk(3, cond(2, 4)),
            blk(4, cond(1, 5)),
            blk(5, Terminator::Ret(None)),
        ]);
        assert_eq!(edges(&f), vec![(3, 2), (4, 1)]);
        let starts = [(0, 0), (1, 16), (2, 32), (3, 48), (4, 64), (5, 96)]
            .into_iter()
            .collect();
        assert_eq!(loop_ranges(&f, &starts, 112), vec![(32, 64), (16, 96)]);
    }

    /// b0 → b1 | b3; b1 → b2 (ret); b3, emitted last, → b2: a branch
    /// backwards in the emitted order that closes no cycle.
    #[test]
    fn a_cold_block_returning_to_a_join_is_no_loop() {
        let f = func(vec![
            blk(0, cond(1, 3)),
            blk(1, Terminator::Br(BlockId(2))),
            blk(2, Terminator::Ret(None)),
            blk(3, Terminator::Br(BlockId(2))),
        ]);
        assert!(edges(&f).is_empty());
        assert!(loop_ranges(&f, &every_16(4), 64).is_empty());
    }

    /// b0 → b1 (header) → b2 → b1 (hot latch) | b3; b3 → b1 (a cold
    /// arm laid out after the latch) | b4 (ret): each edge back keeps
    /// its own range.
    #[test]
    fn each_edge_back_to_a_header_keeps_its_range() {
        let f = func(vec![
            blk(0, Terminator::Br(BlockId(1))),
            blk(1, Terminator::Br(BlockId(2))),
            blk(2, cond(1, 3)),
            blk(3, cond(1, 4)),
            blk(4, Terminator::Ret(None)),
        ]);
        let starts = [(0, 0), (1, 16), (2, 32), (3, 48), (4, 96)]
            .into_iter()
            .collect();
        assert_eq!(loop_ranges(&f, &starts, 112), vec![(16, 48), (16, 96)]);
    }

    /// b0 → b1 (header) → b2, whose both arms branch back to b1: the
    /// walk meets the edge twice, and it closes one range.
    #[test]
    fn two_edges_from_one_block_close_one_range() {
        let f = func(vec![
            blk(0, Terminator::Br(BlockId(1))),
            blk(1, cond(2, 3)),
            blk(2, cond(1, 1)),
            blk(3, Terminator::Ret(None)),
        ]);
        assert_eq!(edges(&f), vec![(2, 1), (2, 1)]);
        assert_eq!(loop_ranges(&f, &every_16(4), 64), vec![(16, 48)]);
    }

    /// b0 → b1 | b2, b1 → b2 | b3, b2 → b1: a cycle with two entries.
    /// The walk enters at b1, so b2 → b1 is the edge back, and the
    /// range runs from b1 to the end of b2.
    #[test]
    fn an_irreducible_cycle_closes_the_edge_the_walk_meets() {
        let f = func(vec![
            blk(0, cond(1, 2)),
            blk(1, cond(2, 3)),
            blk(2, Terminator::Br(BlockId(1))),
            blk(3, Terminator::Ret(None)),
        ]);
        assert_eq!(edges(&f), vec![(2, 1)]);
        assert_eq!(loop_ranges(&f, &every_16(4), 64), vec![(16, 48)]);
    }

    /// Ids that are not the positions (0, 5, 2, 7) take the table:
    /// b0 → b5 (header) → b2 → b5 | b7 (ret).
    #[test]
    fn ids_out_of_position_close_the_same_range() {
        let f = func(vec![
            blk(0, Terminator::Br(BlockId(5))),
            blk(5, Terminator::Br(BlockId(2))),
            blk(2, cond(5, 7)),
            blk(7, Terminator::Ret(None)),
        ]);
        assert!(matches!(Positions::of(&f), Positions::Table(_)));
        assert_eq!(edges(&f), vec![(2, 5)]);
        let starts = [(0, 0), (5, 16), (2, 32), (7, 48)].into_iter().collect();
        assert_eq!(loop_ranges(&f, &starts, 64), vec![(16, 48)]);
    }

    /// A block the emitter left out after the latch (b3, no start):
    /// the latch ends where the next emitted block starts.
    #[test]
    fn a_latch_ends_at_the_next_emitted_block() {
        let f = func(vec![
            blk(0, Terminator::Br(BlockId(1))),
            blk(1, Terminator::Br(BlockId(2))),
            blk(2, cond(1, 3)),
            blk(3, Terminator::Br(BlockId(4))),
            blk(4, Terminator::Ret(None)),
        ]);
        let starts = [(0, 0), (1, 16), (2, 32), (4, 48)].into_iter().collect();
        assert_eq!(loop_ranges(&f, &starts, 64), vec![(16, 48)]);
    }

    /// b0 → b1 (header) → b2 | b4; b2 → b3 (an empty latch, left
    /// out) → b1; b4, laid out after the latch, → b3 | b5 (ret): each
    /// block branching back through the latch closes its own range.
    #[test]
    fn a_left_out_latch_closes_a_range_per_predecessor() {
        let f = func(vec![
            blk(0, Terminator::Br(BlockId(1))),
            blk(1, cond(2, 4)),
            blk(2, Terminator::Br(BlockId(3))),
            blk(3, Terminator::Br(BlockId(1))),
            blk(4, cond(3, 5)),
            blk(5, Terminator::Ret(None)),
        ]);
        assert_eq!(edges(&f), vec![(3, 1)]);
        let starts = [(0, 0), (1, 16), (2, 32), (4, 48), (5, 80)]
            .into_iter()
            .collect();
        assert_eq!(loop_ranges(&f, &starts, 96), vec![(16, 48), (16, 80)]);
        // with b4 ahead of the latch's own end, the range stops there
        let f = func(vec![
            blk(0, Terminator::Br(BlockId(1))),
            blk(1, cond(2, 5)),
            blk(2, Terminator::Br(BlockId(3))),
            blk(3, Terminator::Br(BlockId(1))),
            blk(5, Terminator::Ret(None)),
        ]);
        let starts = [(0, 0), (1, 16), (2, 32), (5, 48)].into_iter().collect();
        assert_eq!(loop_ranges(&f, &starts, 64), vec![(16, 48)]);
    }

    /// b0 → b1 (header) → b2 | b6 (exit); b2 → b3 | b5 (cold); b3 →
    /// b4 (an empty latch, left out) → b1; b6 → b7 (ret); b5, laid out
    /// after the exit code, → b4. The hot range ends with b3: the cold
    /// arm closes its own range over what lies between, and does not
    /// stretch the hot one.
    #[test]
    fn a_cold_arm_through_a_left_out_latch_keeps_the_hot_range() {
        let f = func(vec![
            blk(0, Terminator::Br(BlockId(1))),
            blk(1, cond(2, 6)),
            blk(2, cond(3, 5)),
            blk(3, Terminator::Br(BlockId(4))),
            blk(4, Terminator::Br(BlockId(1))),
            blk(6, Terminator::Br(BlockId(7))),
            blk(7, Terminator::Ret(None)),
            blk(5, Terminator::Br(BlockId(4))),
        ]);
        assert_eq!(edges(&f), vec![(4, 1)]);
        let starts = [(0, 0), (1, 16), (2, 32), (3, 48), (6, 64), (7, 80), (5, 96)]
            .into_iter()
            .collect();
        assert_eq!(loop_ranges(&f, &starts, 112), vec![(16, 64), (16, 112)]);
    }

    /// b0 → b1 (header) | b2; b1 → b3 (an empty latch, left out) |
    /// b4 (ret); b3 → b1; b2, laid out ahead of the header, → b3:
    /// entering the loop through the latch closes nothing.
    #[test]
    fn a_predecessor_ahead_of_the_header_closes_nothing() {
        let f = func(vec![
            blk(0, cond(1, 2)),
            blk(2, Terminator::Br(BlockId(3))),
            blk(1, cond(3, 4)),
            blk(3, Terminator::Br(BlockId(1))),
            blk(4, Terminator::Ret(None)),
        ]);
        assert_eq!(edges(&f), vec![(3, 1)]);
        let starts = [(0, 0), (2, 16), (1, 32), (4, 48)].into_iter().collect();
        assert_eq!(loop_ranges(&f, &starts, 64), vec![(32, 48)]);
    }

    /// A latch emitted last ends at the function's end.
    #[test]
    fn a_last_latch_ends_at_the_end() {
        let f = func(vec![
            blk(0, Terminator::Br(BlockId(1))),
            blk(1, Terminator::Br(BlockId(2))),
            blk(2, Terminator::Br(BlockId(1))),
        ]);
        assert_eq!(loop_ranges(&f, &every_16(3), 40), vec![(16, 40)]);
    }
}
