//! The order one function's plans are applied in.
//!
//! Applying a plan points its readers at the out cells: it rewrites
//! the blocks it found reading the loop's cells, and only those. A
//! reader inside another plan's loop, or entering that loop and so
//! seeding its cells, has to be rewritten before that loop is copied
//! or seeded; a copy made from the reader as it was reads the loop
//! cell itself, which the fast copy of the first loop never writes.
//!
//! Planning order does not give that. Plans are made outermost first,
//! and the loop that reads another's cell may be the larger one (a
//! string built by one loop, scanned inside a nest of two) or the
//! smaller (a sum folded by the next loop on the same bound).

use crate::id_hash::HashSet;

use torajs_core::ssa::{BlockId, Function};

use super::Plan;
use crate::float_demote::cfg::successors;

/// The plans in an order that applies a plan before every plan whose
/// loop one of its readers sits in or enters, and how many were left
/// out: a cycle of that relation has no such order, and is broken by
/// leaving out the earliest planned plan still waiting.
pub(super) fn applying_order(func: &Function, plans: Vec<Plan>) -> (Vec<Plan>, u32) {
    let readers: Vec<HashSet<BlockId>> = plans
        .iter()
        .map(|p| p.readers.iter().copied().collect())
        .collect();
    let touched: Vec<HashSet<BlockId>> = plans.iter().map(|p| touched(func, p)).collect();
    let (order, dropped) = order(&readers, &touched);
    let mut slots: Vec<Option<Plan>> = plans.into_iter().map(Some).collect();
    let ordered = order.iter().filter_map(|&i| slots[i].take()).collect();
    (ordered, dropped.len() as u32)
}

/// The loop's blocks and the blocks entering its header.
fn touched(func: &Function, plan: &Plan) -> HashSet<BlockId> {
    let mut out: HashSet<BlockId> = plan.region.iter().copied().collect();
    for block in &func.blocks {
        if successors(&block.term).any(|s| s == plan.header) {
            out.insert(block.id);
        }
    }
    out
}

/// Indices in applying order, then the ones left out. Among the plans
/// free to go, the earliest planned goes first.
fn order(readers: &[HashSet<BlockId>], touched: &[HashSet<BlockId>]) -> (Vec<usize>, Vec<usize>) {
    let before = |p: usize, q: usize| p != q && readers[p].iter().any(|b| touched[q].contains(b));
    let mut left: Vec<usize> = (0..readers.len()).collect();
    let mut order = Vec::new();
    let mut dropped = Vec::new();
    while !left.is_empty() {
        match left
            .iter()
            .position(|&q| !left.iter().any(|&p| before(p, q)))
        {
            Some(i) => order.push(left.remove(i)),
            None => dropped.push(left.remove(0)),
        }
    }
    (order, dropped)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(ids: &[u32]) -> HashSet<BlockId> {
        ids.iter().map(|&i| BlockId(i)).collect()
    }

    #[test]
    fn a_plan_goes_before_the_loops_its_readers_sit_in_or_enter() {
        // planned 0, 1, 2: 0's loop reads 1's cell, and 1's preheader
        // reads 2's
        let readers = [set(&[]), set(&[5]), set(&[9])];
        let touched = [set(&[4, 5]), set(&[1, 2, 9]), set(&[7, 8])];
        assert_eq!(order(&readers, &touched), (vec![2, 1, 0], vec![]));
    }

    #[test]
    fn unrelated_plans_keep_the_planning_order() {
        let readers = [set(&[3]), set(&[6])];
        let touched = [set(&[1, 2]), set(&[4, 5])];
        assert_eq!(order(&readers, &touched), (vec![0, 1], vec![]));
    }

    #[test]
    fn a_cycle_is_broken_by_leaving_its_earliest_plan_out() {
        let readers = [set(&[4]), set(&[1]), set(&[])];
        let touched = [set(&[1]), set(&[4]), set(&[7])];
        assert_eq!(order(&readers, &touched), (vec![2, 1], vec![0]));
    }
}
