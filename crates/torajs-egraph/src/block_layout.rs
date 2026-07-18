//! Loop-body contiguity layout — sink blocks that are positionally
//! inside a natural loop's block range but not part of its body to
//! just after the range (LLVM MachineBlockPlacement's loop-cohesion
//! aspect, reduced to the one distortion our lowering produces).
//!
//! The lowerer + pass pipeline can leave cold blocks (function exit
//! epilogues, loop-exit stitches) positioned between a loop's header
//! and its latch. That costs twice downstream:
//!
//! * codegen's fall-through elision cannot chain the loop body, so
//!   the body pays taken branches detouring around the cold blocks
//!   every iteration;
//! * liveness intervals and spill weights are positional — a call in
//!   an interleaved exit block makes every loop-carried cell look
//!   call-crossing (callee-pool-only, spill-prone) even though no
//!   call runs on the loop path (the popcount count-cell spill).
//!
//! Reordering is semantics-free (branches are explicit; codegen
//! resolves fall-throughs against the final order). Inner loops are
//! processed before outer ones so a nested body stays one contiguous
//! unit inside its parent's partition. Block ids are renumbered to
//! keep the `BlockId.0 == position` invariant.
//!
//! `TORAJS_BLOCK_LAYOUT_OFF=1` skips (bisect gate),
//! `TORAJS_BLOCK_LAYOUT_STATS=1` dumps counters.

use std::collections::{HashMap, HashSet};

use torajs_core::ssa::{Block, BlockId, Function, Module, Terminator};

use crate::dominator::DominatorTree;
use crate::loop_analysis::LoopAnalysis;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct BlockLayoutStats {
    /// Blocks sunk out of a loop's position range.
    pub blocks_sunk: u32,
    /// Functions whose block order changed.
    pub funcs_relaid: u32,
}

/// Restore loop-body contiguity in every function body.
pub fn layout_module(module: &mut Module) -> BlockLayoutStats {
    let mut stats = BlockLayoutStats::default();
    for func in module.funcs.iter_mut() {
        if func.is_declaration() {
            continue;
        }
        let sunk = layout_function(func);
        if sunk > 0 {
            stats.blocks_sunk += sunk;
            stats.funcs_relaid += 1;
        }
    }
    stats
}

fn layout_function(func: &mut Function) -> u32 {
    let dom = DominatorTree::compute(func);
    let la = LoopAnalysis::compute(func, &dom);
    if la.loops().is_empty() {
        return 0;
    }
    // Loop body sets, innermost (smallest) first so nested bodies are
    // already contiguous units when their parent partitions.
    let mut bodies: Vec<HashSet<u32>> = la
        .loops()
        .iter()
        .map(|l| l.body.iter().map(|b| b.0).collect())
        .collect();
    bodies.sort_by_key(|b| b.len());

    // Work over original ids; positions recompute after each rewrite.
    let mut order: Vec<u32> = func.blocks.iter().map(|b| b.id.0).collect();
    let mut sunk = 0u32;
    for body in &bodies {
        let positions: Vec<usize> = order
            .iter()
            .enumerate()
            .filter(|(_, id)| body.contains(id))
            .map(|(i, _)| i)
            .collect();
        let (Some(&lo), Some(&hi)) = (positions.first(), positions.last()) else {
            continue;
        };
        if hi - lo + 1 == body.len() {
            continue; // already contiguous
        }
        // Partition [lo..=hi]: body blocks first, interlopers after,
        // both keeping their relative order.
        let slice: Vec<u32> = order[lo..=hi].to_vec();
        let (members, cold): (Vec<u32>, Vec<u32>) =
            slice.into_iter().partition(|id| body.contains(id));
        sunk += cold.len() as u32;
        order.splice(lo..=hi, members.into_iter().chain(cold));
    }
    if sunk == 0 {
        return 0;
    }

    // Rebuild func.blocks in the new order, renumbering ids and
    // terminator targets (BlockId.0 == position invariant).
    let remap: HashMap<u32, u32> = order
        .iter()
        .enumerate()
        .map(|(new, old)| (*old, new as u32))
        .collect();
    let mut by_id: HashMap<u32, Block> = func.blocks.drain(..).map(|b| (b.id.0, b)).collect();
    for old in &order {
        let mut block = by_id.remove(old).expect("layout order covers every block");
        block.id = BlockId(remap[&block.id.0]);
        match &mut block.term {
            Terminator::Br(t) => t.0 = remap[&t.0],
            Terminator::CondBr {
                then_blk, else_blk, ..
            } => {
                then_blk.0 = remap[&then_blk.0];
                else_blk.0 = remap[&else_blk.0];
            }
            _ => {}
        }
        func.blocks.push(block);
    }
    sunk
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{IPred, Inst, InstKind, Operand, Type, ValueId, ValueInfo};

    fn block(id: u32, insts: Vec<Inst>, term: Terminator) -> Block {
        Block {
            id: BlockId(id),
            insts,
            term,
        }
    }

    fn inst(result: u32, kind: InstKind) -> Inst {
        Inst {
            result: Some(ValueId(result)),
            kind,
            origin: None,
        }
    }

    /// The popcount shape: exit block b3 (with the cold epilogue)
    /// sits between the loop body blocks {b1, b2, b4} — it must sink
    /// after the latch and the body must renumber contiguously.
    ///
    ///   b0 → b1(hdr: cond ? b2 : b3) ; b2 → b4 ; b3: ret ; b4 → b1
    #[test]
    fn interleaved_exit_block_sinks_after_loop() {
        let mut f = Function {
            name: "f".into(),
            params: vec![],
            ret: Type::Void,
            values: vec![
                ValueInfo {
                    ty: Type::I64,
                    name: None
                };
                2
            ],
            blocks: vec![
                block(0, vec![], Terminator::Br(BlockId(1))),
                block(
                    1,
                    vec![inst(
                        1,
                        InstKind::ICmp(
                            IPred::Slt,
                            Operand::Value(ValueId(0)),
                            Operand::ConstI64(9),
                        ),
                    )],
                    Terminator::CondBr {
                        cond: Operand::Value(ValueId(1)),
                        then_blk: BlockId(2),
                        else_blk: BlockId(3),
                    },
                ),
                block(2, vec![], Terminator::Br(BlockId(4))),
                block(3, vec![], Terminator::Ret(None)),
                block(4, vec![], Terminator::Br(BlockId(1))),
            ],
            current_origin: None,
        };
        let sunk = layout_function(&mut f);
        assert_eq!(sunk, 1);
        // new order: b0, hdr, body, latch, exit — ids renumbered to
        // positions, header's else edge follows the exit block.
        for (i, b) in f.blocks.iter().enumerate() {
            assert_eq!(b.id.0 as usize, i);
        }
        assert!(matches!(f.blocks[0].term, Terminator::Br(BlockId(1))));
        let Terminator::CondBr {
            then_blk, else_blk, ..
        } = f.blocks[1].term
        else {
            panic!("header keeps its CondBr");
        };
        assert_eq!(then_blk, BlockId(2), "body right after header");
        assert_eq!(else_blk, BlockId(4), "exit sunk after the latch");
        assert!(
            matches!(f.blocks[3].term, Terminator::Br(BlockId(1))),
            "latch back edge follows the body"
        );
        assert!(matches!(f.blocks[4].term, Terminator::Ret(None)));
    }

    /// An already-contiguous loop must not be touched.
    #[test]
    fn contiguous_loop_is_untouched() {
        let mut f = Function {
            name: "f".into(),
            params: vec![],
            ret: Type::Void,
            values: vec![
                ValueInfo {
                    ty: Type::I64,
                    name: None
                };
                2
            ],
            blocks: vec![
                block(0, vec![], Terminator::Br(BlockId(1))),
                block(
                    1,
                    vec![inst(
                        1,
                        InstKind::ICmp(
                            IPred::Slt,
                            Operand::Value(ValueId(0)),
                            Operand::ConstI64(9),
                        ),
                    )],
                    Terminator::CondBr {
                        cond: Operand::Value(ValueId(1)),
                        then_blk: BlockId(2),
                        else_blk: BlockId(3),
                    },
                ),
                block(2, vec![], Terminator::Br(BlockId(1))),
                block(3, vec![], Terminator::Ret(None)),
            ],
            current_origin: None,
        };
        assert_eq!(layout_function(&mut f), 0);
        for (i, b) in f.blocks.iter().enumerate() {
            assert_eq!(b.id.0 as usize, i);
        }
    }
}
