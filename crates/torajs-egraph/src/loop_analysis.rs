//! Natural loop detection — textbook Aho/Sethi/Ullman "Compilers" §10.4
//! algorithm:
//!
//!   1. Identify back-edges:  CFG edge (n → h) where `h` dominates `n`.
//!   2. For each back-edge, the natural loop is the maximal set of
//!      blocks from which `n` is reachable without going through `h`,
//!      plus `h` itself. Equivalently: reverse-BFS from `n` in the
//!      CFG, stopping at `h`.
//!   3. Loop-nest depth of a block = number of natural loops that
//!      contain it.
//!
//! This module gives the elaboration phase the data it needs to
//! decide LICM (hoist a pure value out of the loop if its body has
//! no loop-internal data dependency) and the cost-model loop-nest
//! scaling (`Cost::scale_for_depth`). Preheader detection / preheader
//! synthesis is deferred to elaboration — Phase 0 only computes the
//! information; transforms come in Phase 1.
//!
//! Reference impls:
//! - LLVM `lib/Analysis/LoopInfo.cpp` (more complex; full hierarchy)
//! - Cranelift `cranelift/codegen/src/loop_analysis.rs` (closer to
//!   what we do here — single-pass back-edge collection + body
//!   discovery + nest-depth assignment)

use crate::dominator::DominatorTree;
use torajs_core::ssa::{BlockId, Function, Terminator};

/// One natural loop, identified by its header block. A header may
/// have multiple back-edges into it (irreducible-CFG-adjacent
/// constructs); in that case all blocks reachable from any back-edge
/// source up to the header form a single combined loop body.
#[derive(Debug, Clone)]
pub struct NaturalLoop {
    /// The header block — the unique entry into the loop body and
    /// the immediate dominator of every other block in the loop.
    pub header: BlockId,
    /// Every block in the loop, including the header. Sorted by
    /// `BlockId` for determinism.
    pub body: Vec<BlockId>,
    /// Back-edge source blocks (the `n` in each `n → header` back-edge).
    /// Sorted by `BlockId`.
    pub back_edge_sources: Vec<BlockId>,
}

/// Loop information for a function — list of natural loops plus a
/// per-block nest-depth vector.
#[derive(Debug, Clone)]
pub struct LoopAnalysis {
    loops: Vec<NaturalLoop>,
    /// `nest_depth[i]` = number of natural loops containing block `i`.
    /// 0 means block is not inside any loop. Indexed by `BlockId.0`.
    nest_depth: Vec<u32>,
}

impl LoopAnalysis {
    /// Run natural-loop detection. Requires a precomputed dominator
    /// tree (the same one the optimize / elaborate phases use).
    pub fn compute(func: &Function, dom: &DominatorTree) -> Self {
        let n = func.blocks.len();
        let mut loops: Vec<NaturalLoop> = Vec::new();
        let mut nest_depth = vec![0u32; n];

        if n == 0 {
            return Self { loops, nest_depth };
        }

        // Step 1+2: walk every CFG edge; for each (u, v) where v
        // dominates u, that's a back-edge. Group back-edges by
        // header so a multi-back-edge loop becomes one NaturalLoop.
        let mut headers_seen: Vec<BlockId> = Vec::new();
        let mut per_header_back_sources: Vec<Vec<BlockId>> = Vec::new();
        for blk in &func.blocks {
            for succ in successors(&blk.term) {
                if dom.dominates(succ, blk.id) {
                    let idx = match headers_seen.iter().position(|h| *h == succ) {
                        Some(i) => i,
                        None => {
                            headers_seen.push(succ);
                            per_header_back_sources.push(Vec::new());
                            headers_seen.len() - 1
                        }
                    };
                    per_header_back_sources[idx].push(blk.id);
                }
            }
        }

        // For each header, collect the natural-loop body via reverse
        // BFS from each back-edge source, stopping at the header.
        for (header, sources) in headers_seen.iter().zip(per_header_back_sources.iter()) {
            let body = collect_loop_body(func, *header, sources);
            let mut sorted_sources = sources.clone();
            sorted_sources.sort_by_key(|b| b.0);
            sorted_sources.dedup();
            loops.push(NaturalLoop {
                header: *header,
                body,
                back_edge_sources: sorted_sources,
            });
        }

        // Step 3: nest depth = count of containing loops per block.
        for lp in &loops {
            for b in &lp.body {
                let i = b.0 as usize;
                if i < nest_depth.len() {
                    nest_depth[i] += 1;
                }
            }
        }

        // Sort loops by header for determinism.
        loops.sort_by_key(|l| l.header.0);

        Self { loops, nest_depth }
    }

    /// All natural loops, sorted by header `BlockId`.
    pub fn loops(&self) -> &[NaturalLoop] {
        &self.loops
    }

    /// Nest depth of block `b` — number of natural loops that
    /// contain it. Returns 0 for blocks outside every loop or for
    /// out-of-range / unreachable blocks.
    pub fn nest_depth(&self, b: BlockId) -> u32 {
        let i = b.0 as usize;
        if i < self.nest_depth.len() {
            self.nest_depth[i]
        } else {
            0
        }
    }

    /// True iff `b` is the header of some natural loop.
    pub fn is_loop_header(&self, b: BlockId) -> bool {
        self.loops.iter().any(|l| l.header == b)
    }
}

// ---- internals ------------------------------------------------------------

fn successors(term: &Terminator) -> impl Iterator<Item = BlockId> + '_ {
    let mut out: [Option<BlockId>; 2] = [None, None];
    match term {
        Terminator::Br(b) => out[0] = Some(*b),
        Terminator::CondBr {
            then_blk, else_blk, ..
        } => {
            out[0] = Some(*then_blk);
            out[1] = Some(*else_blk);
        }
        Terminator::Ret(_) | Terminator::Unreachable => {}
    }
    out.into_iter().flatten()
}

/// Reverse-BFS from each back-edge source, stopping at the header.
/// Result includes the header itself plus every block on some path
/// from a source up to (but not crossing) the header.
fn collect_loop_body(func: &Function, header: BlockId, sources: &[BlockId]) -> Vec<BlockId> {
    let n = func.blocks.len();
    // Pred map limited to blocks that can reach back into the loop.
    let mut preds: Vec<Vec<BlockId>> = vec![Vec::new(); n];
    for blk in &func.blocks {
        for succ in successors(&blk.term) {
            let si = succ.0 as usize;
            if si < n {
                preds[si].push(blk.id);
            }
        }
    }
    let mut in_body = vec![false; n];
    let mut stack: Vec<BlockId> = Vec::new();
    let hi = header.0 as usize;
    if hi < n {
        in_body[hi] = true; // header is always in its own loop body
    }
    for src in sources {
        let si = src.0 as usize;
        if si < n && !in_body[si] {
            in_body[si] = true;
            stack.push(*src);
        }
    }
    while let Some(b) = stack.pop() {
        let bi = b.0 as usize;
        for p in &preds[bi] {
            let pi = p.0 as usize;
            if pi < n && !in_body[pi] {
                in_body[pi] = true;
                stack.push(*p);
            }
        }
    }
    let mut body: Vec<BlockId> = (0..n)
        .filter(|&i| in_body[i])
        .map(|i| BlockId(i as u32))
        .collect();
    body.sort_by_key(|b| b.0);
    body
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dominator::DominatorTree;
    use torajs_core::ssa::{Block, Function, Operand, Terminator, Type};

    fn fixture(blocks: Vec<Block>) -> Function {
        Function {
            name: "test".to_string(),
            params: vec![],
            ret: Type::Void,
            blocks,
            values: vec![],
            current_origin: None,
        }
    }

    fn block(id: u32, term: Terminator) -> Block {
        Block {
            id: BlockId(id),
            insts: vec![],
            term,
        }
    }

    #[test]
    fn no_loops_in_straight_line() {
        let func = fixture(vec![
            block(0, Terminator::Br(BlockId(1))),
            block(1, Terminator::Br(BlockId(2))),
            block(2, Terminator::Ret(None)),
        ]);
        let dom = DominatorTree::compute(&func);
        let la = LoopAnalysis::compute(&func, &dom);
        assert!(la.loops().is_empty());
        for b in 0..3 {
            assert_eq!(la.nest_depth(BlockId(b)), 0);
        }
    }

    #[test]
    fn simple_while_loop() {
        // 0 -> 1 (header); 1 (cond) -> 2 (body), 3 (exit); 2 -> 1
        let func = fixture(vec![
            block(0, Terminator::Br(BlockId(1))),
            block(
                1,
                Terminator::CondBr {
                    cond: Operand::ConstBool(true),
                    then_blk: BlockId(2),
                    else_blk: BlockId(3),
                },
            ),
            block(2, Terminator::Br(BlockId(1))),
            block(3, Terminator::Ret(None)),
        ]);
        let dom = DominatorTree::compute(&func);
        let la = LoopAnalysis::compute(&func, &dom);
        assert_eq!(la.loops().len(), 1);
        let lp = &la.loops()[0];
        assert_eq!(lp.header, BlockId(1));
        assert_eq!(lp.body, vec![BlockId(1), BlockId(2)]);
        assert_eq!(lp.back_edge_sources, vec![BlockId(2)]);
        // Nest depth: header + body block = 1; entry + exit = 0.
        assert_eq!(la.nest_depth(BlockId(0)), 0);
        assert_eq!(la.nest_depth(BlockId(1)), 1);
        assert_eq!(la.nest_depth(BlockId(2)), 1);
        assert_eq!(la.nest_depth(BlockId(3)), 0);
        assert!(la.is_loop_header(BlockId(1)));
        assert!(!la.is_loop_header(BlockId(2)));
    }

    #[test]
    fn nested_loops_compose_depth() {
        // Outer 1, inner 3:
        //   0 -> 1 (outer header) (cond) -> 2 (after), 3 (inner header)
        //   3 (cond) -> 4 (inner body), 5 (inner exit)
        //   4 -> 3 (inner back-edge)
        //   5 -> 1 (outer back-edge)
        //   2 -> ret
        let func = fixture(vec![
            block(0, Terminator::Br(BlockId(1))),
            block(
                1,
                Terminator::CondBr {
                    cond: Operand::ConstBool(true),
                    then_blk: BlockId(2),
                    else_blk: BlockId(3),
                },
            ),
            block(2, Terminator::Ret(None)),
            block(
                3,
                Terminator::CondBr {
                    cond: Operand::ConstBool(true),
                    then_blk: BlockId(4),
                    else_blk: BlockId(5),
                },
            ),
            block(4, Terminator::Br(BlockId(3))),
            block(5, Terminator::Br(BlockId(1))),
        ]);
        let dom = DominatorTree::compute(&func);
        let la = LoopAnalysis::compute(&func, &dom);
        assert_eq!(la.loops().len(), 2, "expect outer + inner loop");
        let inner = la.loops().iter().find(|l| l.header == BlockId(3)).unwrap();
        let outer = la.loops().iter().find(|l| l.header == BlockId(1)).unwrap();
        assert_eq!(inner.body, vec![BlockId(3), BlockId(4)]);
        // Outer body = header 1 + inner header 3 + inner body 4 +
        // inner exit 5. Block 2 is the after-loop branch, not in body.
        assert_eq!(
            outer.body,
            vec![BlockId(1), BlockId(3), BlockId(4), BlockId(5)]
        );
        assert_eq!(la.nest_depth(BlockId(0)), 0);
        assert_eq!(la.nest_depth(BlockId(1)), 1); // outer header
        assert_eq!(la.nest_depth(BlockId(2)), 0); // after-loop
        assert_eq!(la.nest_depth(BlockId(3)), 2); // inner header inside outer
        assert_eq!(la.nest_depth(BlockId(4)), 2); // inner body inside outer
        assert_eq!(la.nest_depth(BlockId(5)), 1); // inner exit, still outer
    }

    #[test]
    fn multiple_back_edges_one_loop() {
        // Header has two back-edges (e.g. continue and end-of-iter):
        //   0 -> 1 (header); 1 (cond) -> 2, 3 (exit)
        //   2 (cond) -> 1 (back A), 4
        //   4 -> 1 (back B)
        //   3 -> ret
        let func = fixture(vec![
            block(0, Terminator::Br(BlockId(1))),
            block(
                1,
                Terminator::CondBr {
                    cond: Operand::ConstBool(true),
                    then_blk: BlockId(2),
                    else_blk: BlockId(3),
                },
            ),
            block(
                2,
                Terminator::CondBr {
                    cond: Operand::ConstBool(true),
                    then_blk: BlockId(1),
                    else_blk: BlockId(4),
                },
            ),
            block(3, Terminator::Ret(None)),
            block(4, Terminator::Br(BlockId(1))),
        ]);
        let dom = DominatorTree::compute(&func);
        let la = LoopAnalysis::compute(&func, &dom);
        assert_eq!(la.loops().len(), 1, "two back-edges → one combined loop");
        let lp = &la.loops()[0];
        assert_eq!(lp.header, BlockId(1));
        assert_eq!(lp.back_edge_sources, vec![BlockId(2), BlockId(4)]);
        // Body = header + every source that reaches it = 1, 2, 4.
        assert_eq!(lp.body, vec![BlockId(1), BlockId(2), BlockId(4)]);
        assert_eq!(la.nest_depth(BlockId(3)), 0);
    }

    #[test]
    fn empty_function_is_loop_free() {
        let func = fixture(vec![]);
        let dom = DominatorTree::compute(&func);
        let la = LoopAnalysis::compute(&func, &dom);
        assert!(la.loops().is_empty());
        assert_eq!(la.nest_depth(BlockId(0)), 0);
        assert!(!la.is_loop_header(BlockId(0)));
    }
}
