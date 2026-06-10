//! Plan application + φ destruction — operand rewrite, load/store
//! removal, slot DSE, predecessor-end Copy emission with critical-edge
//! splitting and parallel-copy sequentialization. See the parent
//! module doc for the full pipeline.

use std::collections::{HashMap, HashSet};

use torajs_core::ssa::{
    Block, BlockId, Function, Inst, InstKind, Operand, Terminator, Type, ValueId, ValueInfo,
};

use super::{CellPlan, PhiPromoteStats, term_succs};
use crate::slot_forward::{resolve, rewrite_operands};

/// Apply every successful plan at once: rewrite loads through the
/// merged replace map, delete forwarded loads and promoted cells'
/// stores, DSE empty slots, then destruct live φs into predecessor
/// Copys (splitting multi-successor predecessor edges).
pub(super) fn apply_plans(
    func: &mut Function,
    plans: Vec<CellPlan>,
    slots: &HashSet<ValueId>,
    stats: &mut PhiPromoteStats,
) {
    let mut replace: HashMap<ValueId, Operand> = HashMap::new();
    let mut promoted_cells: HashSet<(ValueId, u64)> = HashSet::new();
    for plan in &plans {
        replace.extend(plan.replace.iter().map(|(k, v)| (*k, *v)));
        promoted_cells.insert((plan.slot, plan.off));
    }
    stats.cells_promoted += plans.len() as u32;
    // rewrite all operand/terminator uses through the replace chains.
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
    // drop forwarded loads and promoted cells' stores.
    let mut loads_dropped = 0u32;
    let mut stores_dropped = 0u32;
    for block in func.blocks.iter_mut() {
        block.insts.retain(|inst| match &inst.kind {
            InstKind::Load(_, _, _) if matches!(inst.result, Some(r) if replace.contains_key(&r)) =>
            {
                loads_dropped += 1;
                false
            }
            InstKind::Store(_, Operand::Value(s), o) if promoted_cells.contains(&(*s, *o)) => {
                stores_dropped += 1;
                false
            }
            _ => true,
        });
    }
    stats.loads_forwarded += loads_dropped;
    stats.stores_removed += stores_dropped;
    // DSE: slots with zero loads anywhere lose remaining stores + alloca.
    let mut loaded: HashSet<ValueId> = HashSet::new();
    for block in &func.blocks {
        for inst in &block.insts {
            if let InstKind::Load(_, Operand::Value(s), _) = &inst.kind {
                loaded.insert(*s);
            }
        }
    }
    let dead_slots: HashSet<ValueId> = slots.difference(&loaded).copied().collect();
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
    destruct_phis(func, &plans, &replace, stats);
}

/// Lower every live φ into predecessor-end `Copy`s. Edge copies are
/// gathered per (pred, φ-block) so several cells' φs on the same edge
/// sequentialize together as one parallel-copy group.
fn destruct_phis(
    func: &mut Function,
    plans: &[CellPlan],
    replace: &HashMap<ValueId, Operand>,
    stats: &mut PhiPromoteStats,
) {
    let mut edge_copies: HashMap<(BlockId, BlockId), Vec<(ValueId, Operand, Type)>> =
        HashMap::new();
    for plan in plans {
        for phi in &plan.phis {
            if !phi.live {
                continue;
            }
            for &(pred, ref op) in &phi.incoming {
                let src = resolve(op, replace);
                edge_copies
                    .entry((pred, phi.block))
                    .or_default()
                    .push((phi.vid, src, plan.ty));
            }
        }
    }
    if edge_copies.is_empty() {
        return;
    }
    let mut edges: Vec<(BlockId, BlockId)> = edge_copies.keys().copied().collect();
    edges.sort_by_key(|(p, s)| (p.0, s.0));
    for edge in edges {
        let copies = sequentialize_copies(func, &edge_copies[&edge]);
        stats.copies_inserted += copies.len() as u32;
        let (pred, succ) = edge;
        let pred_multi_succ = term_succs(&func.blocks[pred.0 as usize].term).len() > 1;
        if pred_multi_succ {
            // critical edge — split with a fresh forwarding block.
            let new_id = BlockId(func.blocks.len() as u32);
            func.blocks.push(Block {
                id: new_id,
                insts: copies,
                term: Terminator::Br(succ),
            });
            if let Terminator::CondBr {
                then_blk, else_blk, ..
            } = &mut func.blocks[pred.0 as usize].term
            {
                if *then_blk == succ {
                    *then_blk = new_id;
                }
                if *else_blk == succ {
                    *else_blk = new_id;
                }
            }
            stats.edges_split += 1;
        } else {
            func.blocks[pred.0 as usize].insts.extend(copies);
        }
    }
}

/// Sequentialize one edge's parallel copies. Ready-first: emit any
/// copy whose dst no other pending copy still reads. Cycles (the swap
/// problem) break by saving one dst into a fresh temp and redirecting
/// its readers there.
fn sequentialize_copies(func: &mut Function, group: &[(ValueId, Operand, Type)]) -> Vec<Inst> {
    let mut pending: Vec<(ValueId, Operand, Type)> = group.to_vec();
    let mut out: Vec<Inst> = Vec::new();
    let copy_inst = |dst: ValueId, ty: Type, src: Operand| Inst {
        result: Some(dst),
        kind: InstKind::Copy(ty, src),
        origin: None,
    };
    while !pending.is_empty() {
        let ready = pending.iter().position(|(dst, _, _)| {
            !pending
                .iter()
                .any(|(_, src, _)| *src == Operand::Value(*dst))
        });
        match ready {
            Some(i) => {
                let (dst, src, ty) = pending.remove(i);
                if src != Operand::Value(dst) {
                    out.push(copy_inst(dst, ty, src));
                }
            }
            None => {
                // pure cycle — save pending[0]'s dst into a temp and
                // retarget its readers.
                let (dst, _, ty) = pending[0];
                let temp = ValueId(func.values.len() as u32);
                func.values.push(ValueInfo { ty, name: None });
                out.push(copy_inst(temp, ty, Operand::Value(dst)));
                for (_, src, _) in pending.iter_mut() {
                    if *src == Operand::Value(dst) {
                        *src = Operand::Value(temp);
                    }
                }
            }
        }
    }
    out
}
