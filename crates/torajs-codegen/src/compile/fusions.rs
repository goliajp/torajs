//! The instruction fusions of one function, planned once, and the
//! question the inst loop asks before emitting an instruction alone:
//! is it part of a fused group, and if so, does it emit that group or
//! is it absorbed into a neighbour?

use std::collections::HashMap;

use torajs_core::ssa::{Block, Function, Inst, InstKind};

use super::brfuse::{self, FusedCmp, FusedSelCmp};
use super::cmp;
use super::selinc::{self, SelectIncs};
use super::store_pair::{self, StorePair};
use crate::regalloc::Assignment;

pub(super) struct Fusions {
    /// Block-tail single-use ICmp + CondBr pairs fuse into cmp+b.cond
    /// (or cbz/cbnz) — the inst loop skips the compare, the terminator
    /// emits it. See compile/brfuse.rs.
    cmps: HashMap<u32, FusedCmp>,
    select_cmps: HashMap<(u32, u32), FusedSelCmp>,
    /// `select c, (x+1), x` collapses into one CSINC, absorbing the ADD
    /// (and the ICmp behind it, which the ADD used to keep out of
    /// fusible_select_cmps' adjacency window). See compile/selinc.rs.
    select_incs: SelectIncs,
    /// Adjacent stores into neighbouring words, keyed by the first;
    /// the second is absorbed. See compile/store_pair.rs.
    store_pairs: HashMap<(u32, u32), StorePair>,
}

impl Fusions {
    pub(super) fn of(func: &Function, alloc: &Assignment) -> Self {
        Fusions {
            cmps: brfuse::fusible_cmps(func),
            select_cmps: brfuse::fusible_select_cmps(func, alloc),
            select_incs: selinc::fusible_select_incs(func),
            store_pairs: store_pair::plan(func, alloc),
        }
    }

    /// The compare the terminator of `block` emits fused.
    pub(super) fn cmp_of(&self, block: u32) -> Option<&FusedCmp> {
        self.cmps.get(&block)
    }

    /// True when instruction `ii` of `block` was handled here (emitted
    /// as its fused group, or absorbed into one); false when the
    /// caller emits it on its own.
    pub(super) fn emit(
        &self,
        bytes: &mut Vec<u8>,
        block: &Block,
        ii: usize,
        inst: &Inst,
        alloc: &Assignment,
    ) -> bool {
        let (b, i) = (block.id.0, ii as u32);
        if self.cmps.contains_key(&b) && ii == block.insts.len() - 1 {
            return true; // the CondBr emits this compare fused
        }
        if self.select_cmps.contains_key(&(b, i)) {
            return true; // the next Select emits this compare fused
        }
        if self.select_incs.absorbed(b, i) {
            return true; // the CSINC downstream emits this add / compare
        }
        if let Some(plan) = self.select_incs.plan(b, i) {
            cmp::emit_select_inc(bytes, inst, plan, alloc);
            return true;
        }
        if let Some(pair) = self.store_pairs.get(&(b, i)) {
            store_pair::emit(bytes, pair, alloc);
            return true;
        }
        if ii > 0 && self.store_pairs.contains_key(&(b, i - 1)) {
            return true; // the store before emitted this one as its pair
        }
        if let InstKind::Select(ty, _, then_op, else_op) = &inst.kind
            && ii > 0
            && let Some(fc) = self.select_cmps.get(&(b, i - 1))
        {
            cmp::emit_select_fused(bytes, inst, ty, fc, then_op, else_op, alloc);
            return true;
        }
        false
    }
}
