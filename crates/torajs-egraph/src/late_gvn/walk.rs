//! The dominator walk's state: the two tables, their scope frames and
//! the keys entries are filed under.

use std::collections::{HashMap, HashSet};

use torajs_core::ssa::{
    Function, InstKind, Operand, Type, ValueId, rewrite_value_operands, visit_value_operands,
};

type Key = (Type, InstKind, Vec<u32>);

/// One dominator-walk scope frame. `barrier` marks a block whose
/// predecessor set is not exactly its immediate dominator — risky
/// entries above it are invisible below (see module doc).
/// Scope-exit removal is by exact key: an inner insert that shadows
/// an outer entry drops the outer one at inner exit — a conservative
/// loss (later lookups miss and recompute), never a stale hit.
pub(super) struct Frame {
    pub(super) safe: Vec<Key>,
    pub(super) risky: Vec<Key>,
    pub(super) barrier: bool,
}

pub(super) struct Walk {
    pub(super) safe: HashMap<Key, ValueId>,
    pub(super) risky: HashMap<Key, (ValueId, usize)>,
    pub(super) frames: Vec<Frame>,
    pub(super) barrier_depth: Vec<usize>,
    pub(super) vgen: HashMap<ValueId, u32>,
    pub(super) replace: HashMap<ValueId, ValueId>,
    /// Each `global_ref` value's stand-in inside a key: the first one
    /// naming the same symbol. The instructions themselves stay (the
    /// probe and guard matchers read them in place); only what reads
    /// two of them compares equal.
    pub(super) twin: HashMap<ValueId, ValueId>,
    pub(super) multi: HashSet<ValueId>,
    /// The memory epoch at the current point (`epochs.rs`).
    pub(super) epoch: u32,
}

impl Walk {
    pub(super) fn resolve(&self, mut v: ValueId) -> ValueId {
        while let Some(&n) = self.replace.get(&v) {
            v = n;
        }
        v
    }

    pub(super) fn key_for(&self, ty: Type, kind: &InstKind) -> (Key, bool) {
        let mut resolved = kind.clone();
        rewrite_value_operands(&mut resolved, |v| {
            let v = self.resolve(v);
            self.twin.get(&v).copied().unwrap_or(v)
        });
        let mut gens: Vec<u32> = Vec::new();
        let mut risky = false;
        visit_value_operands(&resolved, |v| {
            if self.multi.contains(&v) {
                risky = true;
                gens.push(self.vgen.get(&v).copied().unwrap_or(0));
            } else {
                gens.push(0);
            }
        });
        ((ty, resolved, gens), risky)
    }

    pub(super) fn enter(&mut self, barrier: bool) {
        self.frames.push(Frame {
            safe: Vec::new(),
            risky: Vec::new(),
            barrier,
        });
        if barrier {
            self.barrier_depth.push(self.frames.len());
        }
    }

    pub(super) fn exit(&mut self) {
        let f = self.frames.pop().expect("scope underflow");
        for k in f.safe {
            self.safe.remove(&k);
        }
        for k in f.risky {
            self.risky.remove(&k);
        }
        if f.barrier {
            self.barrier_depth.pop();
        }
    }

    /// A risky entry is visible only when it was inserted at or below
    /// the innermost barrier (i.e. along the unique-pred chain).
    pub(super) fn risky_visible(&self, depth: usize) -> bool {
        self.barrier_depth.last().is_none_or(|&b| depth >= b)
    }

    /// A load of the address a store just wrote, in the store's own
    /// epoch, reads the stored value: enter it as that load's twin.
    pub(super) fn note_store(&mut self, func: &Function, kind: &InstKind) {
        let InstKind::Store(Operand::Value(val), ptr, off) = kind else {
            return;
        };
        let val = self.resolve(*val);
        if self.multi.contains(&val) {
            return;
        }
        let ty = func.values[val.0 as usize].ty;
        let (mut key, risky) = self.key_for(ty, &InstKind::Load(ty, *ptr, *off));
        key.2.push(self.epoch);
        let depth = self.frames.len();
        let frame = self.frames.last_mut().expect("in scope");
        if risky {
            self.risky.insert(key.clone(), (val, depth));
            frame.risky.push(key);
        } else {
            self.safe.insert(key.clone(), val);
            frame.safe.push(key);
        }
    }
}
