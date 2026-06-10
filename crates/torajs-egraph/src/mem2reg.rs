//! Cross-block single-def-block slot promotion — the dominance fast
//! path of LLVM's `PromoteMemToReg` (`rewriteSingleStoreAlloca`),
//! generalized from "one store" to "all stores in one block". The
//! inliner spills callee params as `alloca + store` in the entry block
//! and the loads sit in branch arms (`fib`'s `%n`: 1 store in bb0,
//! loads in bb1/bb2) — `slot_forward` is block-local and cannot reach
//! them, but no φ is needed either: when every store to a cell lives
//! in one block `B_s` and `B_s` dominates a load's block, every path
//! to the load executes `B_s` (including its last store) first, so the
//! load always observes that store's value.
//!
//! Soundness:
//!
//! * Slots come from `rc_peephole::collect_unescaped_slots` — the
//!   address never leaves load/store address position, so nothing can
//!   alias the cell between the store and the load.
//! * A load forwards only when its block is strictly dominated by the
//!   single store block. Dominance gives execution order: every entry
//!   to the load block passes through all of `B_s`. Loops re-execute
//!   `B_s` before re-reaching the load, and the forwarded SSA value's
//!   latest dynamic instance is exactly the latest store — same
//!   dominance property that makes any cross-block SSA use valid.
//! * Loads in `B_s` itself are left alone: those after the store were
//!   already forwarded by `slot_forward`; those before it observe a
//!   different reaching def (entry undef or a back-edge) and must keep
//!   the slot alive — per-load forwarding stays correct regardless,
//!   and DSE below only fires when zero loads remain.
//! * Cells whose stores span ≥2 blocks (loop counters, multi-ret join
//!   slots) are the φ shape — out of scope here, handled by the full
//!   mem2reg φ-placement follow-up.
//!
//! Runs after `slot_forward` (which clears in-block round-trips and
//! may DSE whole slots first) and before the egraph pass (forwarded
//! constants feed const-fold / GVN downstream).

use std::collections::{HashMap, HashSet};

use torajs_core::ssa::{BlockId, Function, InstKind, Module, Operand, Terminator, ValueId};

use crate::dominator::DominatorTree;
use crate::rc_peephole::collect_unescaped_slots;
use crate::slot_forward::{resolve, rewrite_operands};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Mem2RegStats {
    /// Cross-block loads replaced by the dominating stored value.
    pub loads_forwarded: u32,
    /// Stores deleted because their slot has no loads left.
    pub stores_removed: u32,
    /// Alloca slots deleted outright (no loads, stores gone).
    pub slots_removed: u32,
}

/// Promote single-def-block slot cells across blocks in every function.
pub fn promote_slots(module: &mut Module) -> Mem2RegStats {
    let mut stats = Mem2RegStats::default();
    for func in module.funcs.iter_mut() {
        if func.is_declaration() {
            continue;
        }
        promote_in_function(func, &mut stats);
    }
    stats
}

/// Per-cell facts gathered in one walk: which blocks store to the cell,
/// the last stored operand per block, and where the loads sit.
#[derive(Default)]
struct CellFacts {
    store_blocks: HashSet<BlockId>,
    /// Last store operand in each block (program order within a block).
    last_store: HashMap<BlockId, Operand>,
    /// (block, load result) for every load of this cell.
    loads: Vec<(BlockId, ValueId)>,
}

fn promote_in_function(func: &mut Function, stats: &mut Mem2RegStats) {
    let slots = collect_unescaped_slots(func);
    if slots.is_empty() {
        return;
    }
    // pass 1 — gather per-(slot, off) store/load facts.
    let mut cells: HashMap<(ValueId, u64), CellFacts> = HashMap::new();
    for block in &func.blocks {
        for inst in &block.insts {
            match &inst.kind {
                InstKind::Store(val, Operand::Value(s), off) if slots.contains(s) => {
                    let cell = cells.entry((*s, *off)).or_default();
                    cell.store_blocks.insert(block.id);
                    cell.last_store.insert(block.id, *val);
                }
                InstKind::Load(_, Operand::Value(s), off) if slots.contains(s) => {
                    if let Some(r) = inst.result {
                        cells
                            .entry((*s, *off))
                            .or_default()
                            .loads
                            .push((block.id, r));
                    }
                }
                _ => {}
            }
        }
    }
    // pass 2 — plan: a cell with all stores in one block B_s forwards
    // every load strictly dominated by B_s to the block's last store.
    let domtree = DominatorTree::compute(func);
    let mut replace: HashMap<ValueId, Operand> = HashMap::new();
    for facts in cells.values() {
        if facts.store_blocks.len() != 1 {
            continue; // zero stores (undef read) or φ shape — leave alone
        }
        let store_blk = *facts.store_blocks.iter().next().unwrap();
        let stored = facts.last_store[&store_blk];
        for &(load_blk, r) in &facts.loads {
            if load_blk != store_blk && domtree.dominates(store_blk, load_blk) {
                replace.insert(r, stored);
            }
        }
    }
    if replace.is_empty() {
        return;
    }
    // pass 3 — apply: rewrite operands/terminators through the map
    // (resolve chases load-of-load chains; acyclic because each link's
    // store strictly precedes its load in execution order), then drop
    // the forwarded loads.
    for block in func.blocks.iter_mut() {
        for inst in block.insts.iter_mut() {
            rewrite_operands(&mut inst.kind, &replace);
        }
        match &mut block.term {
            Terminator::Ret(Some(op)) => *op = resolve(op, &replace),
            Terminator::CondBr { cond, .. } => *cond = resolve(cond, &replace),
            _ => {}
        }
    }
    for block in func.blocks.iter_mut() {
        block
            .insts
            .retain(|inst| !matches!(inst.result, Some(r) if replace.contains_key(&r)));
    }
    stats.loads_forwarded += replace.len() as u32;
    // pass 4 — DSE: slots with zero remaining loads (any offset) get
    // their stores and the alloca itself removed (same contract as
    // slot_forward: stores into unescaped slots are pure bit-moves).
    let mut loaded: HashSet<ValueId> = HashSet::new();
    for block in &func.blocks {
        for inst in &block.insts {
            if let InstKind::Load(_, Operand::Value(s), _) = &inst.kind {
                loaded.insert(*s);
            }
        }
    }
    let dead_slots: HashSet<ValueId> = slots.difference(&loaded).copied().collect();
    if dead_slots.is_empty() {
        return;
    }
    for block in func.blocks.iter_mut() {
        block.insts.retain(|inst| match &inst.kind {
            InstKind::Store(_, Operand::Value(s), _) if dead_slots.contains(s) => {
                stats.stores_removed += 1;
                false
            }
            InstKind::Alloca(_) | InstKind::AllocaBytes(_)
                if matches!(inst.result, Some(r) if dead_slots.contains(&r)) =>
            {
                stats.slots_removed += 1;
                false
            }
            _ => true,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{Block, BlockId, FuncId, Inst, Terminator, Type, ValueInfo};

    fn val(result: u32, kind: InstKind) -> Inst {
        Inst {
            result: Some(ValueId(result)),
            kind,
            origin: None,
        }
    }

    fn void(kind: InstKind) -> Inst {
        Inst {
            result: None,
            kind,
            origin: None,
        }
    }

    fn store(v: Operand, slot: u32) -> Inst {
        void(InstKind::Store(v, Operand::Value(ValueId(slot)), 0))
    }

    fn load(result: u32, slot: u32) -> Inst {
        val(
            result,
            InstKind::Load(Type::I64, Operand::Value(ValueId(slot)), 0),
        )
    }

    fn func_of(n_values: usize, blocks: Vec<Block>) -> Module {
        Module {
            funcs: vec![Function {
                name: "main".into(),
                params: vec![],
                ret: Type::Void,
                blocks,
                values: vec![
                    ValueInfo {
                        ty: Type::I64,
                        name: None
                    };
                    n_values
                ],
                current_origin: None,
            }],
            ..Default::default()
        }
    }

    fn block(id: u32, insts: Vec<Inst>, term: Terminator) -> Block {
        Block {
            id: BlockId(id),
            insts,
            term,
        }
    }

    fn cond_br(cond: Operand, t: u32, e: u32) -> Terminator {
        Terminator::CondBr {
            cond,
            then_blk: BlockId(t),
            else_blk: BlockId(e),
        }
    }

    #[test]
    fn param_spill_diamond_promotes_fully() {
        // the fib shape: bb0 alloca+store, loads in both branch arms.
        let mut m = func_of(
            4,
            vec![
                block(
                    0,
                    vec![
                        val(0, InstKind::Alloca(Type::I64)),
                        store(Operand::ConstI64(40), 0),
                    ],
                    cond_br(Operand::ConstBool(true), 1, 2),
                ),
                block(
                    1,
                    vec![load(1, 0)],
                    Terminator::Ret(Some(Operand::Value(ValueId(1)))),
                ),
                block(
                    2,
                    vec![load(2, 0), load(3, 0)],
                    Terminator::Ret(Some(Operand::Value(ValueId(3)))),
                ),
            ],
        );
        let stats = promote_slots(&mut m);
        assert_eq!(stats.loads_forwarded, 3);
        assert_eq!(stats.stores_removed, 1);
        assert_eq!(stats.slots_removed, 1);
        let f = &m.funcs[0];
        assert!(f.blocks[0].insts.is_empty());
        assert!(f.blocks[1].insts.is_empty());
        assert!(f.blocks[2].insts.is_empty());
        assert!(matches!(
            f.blocks[1].term,
            Terminator::Ret(Some(Operand::ConstI64(40)))
        ));
        assert!(matches!(
            f.blocks[2].term,
            Terminator::Ret(Some(Operand::ConstI64(40)))
        ));
    }

    #[test]
    fn same_block_multi_store_forwards_last() {
        // the %xs shape: several stores all in bb0, load downstream.
        let mut m = func_of(
            2,
            vec![
                block(
                    0,
                    vec![
                        val(0, InstKind::Alloca(Type::I64)),
                        store(Operand::ConstI64(1), 0),
                        store(Operand::ConstI64(2), 0),
                    ],
                    Terminator::Br(BlockId(1)),
                ),
                block(
                    1,
                    vec![load(1, 0)],
                    Terminator::Ret(Some(Operand::Value(ValueId(1)))),
                ),
            ],
        );
        let stats = promote_slots(&mut m);
        assert_eq!(stats.loads_forwarded, 1);
        assert!(matches!(
            m.funcs[0].blocks[1].term,
            Terminator::Ret(Some(Operand::ConstI64(2)))
        ));
    }

    #[test]
    fn multi_block_stores_left_for_phi() {
        // the loop-counter shape: init store in bb0, back-edge store
        // in bb2 — two store blocks, must stay untouched.
        let mut m = func_of(
            3,
            vec![
                block(
                    0,
                    vec![
                        val(0, InstKind::Alloca(Type::I64)),
                        store(Operand::ConstI64(0), 0),
                    ],
                    Terminator::Br(BlockId(1)),
                ),
                block(
                    1,
                    vec![load(1, 0)],
                    cond_br(Operand::Value(ValueId(1)), 2, 3),
                ),
                block(
                    2,
                    vec![store(Operand::ConstI64(9), 0).clone()],
                    Terminator::Br(BlockId(1)),
                ),
                block(3, vec![], Terminator::Ret(None)),
            ],
        );
        let stats = promote_slots(&mut m);
        assert_eq!(stats, Mem2RegStats::default());
        assert_eq!(m.funcs[0].blocks[1].insts.len(), 1);
    }

    #[test]
    fn non_dominating_store_not_forwarded() {
        // diamond with the store only on one arm: join load must not
        // forward (the other arm reaches it without storing).
        let mut m = func_of(
            2,
            vec![
                block(
                    0,
                    vec![val(0, InstKind::Alloca(Type::I64))],
                    cond_br(Operand::ConstBool(true), 1, 2),
                ),
                block(
                    1,
                    vec![store(Operand::ConstI64(5), 0)],
                    Terminator::Br(BlockId(3)),
                ),
                block(2, vec![], Terminator::Br(BlockId(3))),
                block(
                    3,
                    vec![load(1, 0)],
                    Terminator::Ret(Some(Operand::Value(ValueId(1)))),
                ),
            ],
        );
        let stats = promote_slots(&mut m);
        assert_eq!(stats, Mem2RegStats::default());
        assert_eq!(m.funcs[0].blocks[3].insts.len(), 1);
    }

    #[test]
    fn load_before_store_keeps_slot_alive() {
        // loop header loads, then stores, back-edge re-enters: the
        // in-block load-before-store reads the previous iteration's
        // value and blocks DSE; the cross-block exit load (dominated
        // by the storing block) still forwards.
        let mut m = func_of(
            3,
            vec![
                block(
                    0,
                    vec![val(0, InstKind::Alloca(Type::I64))],
                    Terminator::Br(BlockId(1)),
                ),
                block(
                    1,
                    vec![load(1, 0), store(Operand::ConstI64(7), 0)],
                    cond_br(Operand::Value(ValueId(1)), 1, 2),
                ),
                block(
                    2,
                    vec![load(2, 0)],
                    Terminator::Ret(Some(Operand::Value(ValueId(2)))),
                ),
            ],
        );
        let stats = promote_slots(&mut m);
        assert_eq!(stats.loads_forwarded, 1);
        assert_eq!(stats.stores_removed, 0);
        assert_eq!(stats.slots_removed, 0);
        // bb1's pre-store load survives; bb2's load is gone.
        assert_eq!(m.funcs[0].blocks[1].insts.len(), 2);
        assert!(m.funcs[0].blocks[2].insts.is_empty());
        assert!(matches!(
            m.funcs[0].blocks[2].term,
            Terminator::Ret(Some(Operand::ConstI64(7)))
        ));
    }

    #[test]
    fn load_of_load_chain_resolves_across_cells() {
        // slot B's store value is a load from slot A that itself
        // forwards — the chain resolves to A's stored constant.
        let mut m = func_of(
            4,
            vec![
                block(
                    0,
                    vec![
                        val(0, InstKind::Alloca(Type::I64)),
                        store(Operand::ConstI64(9), 0),
                    ],
                    Terminator::Br(BlockId(1)),
                ),
                block(
                    1,
                    vec![
                        load(1, 0),
                        val(2, InstKind::Alloca(Type::I64)),
                        store(Operand::Value(ValueId(1)), 2),
                    ],
                    Terminator::Br(BlockId(2)),
                ),
                block(
                    2,
                    vec![load(3, 2)],
                    Terminator::Ret(Some(Operand::Value(ValueId(3)))),
                ),
            ],
        );
        let stats = promote_slots(&mut m);
        assert_eq!(stats.loads_forwarded, 2);
        assert_eq!(stats.slots_removed, 2);
        assert!(matches!(
            m.funcs[0].blocks[2].term,
            Terminator::Ret(Some(Operand::ConstI64(9)))
        ));
    }

    #[test]
    fn escaped_slot_untouched() {
        // slot pointer passed to a call — escape analysis must reject.
        let mut m = func_of(
            2,
            vec![
                block(
                    0,
                    vec![
                        val(0, InstKind::Alloca(Type::I64)),
                        store(Operand::ConstI64(3), 0),
                        void(InstKind::Call(FuncId(0), vec![Operand::Value(ValueId(0))])),
                    ],
                    Terminator::Br(BlockId(1)),
                ),
                block(
                    1,
                    vec![load(1, 0)],
                    Terminator::Ret(Some(Operand::Value(ValueId(1)))),
                ),
            ],
        );
        let stats = promote_slots(&mut m);
        assert_eq!(stats, Mem2RegStats::default());
        assert_eq!(m.funcs[0].blocks[1].insts.len(), 1);
    }

    #[test]
    fn offsets_are_independent_cells_and_gate_dse() {
        // off 0 forwards; off 8 has a φ-shaped store pair and keeps
        // its load — so the shared alloca must survive DSE.
        let mut m = func_of(
            4,
            vec![
                block(
                    0,
                    vec![
                        val(0, InstKind::AllocaBytes(16)),
                        void(InstKind::Store(
                            Operand::ConstI64(1),
                            Operand::Value(ValueId(0)),
                            0,
                        )),
                        void(InstKind::Store(
                            Operand::ConstI64(2),
                            Operand::Value(ValueId(0)),
                            8,
                        )),
                    ],
                    Terminator::Br(BlockId(1)),
                ),
                block(
                    1,
                    vec![
                        val(1, InstKind::Load(Type::I64, Operand::Value(ValueId(0)), 0)),
                        val(2, InstKind::Load(Type::I64, Operand::Value(ValueId(0)), 8)),
                        void(InstKind::Store(
                            Operand::ConstI64(3),
                            Operand::Value(ValueId(0)),
                            8,
                        )),
                    ],
                    cond_br(Operand::Value(ValueId(1)), 1, 2),
                ),
                block(
                    2,
                    vec![val(
                        3,
                        InstKind::Load(Type::I64, Operand::Value(ValueId(0)), 8),
                    )],
                    Terminator::Ret(Some(Operand::Value(ValueId(3)))),
                ),
            ],
        );
        let stats = promote_slots(&mut m);
        // off-0 load in bb1 forwards (single store block bb0 dominates
        // bb1); off-8 cell has stores in bb0 AND bb1 → φ shape, both
        // its loads stay.
        assert_eq!(stats.loads_forwarded, 1);
        assert_eq!(stats.stores_removed, 0);
        assert_eq!(stats.slots_removed, 0);
        assert!(matches!(
            m.funcs[0].blocks[1].term,
            Terminator::CondBr {
                cond: Operand::ConstI64(1),
                ..
            }
        ));
    }
}
