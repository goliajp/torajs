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

use crate::block_chain::{BlockSet, ChainFacts, ColdEdges, DomView};
use crate::tail_dup::Copied;

use torajs_core::ssa::{Block, BlockId, Function, Module, Terminator};

use torajs_core::ssa::dominator::DominatorTree;
use torajs_core::ssa::loop_analysis::LoopAnalysis;

mod forwarders;
use forwarders::{fold_bare_branches, thread_empty_forwarders};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct BlockLayoutStats {
    /// Blocks sunk out of a loop's position range.
    pub blocks_sunk: u32,
    /// Functions whose block order changed.
    pub funcs_relaid: u32,
    /// Blocks nothing could reach, dropped before laying the rest out.
    pub blocks_pruned: u32,
    /// Latches moved to the bottom of their loop's range.
    pub latches_sunk: u32,
    /// Loops whose body was reordered into hot-successor chains.
    pub loops_chained: u32,
    /// Blocks copied into the extra arms of small joins (`tail_dup`).
    pub blocks_duplicated: u32,
}

/// Restore loop-body contiguity in every function body.
pub fn layout_module(module: &mut Module) -> BlockLayoutStats {
    let mut stats = BlockLayoutStats::default();
    for func in module.funcs.iter_mut() {
        if func.is_declaration() {
            continue;
        }
        // 507-04 — an empty block whose terminator is a bare `br` is
        // pure forwarding. Codegen already threads through it, so the
        // block itself is unreachable in the MACHINE cfg and is
        // emitted as a `b` nothing jumps to (measured: two of them,
        // 8 B, on every loop whose versioning guard got folded). It
        // stays in the SSA cfg because the edge is still spelled
        // there, so `branch_fold`'s entry-reachability sweep does not
        // see it. Thread the edges here — this is the last pass that
        // touches the cfg — and the sweep that follows then really can
        // drop them.
        stats.blocks_pruned += thread_empty_forwarders(func);
        stats.blocks_pruned += fold_bare_branches(func);
        let dom = DominatorTree::compute(func);
        let la = LoopAnalysis::compute(func, &dom);
        // before the chain, which then lays each copy after its arm
        let copied = crate::tail_dup::duplicate_small_joins(func, &dom, &la);
        stats.blocks_duplicated += copied.blocks();
        let (sunk, latches, chained) = layout_after(func, &dom, &la, copied);
        if sunk + latches + chained > 0 {
            stats.blocks_sunk += sunk;
            stats.latches_sunk += latches;
            stats.loops_chained += chained;
            stats.funcs_relaid += 1;
        }
    }
    stats
}

/// Answers (blocks sunk out of loop ranges, latches moved to a loop's
/// bottom, loops chained).
#[cfg(test)]
fn layout_function(func: &mut Function) -> (u32, u32, u32) {
    let dom = DominatorTree::compute(func);
    let la = LoopAnalysis::compute(func, &dom);
    layout_with(func, DomView::whole(&dom), loop_sets(&la), None)
}

/// Lay out the function as tail duplication left it, given the
/// dominators and loops from before the copies; tail duplication says
/// how they change, and the old cold edges are kept when it says they
/// still hold.
fn layout_after(
    func: &mut Function,
    dom: &DominatorTree,
    la: &LoopAnalysis,
    copied: Copied,
) -> (u32, u32, u32) {
    if copied.joins.is_empty() {
        return layout_with(func, DomView::whole(dom), loop_sets(la), None);
    }
    let Some(idoms) = crate::tail_dup::idoms_after(func, dom, &copied) else {
        let dom = DominatorTree::compute(func);
        let la = LoopAnalysis::compute(func, &dom);
        return layout_with(func, DomView::whole(&dom), loop_sets(&la), None);
    };
    let mut loops = loop_sets(la);
    crate::tail_dup::add_to_loops(&mut loops, &copied);
    let dom = DomView::patched(dom, &idoms);
    layout_with(func, dom, loops, copied.cold_edges)
}

/// Every loop's body, header and back-edge sources.
fn loop_sets(la: &LoopAnalysis) -> Vec<(BlockSet, u32, Vec<u32>)> {
    (la.loops().iter())
        .map(|l| {
            let body = BlockSet::of(l.body.iter().map(|b| b.0));
            let latches = l.back_edge_sources.iter().map(|b| b.0).collect();
            (body, l.header.0, latches)
        })
        .collect()
}

/// Lay the function out with its dominators and loops in hand, and its
/// cold edges if already read.
fn layout_with(
    func: &mut Function,
    dom: DomView,
    mut loops: Vec<(BlockSet, u32, Vec<u32>)>,
    cold_edges: Option<ColdEdges>,
) -> (u32, u32, u32) {
    if loops.is_empty() {
        return (0, 0, 0);
    }
    let facts = match cold_edges {
        Some(edges) => ChainFacts::with_cold_edges(dom, edges),
        None => ChainFacts::new(func, dom),
    };
    // innermost (smallest) first so nested bodies are already
    // contiguous units when their parent partitions
    loops.sort_by_key(|(b, _, _)| b.len());

    // Work over original ids; positions recompute after each rewrite.
    let mut order: Vec<u32> = func.blocks.iter().map(|b| b.id.0).collect();
    let mut sunk = 0u32;
    let mut latches_sunk = 0u32;
    let mut chained = 0u32;
    for (k, (body, header, latches)) in loops.iter().enumerate() {
        let lo = order.iter().position(|&id| body.contains(id));
        let hi = order.iter().rposition(|&id| body.contains(id));
        let (Some(lo), Some(hi)) = (lo, hi) else {
            continue;
        };
        if hi - lo + 1 != body.len() {
            // Partition [lo..=hi]: body blocks first, interlopers
            // after, both keeping their relative order.
            let slice: Vec<u32> = order[lo..=hi].to_vec();
            let (members, cold): (Vec<u32>, Vec<u32>) =
                slice.into_iter().partition(|id| body.contains(*id));
            sunk += cold.len() as u32;
            order.splice(lo..=hi, members.into_iter().chain(cold));
        }
        // The loops directly inside this one (not their own nests).
        let inner: Vec<(&BlockSet, u32)> = loops[..k]
            .iter()
            .map(|(b, h, _)| (b, *h))
            .filter(|(b, _)| b.len() < body.len() && b.is_subset(body))
            .collect();
        let children: Vec<(&BlockSet, u32)> = inner
            .iter()
            .filter(|(b, _)| {
                !inner
                    .iter()
                    .any(|(o, _)| o.len() > b.len() && b.is_subset(o))
            })
            .copied()
            .collect();
        // A chain already places the latch where the hot path reaches
        // it, with cold arms after; sinking it would undo that.
        if crate::block_chain::chain_loop_range(
            func,
            &mut order,
            lo..lo + body.len(),
            *header,
            body,
            &children,
            &facts,
        ) {
            chained += 1;
        } else if sink_latch(func, &mut order, lo, lo + body.len() - 1, *header, latches) {
            latches_sunk += 1;
        }
    }
    if sunk + latches_sunk + chained == 0 {
        return (0, 0, 0);
    }

    // Rebuild func.blocks in the new order, renumbering ids and
    // terminator targets (BlockId.0 == position invariant).
    let span = order.iter().max().map_or(0, |&m| m as usize + 1);
    let mut remap = vec![u32::MAX; span];
    for (new, &old) in order.iter().enumerate() {
        remap[old as usize] = new as u32;
    }
    let mut by_id: Vec<Option<Block>> = (0..span).map(|_| None).collect();
    for block in func.blocks.drain(..) {
        let id = block.id.0 as usize;
        by_id[id] = Some(block);
    }
    for &old in &order {
        let mut block = by_id[old as usize]
            .take()
            .expect("layout order covers every block");
        block.id = BlockId(remap[block.id.0 as usize]);
        match &mut block.term {
            Terminator::Br(t) => t.0 = remap[t.0 as usize],
            Terminator::CondBr {
                then_blk, else_blk, ..
            } => {
                then_blk.0 = remap[then_blk.0 as usize];
                else_blk.0 = remap[else_blk.0 as usize];
            }
            _ => {}
        }
        func.blocks.push(block);
    }
    (sunk, latches_sunk, chained)
}

/// Move a loop's only latch to the bottom of its (now contiguous)
/// range `[lo..=hi]`. A `for` loop's step block is created right after
/// its header, so it sat between the header and the body: the header
/// branched over it into the body, the body branched back to it, and
/// it branched to the header — three taken branches an iteration where
/// the bottom-latch order has one (generic-id-1m 14.0 ms against
/// 12.6 ms for the same loop written as a `while`). Only a latch that
/// ends in a bare `br header` moves; any other shape is left alone.
fn sink_latch(
    func: &Function,
    order: &mut Vec<u32>,
    lo: usize,
    hi: usize,
    header: u32,
    latches: &[u32],
) -> bool {
    let [latch] = latches else {
        return false;
    };
    if *latch == header || order[hi] == *latch {
        return false;
    }
    if !matches!(func.blocks[*latch as usize].term, Terminator::Br(t) if t.0 == header) {
        return false;
    }
    let Some(pos) = order[lo..=hi].iter().position(|id| id == latch) else {
        return false;
    };
    let id = order.remove(lo + pos);
    order.insert(hi, id);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{BinOp, IPred, Inst, InstKind, Operand, Type, ValueId, ValueInfo};

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
        let (sunk, latches, _) = layout_function(&mut f);
        assert_eq!(sunk, 1);
        assert_eq!(latches, 0, "the latch already sits at the bottom");
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
        assert_eq!(layout_function(&mut f), (0, 0, 0));
        for (i, b) in f.blocks.iter().enumerate() {
            assert_eq!(b.id.0 as usize, i);
        }
    }

    /// The `for` shape: the step block (the latch) is created right
    /// after the header, ahead of the body. It must move to the loop's
    /// bottom so the header falls into the body.
    ///
    ///   b0 → b1(hdr: cond ? b3 : b4) ; b2(step) → b1 ; b3(body) → b2 ; b4: ret
    #[test]
    fn for_loop_step_block_sinks_below_body() {
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
                        then_blk: BlockId(3),
                        else_blk: BlockId(4),
                    },
                ),
                block(
                    2,
                    vec![inst(
                        0,
                        InstKind::BinOp(
                            BinOp::Add,
                            Operand::Value(ValueId(0)),
                            Operand::ConstI64(1),
                        ),
                    )],
                    Terminator::Br(BlockId(1)),
                ),
                block(3, vec![], Terminator::Br(BlockId(2))),
                block(4, vec![], Terminator::Ret(None)),
            ],
            current_origin: None,
        };
        assert_eq!(
            layout_function(&mut f),
            (0, 0, 1),
            "the chain already ends at the step"
        );
        // new order: b0, hdr, body, step, exit
        let Terminator::CondBr {
            then_blk, else_blk, ..
        } = f.blocks[1].term
        else {
            panic!("header keeps its CondBr");
        };
        assert_eq!(then_blk, BlockId(2), "body right after header");
        assert_eq!(else_blk, BlockId(4));
        assert!(
            matches!(f.blocks[2].term, Terminator::Br(BlockId(3))),
            "body falls into step"
        );
        assert!(
            matches!(f.blocks[3].term, Terminator::Br(BlockId(1))),
            "step closes the loop"
        );
        assert!(matches!(f.blocks[4].term, Terminator::Ret(None)));
    }

    fn void_fn(blocks: Vec<Block>) -> Function {
        Function {
            name: "f".into(),
            params: vec![],
            ret: Type::Void,
            values: vec![],
            blocks,
            current_origin: None,
        }
    }

    /// The 507-04 shape: `br` lands on an empty block that only
    /// forwards, so codegen threads past it and emits the block as a
    /// `b` nothing jumps to. Threading the edge lets the sweep drop it.
    ///
    ///   b0 → b1 ; b1(empty) → b2 ; b2: ret   ==>   b0 → b1(ret)
    #[test]
    fn an_empty_forwarder_is_threaded_away() {
        let mut f = void_fn(vec![
            block(0, vec![], Terminator::Br(BlockId(1))),
            block(1, vec![], Terminator::Br(BlockId(2))),
            block(2, vec![], Terminator::Ret(None)),
        ]);
        // b0 is itself an empty forwarder, but it is the entry: no
        // edge lands on it, so it stays and takes the resolved target.
        assert_eq!(thread_empty_forwarders(&mut f), 1);
        assert_eq!(f.blocks.len(), 2);
        assert!(matches!(f.blocks[0].term, Terminator::Br(BlockId(1))));
        assert!(matches!(f.blocks[1].term, Terminator::Ret(None)));
    }

    /// A chain resolves in one walk, and both arms of a CondBr take
    /// the resolution.
    #[test]
    fn a_forwarder_chain_resolves_through() {
        let mut f = void_fn(vec![
            block(
                0,
                vec![inst(
                    0,
                    InstKind::BinOp(BinOp::Add, Operand::ConstI64(1), Operand::ConstI64(2)),
                )],
                Terminator::CondBr {
                    cond: Operand::Value(ValueId(0)),
                    then_blk: BlockId(1),
                    else_blk: BlockId(3),
                },
            ),
            block(1, vec![], Terminator::Br(BlockId(2))),
            block(2, vec![], Terminator::Br(BlockId(4))),
            block(3, vec![], Terminator::Br(BlockId(4))),
            block(4, vec![], Terminator::Ret(None)),
        ]);
        assert_eq!(thread_empty_forwarders(&mut f), 3);
        assert_eq!(f.blocks.len(), 2);
        match f.blocks[0].term {
            Terminator::CondBr {
                then_blk, else_blk, ..
            } => assert_eq!((then_blk.0, else_blk.0), (1, 1)),
            ref other => panic!("expected a cond_br, got {other:?}"),
        }
    }

    /// An empty infinite loop is real control flow — it forwards to
    /// itself, so there is nothing to thread and the block stays.
    #[test]
    fn a_self_forwarding_block_stays() {
        let mut f = void_fn(vec![
            block(0, vec![], Terminator::Br(BlockId(1))),
            block(1, vec![], Terminator::Br(BlockId(1))),
        ]);
        assert_eq!(thread_empty_forwarders(&mut f), 0);
        assert_eq!(f.blocks.len(), 2);
    }

    /// The `a || b` loop test select formation leaves: the compare ends
    /// b1, its branch waits alone in b2. The branch moves up so the
    /// compare is its own block's tail; b4 → b2 has two predecessors
    /// in the second function and stays.
    ///
    ///   b0 → b1 ; b1: %0 = cmp → b2 ; b2: %0 ? b3 : b4 ; b3: ret ; b4 → b1
    #[test]
    fn a_bare_branch_moves_into_its_only_predecessor() {
        let cbr = Terminator::CondBr {
            cond: Operand::Value(ValueId(0)),
            then_blk: BlockId(3),
            else_blk: BlockId(4),
        };
        let cmp = || {
            inst(
                0,
                InstKind::ICmp(IPred::Ult, Operand::ConstI64(1), Operand::ConstI64(2)),
            )
        };
        let mut f = void_fn(vec![
            block(0, vec![], Terminator::Br(BlockId(1))),
            block(1, vec![cmp()], Terminator::Br(BlockId(2))),
            block(2, vec![], cbr.clone()),
            block(3, vec![], Terminator::Ret(None)),
            block(4, vec![], Terminator::Br(BlockId(1))),
        ]);
        assert_eq!(fold_bare_branches(&mut f), 1);
        assert_eq!(f.blocks.len(), 4);
        match f.blocks[1].term {
            Terminator::CondBr {
                then_blk, else_blk, ..
            } => assert_eq!((then_blk.0, else_blk.0), (2, 3)),
            ref other => panic!("expected a cond_br, got {other:?}"),
        }
        let mut g = void_fn(vec![
            block(0, vec![cmp()], Terminator::Br(BlockId(2))),
            block(1, vec![], Terminator::Br(BlockId(2))),
            block(2, vec![], cbr),
            block(3, vec![], Terminator::Ret(None)),
            block(4, vec![], Terminator::Br(BlockId(1))),
        ]);
        assert_eq!(fold_bare_branches(&mut g), 0);
    }

    /// A guard whose fallback arm calls a kernel: the arm the lowering
    /// placed first (the call) must move out of the hot path so the
    /// continuation falls through.
    ///
    ///   b0 → b1(hdr: c ? b2 : b5) ; b2: c ? b3(call) : b4 ;
    ///   b3 → b4 ; b4 → b1 ; b5: ret
    #[test]
    fn cold_call_arm_leaves_the_hot_path() {
        let cond = Operand::Value(ValueId(0));
        let cbr = |t: u32, e: u32| Terminator::CondBr {
            cond,
            then_blk: BlockId(t),
            else_blk: BlockId(e),
        };
        let call = Inst {
            result: None,
            kind: InstKind::Call(torajs_core::ssa::FuncId(0), vec![]),
            origin: None,
        };
        let mut f = Function {
            name: "f".into(),
            params: vec![],
            ret: Type::Void,
            values: vec![
                ValueInfo {
                    ty: Type::Bool,
                    name: None
                };
                1
            ],
            blocks: vec![
                block(0, vec![], Terminator::Br(BlockId(1))),
                block(1, vec![], cbr(2, 5)),
                block(2, vec![], cbr(3, 4)),
                block(3, vec![call], Terminator::Br(BlockId(4))),
                block(4, vec![], Terminator::Br(BlockId(1))),
                block(5, vec![], Terminator::Ret(None)),
            ],
            current_origin: None,
        };
        assert_eq!(layout_function(&mut f), (0, 0, 1));
        // new order: b0, hdr, guard, continuation (latch), call arm, exit
        assert!(matches!(
            f.blocks[2].term,
            Terminator::CondBr {
                then_blk: BlockId(4),
                else_blk: BlockId(3),
                ..
            }
        ));
        assert!(
            matches!(f.blocks[3].term, Terminator::Br(BlockId(1))),
            "guard falls into the latch"
        );
        assert!(
            matches!(f.blocks[4].term, Terminator::Br(BlockId(3))),
            "call arm rejoins"
        );
    }

    /// A bounds check's out-of-bounds read has no call and no exit, so
    /// only the compare says it is rare; the in-bounds read must be
    /// the guard's fall-through.
    #[test]
    fn out_of_bounds_arm_leaves_the_hot_path() {
        let inst = |r: u32, kind: InstKind| Inst {
            result: Some(ValueId(r)),
            kind,
            origin: None,
        };
        let guard = vec![
            inst(1, InstKind::Load(Type::I64, Operand::Value(ValueId(0)), 8)),
            inst(
                2,
                InstKind::ICmp(IPred::Uge, Operand::ConstI64(3), Operand::Value(ValueId(1))),
            ),
        ];
        let undef = vec![inst(3, InstKind::Copy(Type::I64, Operand::ConstI64(0)))];
        let mut f = Function {
            name: "f".into(),
            params: vec![],
            ret: Type::Void,
            values: vec![
                ValueInfo {
                    ty: Type::I64,
                    name: None
                };
                4
            ],
            blocks: vec![
                block(0, vec![], Terminator::Br(BlockId(1))),
                block(
                    1,
                    vec![],
                    Terminator::CondBr {
                        cond: Operand::Value(ValueId(3)),
                        then_blk: BlockId(2),
                        else_blk: BlockId(6),
                    },
                ),
                block(
                    2,
                    guard,
                    Terminator::CondBr {
                        cond: Operand::Value(ValueId(2)),
                        then_blk: BlockId(3),
                        else_blk: BlockId(4),
                    },
                ),
                block(3, undef, Terminator::Br(BlockId(5))),
                block(4, vec![], Terminator::Br(BlockId(5))),
                block(5, vec![], Terminator::Br(BlockId(1))),
                block(6, vec![], Terminator::Ret(None)),
            ],
            current_origin: None,
        };
        layout_function(&mut f);
        // b0, hdr, guard, then the in-bounds arm (no insts)
        assert!(f.blocks[3].insts.is_empty(), "{:?}", f.blocks[3]);
    }
}
