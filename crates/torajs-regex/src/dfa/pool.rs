//! Builder-side state shared by every intern of the subset
//! construction: the state pool with its flattened keys, the dedup
//! map, the work queue and the closure scratch.

use alloc::vec::Vec;

use crate::dfa::build_helpers::LeftByteAttr;
use crate::dfa::ctx::{PositionCtx, closure_into};
use crate::dfa::search::DfaState;
use crate::dfa::sparse::{KeyMap, SparseSet};
use crate::program::Program;

/// Sorted PC list; every ready / deferred set in the pool is one.
pub(super) type PcList = Vec<u32>;

/// What expanding a state needs: ready PCs (u_skip=0), per-u_skip
/// deferred PCs (`deferred[i]` becomes ready in `i + 1` continuation
/// bytes, chunk 10b) and the attr. Carries the original attr so
/// `pc_set_is_accept_at_end` and the expansion closure see the true
/// left-byte class (the pool key may have folded attr onto
/// `TextStart` for dedup when `needs_attr_split` is false).
#[derive(Clone)]
pub(super) struct StateItem {
    pub(super) ready: PcList,
    pub(super) deferred: [PcList; 3],
    pub(super) attr: LeftByteAttr,
}

impl StateItem {
    pub(super) fn empty() -> Self {
        Self {
            ready: PcList::new(),
            deferred: Default::default(),
            attr: LeftByteAttr::TextStart,
        }
    }
}

/// Separator inside a flattened state key; never a valid PC.
const KEY_SEP: u32 = u32::MAX;

/// The builder state every intern shares: the state pool, its dedup
/// map, the work queue of states whose rows are still unexpanded, the
/// poison flag and the closure scratch.
pub(super) struct Pool {
    pub(super) states: Vec<DfaState>,
    /// `keys[i]` is the flattened `(ready, deferred, attr)` of state i.
    pub(super) keys: Vec<PcList>,
    /// `items[i]` is what expanding state i's row starts from.
    pub(super) items: Vec<StateItem>,
    pub(super) set_to_idx: KeyMap,
    pub(super) key_scratch: PcList,
    pub(super) closed: SparseSet,
    pub(super) closure_work: Vec<usize>,
    pub(super) work: Vec<u32>,
    pub(super) poisoned: bool,
}

impl Pool {
    pub(super) fn new(prog: &Program) -> Self {
        Self {
            states: Vec::new(),
            keys: Vec::new(),
            items: Vec::new(),
            set_to_idx: KeyMap::new(),
            key_scratch: PcList::new(),
            closed: SparseSet::new(prog.len()),
            closure_work: Vec::new(),
            work: Vec::new(),
            poisoned: false,
        }
    }

    /// Drop every state and push the dead state 0. Its key is never
    /// looked up — `intern_state` answers empty sets directly.
    pub(super) fn reset(&mut self) {
        self.states.clear();
        self.keys.clear();
        self.items.clear();
        self.set_to_idx = KeyMap::new();
        self.work.clear();
        self.states.push(DfaState::default());
        self.keys.push(PcList::new());
        self.items.push(StateItem::empty());
    }

    /// ε-close `seeds` under `ctx` with WBound / NWBound left terminal
    /// and return the members sorted.
    pub(super) fn closure_sorted(
        &mut self,
        prog: &Program,
        seeds: &[u32],
        ctx: PositionCtx,
    ) -> PcList {
        self.closed.clear();
        let seeds = seeds.iter().map(|&pc| pc as usize);
        closure_into(
            prog,
            seeds,
            ctx,
            None,
            &mut self.closed,
            &mut self.closure_work,
        );
        let mut out = PcList::new();
        self.closed.sorted_into(&mut out);
        out
    }
}

pub(super) fn flatten_key(
    out: &mut PcList,
    ready: &[u32],
    deferred: &[PcList; 3],
    attr: LeftByteAttr,
) {
    out.clear();
    out.extend_from_slice(ready);
    for d in deferred {
        out.push(KEY_SEP);
        out.extend_from_slice(d);
    }
    out.push(KEY_SEP);
    out.push(attr as u32);
}
