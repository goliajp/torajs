//! Aliases of a heap-to-stack candidate through unescaped stack slots.
//!
//! Before `MEM2REG_PHI` a cell that crosses a join is carried by an
//! `Alloca` slot rather than a φ: the inliner's return slot takes the
//! cell on the normal `Ret` and a null on the propagate `Ret`, and a
//! `let` slot takes it in the body and null at scope exit. LLVM's
//! `AAHeapToStack` follows a φ / select to its uses; C2's
//! `ReduceAllocationMerges` accepts a φ of one allocation and null.
//! This is the same Follow over the slot form: a slot whose every
//! store is the candidate, an alias of it, or null carries the
//! candidate, and every load from it is an alias (possibly null).
//!
//! Two things break the equivalence and reject the candidate: a slot
//! holding two candidates (or anything else non-null), and a stale
//! alias — a slot still holding the previous iteration's cell when the
//! allocation runs again and read before being overwritten. The heap
//! version reads the old cell there; the frame version would read the
//! new one, since the frame slot is reused. SSA values cannot carry an
//! alias across a back edge at this stage (no φ yet), so slots are the
//! only way, and the live-in check below is complete.

use std::collections::{HashMap, HashSet};

use torajs_core::ssa::{BlockId, Function, InstKind, Operand, ValueId};

use crate::phi_promote::{cfg_preds, compute_live_in};
use crate::rc_peephole::collect_unescaped_slots;

use super::is_null;

pub(super) struct Owners {
    /// Value → the candidate it names (a candidate names itself).
    pub(super) owner: HashMap<ValueId, ValueId>,
    /// Slot → the candidate it carries.
    pub(super) slot_owner: HashMap<ValueId, ValueId>,
    /// Candidates a slot rule rejected.
    pub(super) bad: HashSet<ValueId>,
}

pub(super) fn follow(func: &Function, cands: &HashMap<ValueId, i64>) -> Owners {
    let alloca_slots: HashSet<ValueId> = func
        .blocks
        .iter()
        .flat_map(|b| &b.insts)
        .filter(|i| matches!(i.kind, InstKind::Alloca(_)))
        .filter_map(|i| i.result)
        .collect();
    let slots: HashSet<ValueId> = collect_unescaped_slots(func)
        .intersection(&alloca_slots)
        .copied()
        .collect();
    let mut o = Owners {
        owner: cands.keys().map(|c| (*c, *c)).collect(),
        slot_owner: HashMap::new(),
        bad: HashSet::new(),
    };
    let mut changed = true;
    while changed {
        changed = false;
        for inst in func.blocks.iter().flat_map(|b| &b.insts) {
            match (&inst.kind, inst.result) {
                (InstKind::Store(Operand::Value(v), Operand::Value(s), 0), _)
                    if slots.contains(s) =>
                {
                    let Some(&c) = o.owner.get(v) else { continue };
                    match o.slot_owner.get(s) {
                        None => {
                            o.slot_owner.insert(*s, c);
                            changed = true;
                        }
                        Some(&d) if d != c => {
                            o.bad.extend([c, d]);
                        }
                        Some(_) => {}
                    }
                }
                (InstKind::Load(_, Operand::Value(s), 0), Some(r)) => {
                    if let Some(&c) = o.slot_owner.get(s)
                        && o.owner.insert(r, c).is_none()
                    {
                        changed = true;
                    }
                }
                (InstKind::Identity(Operand::Value(x)), Some(r)) => {
                    if let Some(&c) = o.owner.get(x)
                        && o.owner.insert(r, c).is_none()
                    {
                        changed = true;
                    }
                }
                _ => {}
            }
        }
    }
    // a carrying slot holds only its candidate, its aliases or null,
    // and is only ever accessed as a whole
    for inst in func.blocks.iter().flat_map(|b| &b.insts) {
        match &inst.kind {
            InstKind::Store(val, Operand::Value(s), off) => {
                if let Some(&c) = o.slot_owner.get(s) {
                    let named = matches!(val, Operand::Value(v) if o.owner.get(v) == Some(&c));
                    if *off != 0 || !(named || is_null(val)) {
                        o.bad.insert(c);
                    }
                }
            }
            InstKind::Load(_, Operand::Value(s), off) if *off != 0 => {
                o.bad.extend(o.slot_owner.get(s).copied());
            }
            _ => {}
        }
    }
    o.bad.extend(stale_aliases(func, &o));
    o
}

/// Candidates whose allocation block is live-in for a slot carrying
/// them: the previous iteration's alias is read after the frame slot
/// has been handed out again (`let last = null; for (…) last = new C()`).
fn stale_aliases(func: &Function, o: &Owners) -> HashSet<ValueId> {
    let mut alloc_block: HashMap<ValueId, BlockId> = HashMap::new();
    for block in &func.blocks {
        for inst in &block.insts {
            if let Some(r) = inst.result
                && o.owner.get(&r) == Some(&r)
            {
                alloc_block.insert(r, block.id);
            }
        }
    }
    let preds = cfg_preds(func);
    let mut bad = HashSet::new();
    for (&s, &c) in &o.slot_owner {
        let store_blocks: HashSet<BlockId> = func
            .blocks
            .iter()
            .filter(|b| {
                b.insts
                    .iter()
                    .any(|i| matches!(&i.kind, InstKind::Store(_, Operand::Value(p), 0) if *p == s))
            })
            .map(|b| b.id)
            .collect();
        let live = compute_live_in(func, &preds, (s, 0), &store_blocks);
        if alloc_block.get(&c).is_some_and(|a| live.contains(a)) {
            bad.insert(c);
        }
    }
    bad
}
