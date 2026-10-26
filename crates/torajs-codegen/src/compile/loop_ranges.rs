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
/// block). Each edge back to one header keeps its own range: block
/// layout puts a loop's cold arms after its hot latch, and one range
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
    // a loop needs a branch to a block emitted at or before its own:
    // most functions have none, and are answered before anything is
    // allocated
    let start = |b: BlockId| block_byte_starts.get(&b.0).copied();
    let backward = func.blocks.iter().any(|block| {
        start(block.id).is_some_and(|from| {
            (targets(&block.term).into_iter().flatten())
                .any(|t| start(t).is_some_and(|to| to <= from))
        })
    });
    if !backward {
        return Vec::new();
    }
    let mut starts: Vec<u32> = block_byte_starts.values().copied().collect();
    starts.sort_unstable();
    starts.dedup();
    // the end of the block starting at `from`: the first start past
    // it, or the function's end
    let block_end = |from: u32| {
        let i = starts.partition_point(|&s| s <= from);
        starts.get(i).copied().unwrap_or(end)
    };
    // shortest first, as (length, start) pairs, a total order a plain
    // tuple sort keeps; two edges from one block close one range
    let mut by_len: Vec<(u32, u32)> = back_edges(func)
        .into_iter()
        .filter_map(|(from, to)| {
            let (from, to) = (start(from)?, start(to)?);
            // block layout lays a loop out from its header and never
            // rotates it (egraph `block_layout`), so a back edge's
            // target is emitted first; one that is not names no
            // contiguous range
            (to <= from).then(|| (block_end(from) - to, to))
        })
        .collect();
    by_len.sort_unstable();
    by_len.dedup();
    by_len.into_iter().map(|(len, lo)| (lo, lo + len)).collect()
}

/// The branches `(from, to)` of `func` to a block still on the
/// depth-first walk from the entry, each closing a cycle, in the order
/// the walk meets them; a block branching twice to one such block
/// gives the edge twice.
fn back_edges(func: &Function) -> Vec<(BlockId, BlockId)> {
    let span = func
        .blocks
        .iter()
        .map(|b| b.id.0 as usize + 1)
        .max()
        .unwrap_or(0);
    let mut succ: Vec<[Option<BlockId>; 2]> = vec![[None, None]; span];
    for block in &func.blocks {
        succ[block.id.0 as usize] = targets(&block.term);
    }
    let mut edges = Vec::new();
    // 0 unvisited, 1 on the walk, 2 finished
    let mut state = vec![0u8; span];
    let entry = func.blocks[0].id;
    let mut walk: Vec<(BlockId, usize)> = vec![(entry, 0)];
    state[entry.0 as usize] = 1;
    while let Some((b, next)) = walk.last_mut() {
        let b = *b;
        let Some(t) = succ[b.0 as usize].get(*next).copied() else {
            state[b.0 as usize] = 2;
            walk.pop();
            continue;
        };
        *next += 1;
        let Some(t) = t else { continue };
        match state.get(t.0 as usize) {
            Some(0) => {
                state[t.0 as usize] = 1;
                walk.push((t, 0));
            }
            Some(1) => edges.push((b, t)),
            _ => {}
        }
    }
    edges
}

fn targets(term: &Terminator) -> [Option<BlockId>; 2] {
    match term {
        Terminator::Br(t) => [Some(*t), None],
        Terminator::CondBr {
            then_blk, else_blk, ..
        } => [Some(*then_blk), Some(*else_blk)],
        Terminator::Ret(_) | Terminator::Unreachable => [None, None],
    }
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
        back_edges(f).into_iter().map(|(a, b)| (a.0, b.0)).collect()
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
