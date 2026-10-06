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
//! A slot holding several candidates — the inliner's return slot when
//! the callee returns a fresh cell from more than one `return` (a
//! generator's step object) — carries them as one group, the merge
//! C2's `ReduceAllocationMerges` takes for a φ of N allocations: the
//! group is promoted or kept whole, so the release through the slot
//! stays paired with whichever member reached it.
//!
//! Two things break the equivalence and reject the group: a slot
//! holding anything else non-null, and a stale alias — a slot still holding the previous iteration's cell when the
//! allocation runs again and read before being overwritten. The heap
//! version reads the old cell there; the frame version would read the
//! new one, since the frame slot is reused. SSA values cannot carry an
//! alias across a back edge at this stage (no φ yet), so slots are the
//! only way, and the live-in check below is complete.

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{BlockId, Function, InstKind, Operand, ValueId};

use crate::phi_promote::{cfg_preds, compute_live_in};
use crate::rc_peephole::collect_unescaped_slots;

use super::is_null;

/// Everything is keyed by a group's representative candidate.
pub(super) struct Owners {
    /// Value → the group it names (a candidate names its own group).
    pub(super) owner: HashMap<ValueId, ValueId>,
    /// Slot → the group it carries.
    pub(super) slot_owner: HashMap<ValueId, ValueId>,
    /// Groups a slot rule rejected.
    pub(super) bad: HashSet<ValueId>,
    /// Group → its candidates.
    pub(super) members: HashMap<ValueId, Vec<ValueId>>,
}

/// Union-find over candidates.
struct Groups(HashMap<ValueId, ValueId>);

impl Groups {
    fn find(&self, mut c: ValueId) -> ValueId {
        while let Some(&p) = self.0.get(&c)
            && p != c
        {
            c = p;
        }
        c
    }

    fn union(&mut self, a: ValueId, b: ValueId) {
        let (ra, rb) = (self.find(a), self.find(b));
        // the smaller id leads, so the representative is deterministic
        let (lo, hi) = if ra.0 <= rb.0 { (ra, rb) } else { (rb, ra) };
        self.0.insert(hi, lo);
    }
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
        slot_owner: HashMap::default(),
        bad: HashSet::default(),
        members: HashMap::default(),
    };
    let mut groups = Groups(cands.keys().map(|c| (*c, *c)).collect());
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
                        Some(&d) if groups.find(d) != groups.find(c) => {
                            groups.union(c, d);
                            changed = true;
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
    // from here on every key is a group
    for c in o.owner.values_mut().chain(o.slot_owner.values_mut()) {
        *c = groups.find(*c);
    }
    for c in cands.keys() {
        o.members.entry(groups.find(*c)).or_default().push(*c);
    }
    // a carrying slot holds only its group, its aliases or null, and
    // is only ever accessed as a whole
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
    let cands: HashSet<ValueId> = o.members.values().flatten().copied().collect();
    let mut alloc_blocks: HashMap<ValueId, Vec<BlockId>> = HashMap::default();
    for block in &func.blocks {
        for inst in &block.insts {
            if let Some(r) = inst.result
                && cands.contains(&r)
            {
                alloc_blocks.entry(o.owner[&r]).or_default().push(block.id);
            }
        }
    }
    let preds = cfg_preds(func);
    let mut bad = HashSet::default();
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
        if alloc_blocks
            .get(&c)
            .is_some_and(|bs| bs.iter().any(|a| live.contains(a)))
        {
            bad.insert(c);
        }
    }
    bad
}
