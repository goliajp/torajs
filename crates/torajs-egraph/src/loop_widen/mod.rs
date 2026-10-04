//! Loop widening: run an integer sum loop two i64 lanes per vector,
//! four vectors per step, in front of the scalar loop it came from.
//!
//! The scalar loop float_demote produces checks every element and
//! every partial sum against the exact-integer range of an f64 and
//! leaves for the f64 copy of the loop at the first one outside it.
//! The widened path keeps that contract by deciding per block of
//! elements: the checks are reduced over the block (an or-fold of the
//! checked lane values, the guards on the block's final sum) and a
//! block that fails any of them is handed whole to the scalar loop,
//! which finds the exact element. See `emit` for the block structure
//! and `shape` for which loops qualify.

mod builder;
mod defs;
mod emit;
mod flow;
mod guard;
mod shape;
#[cfg(test)]
mod tests;

use torajs_core::ssa::dominator::DominatorTree;
use torajs_core::ssa::loop_analysis::LoopAnalysis;
use torajs_core::ssa::{Function, InstKind, Module, Type};

#[derive(Debug, Default)]
pub struct LoopWidenStats {
    pub loops_widened: u32,
}

pub fn widen_sum_loops(module: &mut Module) -> LoopWidenStats {
    let mut stats = LoopWidenStats::default();
    for func in module.funcs.iter_mut() {
        if !reads_i64_elements(func) {
            continue;
        }
        let dom = DominatorTree::compute(func);
        let la = LoopAnalysis::compute(func, &dom);
        let defs = defs::DefSites::of(func);
        // loops come sorted by header; widening only appends blocks
        // and retargets edges into a header, so later plans hold
        let plans: Vec<shape::Plan> = la
            .loops()
            .iter()
            .filter_map(|lp| shape::match_loop(func, lp, &defs))
            .collect();
        for p in &plans {
            emit::widen(func, p);
            stats.loops_widened += 1;
        }
    }
    stats
}

/// The cheap filter before any loop analysis: an i64 element read.
fn reads_i64_elements(func: &Function) -> bool {
    func.blocks.iter().flat_map(|b| &b.insts).any(|i| {
        matches!(
            i.kind,
            InstKind::LoadDyn(Type::I64, ..) | InstKind::LoadDynScaled8(Type::I64, ..)
        )
    })
}
