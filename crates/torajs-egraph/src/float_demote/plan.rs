//! Versioning feasibility planning (phase 1b-ii) — pick the regions
//! that can host guarded defs. Read-only over the pre-mutation shape.
//!
//! Region shape: a natural loop hosting in-set defs plus the
//! pre-chain — loop-external blocks holding the component's remaining
//! defs (the `sitofp` entry and cell init copies), each dominating
//! the header with all successors inside the region. Renamed clone
//! values (in-set values and single-def intermediates) must not
//! escape the region; the region keeps a single external entry.
//! Guarded values that no region hosts are evicted and the demotion
//! fixpoint re-runs without them — the pass never fires partially.

use std::collections::{HashMap, HashSet};

use torajs_core::ssa::{BlockId, Function, Operand, ValueId};

use crate::dominator::DominatorTree;
use crate::loop_analysis::LoopAnalysis;

use super::cfg::{
    collect_value_blocks, innermost_loop, multi_def_values, pred_map, successors, touched_values,
};
use super::guard::{GuardCheck, GuardSite};

/// One versionable region, planned against the pre-mutation shape.
pub(crate) struct RegionPlan {
    /// Region blocks (loop body + pre-chain), sorted.
    pub(super) region: Vec<BlockId>,
    /// The single region block with external predecessors.
    pub(super) entry: BlockId,
    /// Growth sites inside the region.
    pub(super) growth: Vec<GuardSite>,
    /// Entry checks (`sitofp` operands not provably exact); checked
    /// once on the region entry edge.
    pub(super) entry_guard: Vec<GuardCheck>,
}

impl RegionPlan {
    /// Pre-mutation positions of the growth sites (for the mutation
    /// walk to track through rewriting).
    pub(crate) fn growth_keys(&self) -> impl Iterator<Item = (BlockId, usize)> + '_ {
        self.growth.iter().map(|s| (s.block, s.inst_idx))
    }
}

/// Plan versioning regions. Candidate regions grow from the innermost
/// natural loop of every in-set def; every guarded site must land in
/// some region. `Err` carries the values whose guarded defs cannot be
/// hosted (caller evicts them from the demotion set and re-runs the
/// fixpoint).
pub(crate) fn plan_regions(
    func: &Function,
    set: &HashSet<ValueId>,
    growth_sites: Vec<GuardSite>,
    entry_sites: Vec<(BlockId, ValueId, Vec<GuardCheck>)>,
) -> Result<Vec<RegionPlan>, HashSet<ValueId>> {
    let dom = DominatorTree::compute(func);
    let loops = LoopAnalysis::compute(func, &dom);
    let value_blocks = collect_value_blocks(func);
    let preds = pred_map(func);

    // candidate headers: innermost loop of every in-set def block,
    // innermost-first
    let mut headers: Vec<(usize, BlockId)> = Vec::new();
    for (v, (defs, _)) in &value_blocks {
        if !set.contains(v) {
            continue;
        }
        for d in defs {
            if let Some((len, h)) = innermost_loop(&loops, *d) {
                if !headers.iter().any(|(_, eh)| *eh == h) {
                    headers.push((len, h));
                }
            }
        }
    }
    headers.sort_by_key(|(len, h)| (*len, h.0));

    let mut plans: Vec<RegionPlan> = Vec::new();
    let mut used: HashSet<BlockId> = HashSet::new();
    for (_, header) in headers {
        let body = loops
            .loops()
            .iter()
            .find(|l| l.header == header)
            .unwrap()
            .body
            .clone();
        if let Some(plan) = plan_one(
            func,
            set,
            &dom,
            &preds,
            &value_blocks,
            body,
            header,
            &growth_sites,
            &entry_sites,
            &used,
        ) {
            if plan.growth.is_empty() && plan.entry_guard.is_empty() {
                continue; // fully exact component — no versioning needed
            }
            used.extend(plan.region.iter().copied());
            plans.push(plan);
        }
    }

    // every guarded site must be hosted; an unhosted one has no slow
    // path to fall into
    let mut evict: HashSet<ValueId> = HashSet::new();
    for s in &growth_sites {
        if !plans.iter().any(|p| p.region.contains(&s.block)) {
            evict.insert(s.result);
        }
    }
    for (b, r, _) in &entry_sites {
        if !plans.iter().any(|p| p.region.contains(b)) {
            evict.insert(*r);
        }
    }
    if evict.is_empty() {
        Ok(plans)
    } else {
        Err(evict)
    }
}

#[expect(clippy::too_many_arguments, reason = "internal planning step")]
fn plan_one(
    func: &Function,
    set: &HashSet<ValueId>,
    dom: &DominatorTree,
    preds: &[Vec<BlockId>],
    value_blocks: &HashMap<ValueId, (Vec<BlockId>, Vec<BlockId>)>,
    body: Vec<BlockId>,
    header: BlockId,
    growth_sites: &[GuardSite],
    entry_sites: &[(BlockId, ValueId, Vec<GuardCheck>)],
    used: &HashSet<BlockId>,
) -> Option<RegionPlan> {
    let mut region: HashSet<BlockId> = body.iter().copied().collect();

    // grow the region until it hosts every touched in-set value's
    // defs and uses (pre-chain absorption), or fail
    loop {
        let touched = touched_values(func, set, &region);
        let mut missing: Vec<BlockId> = Vec::new();
        for v in &touched {
            let (defs, uses) = value_blocks.get(v)?;
            for b in defs.iter().chain(uses) {
                if !region.contains(b) && !missing.contains(b) {
                    missing.push(*b);
                }
            }
        }
        if missing.is_empty() {
            break;
        }
        for b in missing {
            // a pre-chain block dominates the header and flows only
            // into the region
            if dom.dominates(b, header)
                && successors(&func.blocks[b.0 as usize].term).all(|s| region.contains(&s))
            {
                region.insert(b);
            } else {
                return None;
            }
        }
    }

    if region.iter().any(|b| used.contains(b)) {
        return None;
    }

    // single entry: exactly one region block with external preds (or
    // the function entry), every other block's preds stay inside
    let mut entry = None;
    for &b in &region {
        let ext = preds[b.0 as usize].iter().any(|p| !region.contains(p));
        if ext || b.0 == 0 {
            if entry.is_some() {
                return None;
            }
            entry = Some(b);
        }
    }
    let entry = entry?;

    // renamed clone values must not escape: any value the clone will
    // rename (in-set or single-def intermediate defined inside) is
    // used only inside
    let cells = multi_def_values(func);
    for (v, (defs, uses)) in value_blocks {
        if !defs.iter().any(|b| region.contains(b)) {
            continue;
        }
        let renamed = set.contains(v) || !cells.contains(v);
        if renamed && uses.iter().any(|b| !region.contains(b)) {
            return None;
        }
    }

    let growth: Vec<GuardSite> = growth_sites
        .iter()
        .filter(|s| region.contains(&s.block))
        .cloned()
        .collect();

    // entry checks for hosted guarded sitofp defs; their operands
    // must be defined outside the region (available at the entry
    // edge), and a function-entry region has no edge to guard
    let mut entry_guard: Vec<GuardCheck> = Vec::new();
    for (b, _, checks) in entry_sites {
        if !region.contains(b) {
            continue;
        }
        for c in checks {
            if let Operand::Value(cv) = c.operand {
                let inside = value_blocks
                    .get(&cv)
                    .is_some_and(|(defs, _)| defs.iter().any(|d| region.contains(d)));
                if inside {
                    return None;
                }
            }
            entry_guard.push(*c);
        }
    }
    if !entry_guard.is_empty() && entry.0 == 0 {
        return None;
    }

    let mut region: Vec<BlockId> = region.into_iter().collect();
    region.sort_by_key(|b| b.0);
    Some(RegionPlan {
        region,
        entry,
        growth,
        entry_guard,
    })
}
