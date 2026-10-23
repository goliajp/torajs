//! The dominator walk's state: the two tables, their scope frames and
//! the keys entries are filed under.

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{
    Function, Inst, InstKind, Operand, Terminator, Type, ValueId, rewrite_value_operands,
    visit_value_operands,
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
    /// naming the same symbol; and each bit cast's, the dominating cast
    /// of the same operand. The instructions themselves stay (the
    /// probe and guard matchers read them in place); only what reads
    /// two of them compares equal.
    pub(super) twin: HashMap<ValueId, ValueId>,
    /// The casts among `twin`'s keys.
    pub(super) cast_twins: HashSet<ValueId>,
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

    /// `result` computes what the dominating `existing` did. A bit
    /// cast costs no instruction and deleting one only stretches the
    /// earlier register over the later uses, so it stays, and only
    /// what reads it keys as reading `existing`; anything else goes.
    pub(super) fn hit(
        &mut self,
        cast: bool,
        result: ValueId,
        existing: ValueId,
        stats: &mut super::LateGvnStats,
    ) {
        if cast {
            self.twin.insert(result, existing);
            self.cast_twins.insert(result);
        } else {
            self.replace.insert(result, existing);
            stats.dedups += 1;
        }
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
    /// epoch, reads the stored value: enter it as that load's twin. A
    /// multi-def value qualifies when no definition of it can run
    /// after the store (the merged cell of a diamond above it): it is
    /// then the one value everywhere the load's readers are.
    pub(super) fn note_store(&mut self, func: &Function, bi: usize, at: usize) {
        let InstKind::Store(Operand::Value(val), ptr, off) = &func.blocks[bi].insts[at].kind else {
            return;
        };
        let val = self.resolve(*val);
        if self.multi.contains(&val) && defined_after(func, bi, at, val) {
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

/// Can a definition of `v` run after instruction `at` of block `bi`?
fn defined_after(func: &Function, bi: usize, at: usize, v: ValueId) -> bool {
    let defines = |insts: &[Inst]| insts.iter().any(|i| i.result == Some(v));
    let succs = |b: usize| match &func.blocks[b].term {
        Terminator::Br(t) => [Some(t.0 as usize), None],
        Terminator::CondBr {
            then_blk, else_blk, ..
        } => [Some(then_blk.0 as usize), Some(else_blk.0 as usize)],
        Terminator::Ret(_) | Terminator::Unreachable => [None, None],
    };
    if defines(&func.blocks[bi].insts[at + 1..]) {
        return true;
    }
    let mut seen = vec![false; func.blocks.len()];
    let mut work: Vec<usize> = succs(bi).into_iter().flatten().collect();
    while let Some(b) = work.pop() {
        if std::mem::replace(&mut seen[b], true) {
            continue;
        }
        if defines(&func.blocks[b].insts) {
            return true;
        }
        work.extend(succs(b).into_iter().flatten());
    }
    false
}
