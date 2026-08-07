//! Cell planning — pruned-SSA liveness, IDF φ placement, domtree
//! rename, trivial-φ elimination, and live-φ marking. See the parent
//! module doc for the full pipeline.

use std::collections::{HashMap, HashSet};

use torajs_core::ssa::{BlockId, Function, InstKind, Operand, Type, ValueId, ValueInfo};

use super::{CellPlan, PhiNode, term_succs};
use crate::dominator::DominatorTree;
use crate::slot_forward::resolve;

/// Pruned-SSA liveness — LLVM `ComputeLiveInBlocks`: blocks where the
/// cell's value is live at entry (an upward-exposed load reaches
/// before any same-block store, propagated backwards through
/// store-free predecessors). φs only matter in live-in blocks; an IDF
/// join where the cell is dead would demand a reaching def nobody
/// reads — and abort whole cells on harmless uninitialized paths
/// (e.g. an outer-loop header above the slot's first store).
pub(super) fn compute_live_in(
    func: &Function,
    preds: &[Vec<BlockId>],
    (slot, off): (ValueId, u64),
    store_blocks: &HashSet<BlockId>,
) -> HashSet<BlockId> {
    let mut live: HashSet<BlockId> = HashSet::new();
    let mut work: Vec<BlockId> = Vec::new();
    for block in &func.blocks {
        for inst in &block.insts {
            match &inst.kind {
                InstKind::Store(_, Operand::Value(s), o) if *s == slot && *o == off => break,
                InstKind::Load(_, Operand::Value(s), o) if *s == slot && *o == off => {
                    live.insert(block.id);
                    work.push(block.id);
                    break;
                }
                _ => {}
            }
        }
    }
    while let Some(b) = work.pop() {
        for &p in &preds[b.0 as usize] {
            if !store_blocks.contains(&p) && live.insert(p) {
                work.push(p);
            }
        }
    }
    live
}

/// IDF placement + domtree rename for one cell. `None` = abort (a
/// load or φ incoming had no reaching def — uninitialized-read path).
pub(super) fn plan_cell(
    func: &mut Function,
    domtree: &DominatorTree,
    preds: &[Vec<BlockId>],
    df: &[HashSet<BlockId>],
    (slot, off): (ValueId, u64),
    ty: Type,
    store_blocks: &HashSet<BlockId>,
) -> Option<CellPlan> {
    // iterated dominance frontier over def blocks, pruned to live-in
    // joins (a non-live join inserts no φ, hence is no def and stops
    // the frontier walk there — pruned SSA form).
    let live_in = compute_live_in(func, preds, (slot, off), store_blocks);
    let mut phi_blocks: HashSet<BlockId> = HashSet::new();
    let mut work: Vec<BlockId> = store_blocks.iter().copied().collect();
    while let Some(b) = work.pop() {
        if !domtree.is_reachable(b) {
            continue;
        }
        for &j in &df[b.0 as usize] {
            if live_in.contains(&j) && phi_blocks.insert(j) {
                work.push(j);
            }
        }
    }
    // allocate φ vids up front (rename needs them as reaching defs).
    // In block order, not `phi_blocks` order: this loop mints a
    // ValueId per φ, so iterating the HashSet directly would hand the
    // same φs different ids from run to run, and the register
    // allocator breaks ties on ValueId — that reached the artifact as
    // two byte-different binaries for one input. The set itself is a
    // fixpoint (iterated dominance frontier), so its contents do not
    // depend on the walk order above; only this numbering did.
    let mut phi_order: Vec<BlockId> = phi_blocks.iter().copied().collect();
    phi_order.sort_by_key(|b| b.0);
    let mut phis: Vec<PhiNode> = Vec::new();
    let mut phi_at: HashMap<BlockId, usize> = HashMap::new();
    for &b in &phi_order {
        let vid = ValueId(func.values.len() as u32);
        func.values.push(ValueInfo { ty, name: None });
        phi_at.insert(b, phis.len());
        phis.push(PhiNode {
            vid,
            block: b,
            incoming: Vec::new(),
            live: false,
        });
    }
    let mut replace: HashMap<ValueId, Operand> = HashMap::new();
    // domtree preorder DFS threading the reaching def. Explicit stack
    // of (block, reaching-at-entry).
    let mut stack: Vec<(BlockId, Option<Operand>)> = vec![(BlockId(0), None)];
    while let Some((bid, mut reaching)) = stack.pop() {
        if let Some(&pi) = phi_at.get(&bid) {
            reaching = Some(Operand::Value(phis[pi].vid));
        }
        for inst in &func.blocks[bid.0 as usize].insts {
            match &inst.kind {
                InstKind::Load(_, Operand::Value(s), o) if *s == slot && *o == off => {
                    let r = inst.result?;
                    match reaching {
                        Some(v) => {
                            replace.insert(r, v);
                        }
                        None => return None, // uninitialized read — abort
                    }
                }
                InstKind::Store(val, Operand::Value(s), o) if *s == slot && *o == off => {
                    // chase through loads of THIS cell already renamed
                    // (load→store round-trips inside the walk).
                    reaching = Some(resolve(val, &replace));
                }
                _ => {}
            }
        }
        // feed CFG successors' φs with this block's exit def.
        for succ in term_succs(&func.blocks[bid.0 as usize].term) {
            if let Some(&pi) = phi_at.get(&succ) {
                match reaching {
                    Some(v) => phis[pi].incoming.push((bid, v)),
                    None => return None, // φ incoming undef — abort
                }
            }
        }
        for &child in domtree.children(bid) {
            stack.push((child, reaching));
        }
    }
    // a φ in an unreachable-from-entry block got no incomings; those
    // blocks' loads were never renamed either — drop such φs.
    phis.retain(|p| !p.incoming.is_empty() || domtree.is_reachable(p.block));
    Some(CellPlan {
        slot,
        off,
        ty,
        phis,
        replace,
    })
}

/// Braun-style trivial-φ removal to a fixpoint: a φ whose incomings
/// (resolved through prior eliminations) are all the same operand —
/// or the φ itself — is replaced by that operand. Returns the number
/// removed; removed φs get `replace[vid] = operand` entries that
/// `resolve` chases everywhere else.
pub(super) fn trivial_phi_elim(plan: &mut CellPlan) -> u32 {
    let mut removed = 0;
    loop {
        let mut changed = false;
        for i in 0..plan.phis.len() {
            let vid = plan.phis[i].vid;
            if plan.replace.contains_key(&vid) {
                continue; // already eliminated
            }
            let mut unique: Option<Operand> = None;
            let mut trivial = true;
            for (_, op) in &plan.phis[i].incoming {
                let r = resolve(op, &plan.replace);
                if r == Operand::Value(vid) {
                    continue; // self-reference doesn't disqualify
                }
                match unique {
                    None => unique = Some(r),
                    Some(u) if u == r => {}
                    Some(_) => {
                        trivial = false;
                        break;
                    }
                }
            }
            if trivial {
                if let Some(u) = unique {
                    plan.replace.insert(vid, u);
                    removed += 1;
                    changed = true;
                }
            }
        }
        if !changed {
            return removed;
        }
    }
}

/// Mark φs reachable from actual load uses (directly or through other
/// φs' incomings). Unmarked φs were IDF over-placement — they emit no
/// copies.
pub(super) fn mark_live_phis(plan: &mut CellPlan) {
    let by_vid: HashMap<ValueId, usize> = plan
        .phis
        .iter()
        .enumerate()
        .map(|(i, p)| (p.vid, i))
        .collect();
    let mut work: Vec<ValueId> = Vec::new();
    for op in plan.replace.values() {
        let r = resolve(op, &plan.replace);
        if let Operand::Value(v) = r {
            if by_vid.contains_key(&v) {
                work.push(v);
            }
        }
    }
    while let Some(v) = work.pop() {
        let i = by_vid[&v];
        if plan.phis[i].live {
            continue;
        }
        plan.phis[i].live = true;
        for (_, op) in plan.phis[i].incoming.clone() {
            let r = resolve(&op, &plan.replace);
            if let Operand::Value(u) = r {
                if by_vid.contains_key(&u) && !plan.phis[by_vid[&u]].live {
                    work.push(u);
                }
            }
        }
    }
}
