//! The number flow through `any` values: representation selection
//! for the `any` cells of a loop that hold a number all the way round
//! — the `let total = 0` the checker widened to `any` because a `+`
//! with an `any` addend feeds it. The AOT analogue of TurboFan's
//! representation selection over a tagged φ (`ChangeTaggedToFloat64`
//! / `ChangeFloat64ToTagged` hoisted off the loop-carried edge) and
//! of the DFG's double speculation of a φ, with loop versioning and a
//! side exit standing in for deoptimization.
//!
//! The lowering writes and reads the number encoding through six
//! typed nodes (`any_is_number`, `any_is_int32`, `any_to_int32`,
//! `any_to_f64`, `box_int32`, `box_f64`). After φ promotion, a loop's
//! `any` cells that every in-loop def boxes and every in-loop use
//! unboxes ([`web`]) get a copy of the loop where each such cell is
//! three scalar lanes ([`apply`]): the loop-carried sum then never
//! leaves its register. Any in-loop def or use the lanes cannot
//! serve — the `+` kernel call when an addend is not a number, a
//! call taking the cell — is reached only through a side exit that
//! boxes the lanes and continues in the loop as it was; an entering
//! value that is not a known number is tested once on the way in.
//! A loop either gets its fast copy whole or is left alone.
//!
//! What remains boxed is expanded to the bit operations by
//! [`expand`], right after. `TORAJS_ANYNUM_CELL_OFF=1` skips the
//! representation pass; `TORAJS_ANYNUM_CELL_STATS=1` dumps counters.

mod apply;
pub mod dec_split;
pub mod expand;
pub mod inc_sink;
mod live;
pub mod motion_facts;
mod number_facts;
mod preds;
pub mod rc_fold;
mod retain_walk;
mod web;

use crate::id_hash::HashSet;

use torajs_core::ssa::dominator::DominatorTree;
use torajs_core::ssa::loop_analysis::{LoopAnalysis, NaturalLoop};
use torajs_core::ssa::{BlockId, Function, InstKind, Module, Operand, Type};

use crate::float_demote::cfg::multi_def_values;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct AnyNumStats {
    pub loops_versioned: u32,
    /// web cells given lanes
    pub cells_unboxed: u32,
    /// loops with a web the plan did not close
    pub skipped: u32,
}

pub fn unbox_loop_cells(module: &mut Module) -> AnyNumStats {
    let mut stats = AnyNumStats::default();
    for func in module.funcs.iter_mut() {
        let dense = func
            .blocks
            .iter()
            .enumerate()
            .all(|(i, b)| b.id.0 as usize == i);
        if func.is_declaration() || !dense || !may_hold_web(func) {
            continue;
        }
        version_loops(func, &mut stats);
    }
    stats
}

/// A web starts at a copy into an `any` cell and a plan needs a lane
/// read of it: a function with no such copy or no such read has no loop
/// to plan, and is not read for dominators and loops.
fn may_hold_web(func: &Function) -> bool {
    let insts = || func.blocks.iter().flat_map(|b| b.insts.iter());
    insts().any(|i| matches!(i.kind, InstKind::Copy(Type::Any, Operand::Value(_))))
        && insts().any(|i| {
            matches!(
                i.kind,
                InstKind::AnyIsInt32(_) | InstKind::AnyToInt32(_) | InstKind::AnyToF64(..)
            )
        })
}

/// Plans and installs one loop at a time, innermost first, reading
/// the function afresh after each install: a later loop's readers may
/// be blocks an earlier install copied or rewrote (a second sum loop
/// starting from the first one's total), and a plan made before that
/// install would leave the copies reading the old cell. The blocks an
/// install made or versioned are not planned again; a loop around
/// them is left alone. When no loop is left to plan, the function is
/// not read again.
fn version_loops(func: &mut Function, stats: &mut AnyNumStats) {
    let mut taken: HashSet<BlockId> = HashSet::default();
    let mut seen: HashSet<BlockId> = HashSet::default();
    loop {
        let before = func.blocks.len();
        let cells = multi_def_values(func);
        let (plan, more) = {
            let dom = DominatorTree::compute(func);
            let loops = LoopAnalysis::compute(func, &dom);
            let defs = web::single_defs(func, &cells);
            let mut innermost_first: Vec<_> = loops.loops().iter().collect();
            innermost_first.sort_by_key(|l| (l.body.len(), l.header.0));
            let open = |l: &&NaturalLoop, taken: &HashSet<BlockId>| {
                l.header.0 != 0 && !l.body.iter().any(|b| taken.contains(b))
            };
            let mut found = None;
            let mut rest = innermost_first.iter();
            for l in rest.by_ref() {
                if !open(l, &taken) || !seen.insert(l.header) {
                    continue;
                }
                match web::plan_loop(func, l, &dom, &cells, &defs) {
                    web::Verdict::Plan(plan) => {
                        found = Some(plan);
                        break;
                    }
                    web::Verdict::Skipped => stats.skipped += 1,
                    web::Verdict::NoWeb => {}
                }
            }
            // the loops a next round would plan: unseen, and clear of
            // what this install versions
            let more = found.as_ref().is_some_and(|plan: &web::Plan| {
                let versioned: HashSet<BlockId> = plan.region.iter().copied().collect();
                rest.any(|l| {
                    open(l, &taken)
                        && !seen.contains(&l.header)
                        && !l.body.iter().any(|b| versioned.contains(b))
                })
            });
            (found, more)
        };
        let Some(plan) = plan else {
            return;
        };
        apply::apply(func, &plan, &cells);
        stats.loops_versioned += 1;
        stats.cells_unboxed += plan.web.len() as u32;
        if !more {
            return;
        }
        taken.extend(plan.region.iter().copied());
        taken.extend((before..func.blocks.len()).map(|b| BlockId(b as u32)));
    }
}

#[cfg(test)]
mod tests;
