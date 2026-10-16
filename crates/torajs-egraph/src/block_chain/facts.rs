//! What the chainer knows about the whole function: its dominators
//! and the edges the lowerer's guards mark cold.

use super::{DomView, is_cold};

use torajs_core::ssa::branch_prob::cold_arms;
use torajs_core::ssa::{BlockId, Function};

/// What the chainer knows about the whole function.
pub(crate) struct ChainFacts<'a> {
    pub(crate) dom: DomView<'a>,
    /// Edges `(from, to)` into an arm the lowerer's guards mark cold.
    pub(super) cold_edges: ColdEdges,
}

impl<'a> ChainFacts<'a> {
    pub(crate) fn new(func: &Function, dom: DomView<'a>) -> Self {
        Self::with_cold_edges(dom, ColdEdges::of(func))
    }

    /// The facts with the cold edges already read off the function.
    pub(crate) fn with_cold_edges(dom: DomView<'a>, cold_edges: ColdEdges) -> Self {
        ChainFacts { dom, cold_edges }
    }

    /// Hands back the cold edges, for the same function changed in
    /// ways that leave them as they were.
    pub(crate) fn into_cold_edges(self) -> ColdEdges {
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
            if self.cold_edges.contains(idom.0, cur) {
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
                        if self.cold_edges.contains(idom.0, cur) {
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

/// The edges `(from, to)` into the arms the lowerer's guards mark
/// cold: at most one per branching block, so kept as each block's cold
/// arm by block id.
#[derive(Debug, Clone)]
pub(crate) struct ColdEdges(Vec<u32>);

/// No cold arm leaves the block.
const NO_ARM: u32 = u32::MAX;

impl ColdEdges {
    fn of(func: &Function) -> Self {
        let mut arms = Vec::new();
        for (from, to, _) in cold_arms(func) {
            let i = from.0 as usize;
            if arms.len() <= i {
                arms.resize(i + 1, NO_ARM);
            }
            arms[i] = to.0;
        }
        ColdEdges(arms)
    }

    pub(crate) fn contains(&self, from: u32, to: u32) -> bool {
        self.0.get(from as usize) == Some(&to)
    }

    /// The edges, by ascending `from`.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (u32, u32)> + '_ {
        (self.0.iter().enumerate())
            .filter(|(_, to)| **to != NO_ARM)
            .map(|(from, &to)| (from as u32, to))
    }
}

impl PartialEq for ColdEdges {
    fn eq(&self, other: &Self) -> bool {
        self.iter().eq(other.iter())
    }
}
