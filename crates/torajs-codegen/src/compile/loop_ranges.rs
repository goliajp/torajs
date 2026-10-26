//! The byte range of every loop of a compiled function, for the
//! linker's placement.

use torajs_core::ssa::id_hash::HashMap;
use torajs_core::ssa::{BlockId, Function, Terminator};

/// The byte ranges of the loops: each back edge (a branch to a block
/// still on the depth-first walk from the entry, so it closes a cycle)
/// closes a range from its target's first byte to the end of the
/// branching block (the block after it, or the function's end). Each
/// edge back to one header keeps its own range: block layout puts a
/// loop's cold arms after its hot latch, and one range over every
/// edge would take them in, past what a region holds, and leave the
/// hot path unguarded. A branch that only jumps backwards in the
/// emitted order (a cold block at the end returning to a join) closes
/// nothing. Read off the walk and the emitted order rather than from a
/// dominator tree: block layout has already made every loop body
/// contiguous from its header, and this runs on every function of
/// every build. Shortest ranges first, so a placement that cannot keep
/// every range in one region keeps the inner ones.
pub(super) fn loop_ranges(
    func: &Function,
    block_byte_starts: &HashMap<u32, u32>,
    end: u32,
) -> Vec<(u32, u32)> {
    let targets = |term: &Terminator| match term {
        Terminator::Br(t) => [Some(*t), None],
        Terminator::CondBr {
            then_blk, else_blk, ..
        } => [Some(*then_blk), Some(*else_blk)],
        Terminator::Ret(_) | Terminator::Unreachable => [None, None],
    };
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
    let mut starts: Vec<u32> = block_byte_starts.values().copied().collect();
    starts.sort_unstable();
    starts.dedup();
    // the end of the block starting at `from`: the next start, or the function's end
    let block_end = |from: u32| match starts.binary_search(&from) {
        Ok(i) => starts.get(i + 1).copied().unwrap_or(end),
        Err(_) => end,
    };
    let mut loops: Vec<(u32, u32)> = Vec::new();
    let mut back_edge = |from: BlockId, to: BlockId| {
        let (Some(&from), Some(&to)) =
            (block_byte_starts.get(&from.0), block_byte_starts.get(&to.0))
        else {
            return;
        };
        // block layout lays a loop out from its header and never
        // rotates it (egraph `block_layout`), so a back edge's target
        // is emitted first; one that is not names no contiguous range
        if to > from {
            return;
        }
        loops.push((to, block_end(from)));
    };
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
            Some(1) => back_edge(b, t),
            _ => {}
        }
    }
    // shortest first, as (length, start) pairs, a total order a plain
    // tuple sort keeps; two edges from one block close one range
    let mut by_len: Vec<(u32, u32)> = loops.iter().map(|&(lo, hi)| (hi - lo, lo)).collect();
    by_len.sort_unstable();
    by_len.dedup();
    by_len.into_iter().map(|(len, lo)| (lo, lo + len)).collect()
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

    /// b0 → b1 (outer header) → b2 (inner header) → b3 → b2 | b4 →
    /// b1 | b5 (ret).
    #[test]
    fn back_edges_close_loops_shortest_first() {
        let f = Function {
            name: "f".into(),
            params: vec![],
            ret: Type::Void,
            blocks: vec![
                blk(0, Terminator::Br(BlockId(1))),
                blk(1, Terminator::Br(BlockId(2))),
                blk(2, Terminator::Br(BlockId(3))),
                blk(3, cond(2, 4)),
                blk(4, cond(1, 5)),
                blk(5, Terminator::Ret(None)),
            ],
            values: vec![ValueInfo {
                ty: Type::Bool,
                name: None,
            }],
            current_origin: None,
        };
        let starts = [(0, 0), (1, 16), (2, 32), (3, 48), (4, 64), (5, 96)]
            .into_iter()
            .collect();
        assert_eq!(loop_ranges(&f, &starts, 112), vec![(32, 64), (16, 96)]);
    }

    /// b0 → b1 | b3; b1 → b2 (ret); b3, emitted last, → b2: a branch
    /// backwards in the emitted order that closes no cycle.
    #[test]
    fn a_cold_block_returning_to_a_join_is_no_loop() {
        let f = Function {
            name: "f".into(),
            params: vec![],
            ret: Type::Void,
            blocks: vec![
                blk(0, cond(1, 3)),
                blk(1, Terminator::Br(BlockId(2))),
                blk(2, Terminator::Ret(None)),
                blk(3, Terminator::Br(BlockId(2))),
            ],
            values: vec![ValueInfo {
                ty: Type::Bool,
                name: None,
            }],
            current_origin: None,
        };
        let starts = [(0, 0), (1, 16), (2, 32), (3, 48)].into_iter().collect();
        assert!(loop_ranges(&f, &starts, 64).is_empty());
    }

    /// b0 → b1 (header) → b2 → b1 (hot latch) | b3; b3 → b1 (a cold
    /// arm laid out after the latch) | b4 (ret): each edge back keeps
    /// its own range.
    #[test]
    fn each_edge_back_to_a_header_keeps_its_range() {
        let f = Function {
            name: "f".into(),
            params: vec![],
            ret: Type::Void,
            blocks: vec![
                blk(0, Terminator::Br(BlockId(1))),
                blk(1, Terminator::Br(BlockId(2))),
                blk(2, cond(1, 3)),
                blk(3, cond(1, 4)),
                blk(4, Terminator::Ret(None)),
            ],
            values: vec![ValueInfo {
                ty: Type::Bool,
                name: None,
            }],
            current_origin: None,
        };
        let starts = [(0, 0), (1, 16), (2, 32), (3, 48), (4, 96)]
            .into_iter()
            .collect();
        assert_eq!(loop_ranges(&f, &starts, 112), vec![(16, 48), (16, 96)]);
    }
}
