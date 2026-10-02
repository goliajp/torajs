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

/// BFS work-queue entry: ready PCs (u_skip=0), per-u_skip deferred
/// PCs (`deferred[i]` becomes ready in `i + 1` continuation bytes,
/// chunk 10b), the attr and the assigned state index. Carries the
/// original attr so `pc_set_is_accept_at_end` sees the true left-byte
/// class (the pool key may have folded attr onto `TextStart` for
/// dedup when `needs_attr_split` is false).
pub(super) type WorkItem = (PcList, [PcList; 3], LeftByteAttr, u32);

/// Separator inside a flattened state key; never a valid PC.
const KEY_SEP: u32 = u32::MAX;

/// The BFS builder state every intern shares: the state pool, its
/// dedup map, the work queue, the poison flag and the closure scratch.
pub(super) struct Pool {
    pub(super) states: Vec<DfaState>,
    /// `keys[i]` is the flattened `(ready, deferred, attr)` of state i.
    pub(super) keys: Vec<PcList>,
    pub(super) set_to_idx: KeyMap,
    pub(super) key_scratch: PcList,
    pub(super) closed: SparseSet,
    pub(super) closure_work: Vec<usize>,
    pub(super) work: Vec<WorkItem>,
    pub(super) poisoned: bool,
}

impl Pool {
    pub(super) fn new(prog: &Program) -> Self {
        Self {
            states: Vec::new(),
            keys: Vec::new(),
            set_to_idx: KeyMap::new(),
            key_scratch: PcList::new(),
            closed: SparseSet::new(prog.len()),
            closure_work: Vec::new(),
            work: Vec::new(),
            poisoned: false,
        }
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
