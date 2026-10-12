//! What the chainer knows about the whole function: its dominators
//! and the edges the lowerer's guards mark cold.

use super::{DomView, is_cold};
use crate::id_hash::HashSet;

use torajs_core::ssa::branch_prob::cold_arms;
use torajs_core::ssa::{BlockId, Function};

/// What the chainer knows about the whole function.
pub(crate) struct ChainFacts<'a> {
    pub(crate) dom: DomView<'a>,
    /// Edges `(from, to)` into an arm the lowerer's guards mark cold.
    pub(super) cold_edges: HashSet<(u32, u32)>,
}

impl<'a> ChainFacts<'a> {
    pub(crate) fn new(func: &Function, dom: DomView<'a>) -> Self {
        Self::with_cold_edges(dom, cold_edges(func))
    }

    /// The facts with the cold edges already read off the function.
    pub(crate) fn with_cold_edges(dom: DomView<'a>, cold_edges: HashSet<(u32, u32)>) -> Self {
        ChainFacts { dom, cold_edges }
    }

    /// Hands back the cold edges, for the same function changed in
    /// ways that leave them as they were.
    pub(crate) fn into_cold_edges(self) -> HashSet<(u32, u32)> {
        self.cold_edges
    }

    /// Every path to `block` inside `body` runs a call, ends the
    /// function, or takes a predicted-cold edge: read off the
    /// dominators of the block, which every path passes through.
    pub(crate) fn cold_path(
        &self,
        func: &Function,
        in_body: &impl Fn(u32) -> bool,
        block: u32,
    ) -> bool {
        let mut cur = block;
        loop {
            if is_cold(func, cur) {
                return true;
            }
            let Some(idom) = self.dom.immediate_dominator(BlockId(cur)) else {
                return false;
            };
            if !in_body(idom.0) {
                return false;
            }
            if self.cold_edges.contains(&(idom.0, cur)) {
                return true;
            }
            cur = idom.0;
        }
    }

    /// `cold_path` of each of `starts`, sharing the walks: the walks
    /// up from the blocks of one loop meet early.
    pub(super) fn cold_paths(
        &self,
        func: &Function,
        in_body: &impl Fn(u32) -> bool,
        starts: &[u32],
    ) -> Vec<bool> {
        let mut known: Vec<Option<bool>> = vec![None; func.blocks.len()];
        let mut path: Vec<u32> = Vec::new();
        let mut answer = |start: u32| -> bool {
            let mut cur = start;
            let cold = loop {
                if let Some(k) = known[cur as usize] {
                    break k;
                }
                if is_cold(func, cur) {
                    break true;
                }
                match self.dom.immediate_dominator(BlockId(cur)) {
                    Some(idom) if in_body(idom.0) => {
                        if self.cold_edges.contains(&(idom.0, cur)) {
                            break true;
                        }
                        path.push(cur);
                        cur = idom.0;
                    }
                    _ => break false,
                }
            };
            // a block walked through is as cold as its dominator
            known[cur as usize] = Some(cold);
            for b in path.drain(..) {
                known[b as usize] = Some(cold);
            }
            cold
        };
        starts.iter().map(|&b| answer(b)).collect()
    }
}

/// Edges `(from, to)` into an arm the lowerer's guards mark cold.
fn cold_edges(func: &Function) -> HashSet<(u32, u32)> {
    (cold_arms(func).into_iter())
        .map(|(from, to, _)| (from.0, to.0))
        .collect()
}
