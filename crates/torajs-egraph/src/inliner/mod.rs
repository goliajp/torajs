//! Cluster E Inliner — module-level cost-benefit inlining pass.
//!
//! Reference: LLVM `lib/Analysis/InlineCost.cpp` (`InlineCostAnalyzer`)
//! and Go `src/cmd/compile/internal/inline/inl.go` (cost model +
//! hairyness + budget). Both use cost-benefit + size budget + recursion
//! guard + hot-path tagging; LOC-threshold-only is explicitly rejected
//! (RFC 20260609-torajs-codegen-optimizer §9 Q4).
//!
//! Phase 0 (shipped) — decision substrate (this file):
//!
//! * Enumerate direct-call sites (`InstKind::Call(FuncId, _)`).
//! * Compute each callee's body cost via `cost::cost_of_kind` summed
//!   over its blocks.
//! * Apply per-callee cost ceiling + per-caller cumulative budget +
//!   self-recursion guard + declaration / indirect-call exclusion.
//! * Report decisions via `InlinerStats`.
//!
//! Phase 1.0a (shipped) — splice mutation API in `splice` sub-module:
//!
//! * `SpliceError` + `inline_single_block_leaf` materialise a single-
//!   block leaf callee into the caller block, removing the matching
//!   call instruction. Fresh `ValueId`s for non-parameter callee
//!   values; callee parameters substituted by caller-supplied `args`.
//!
//! Phase 1.0b (this commit) — driver wired into production pipeline:
//!
//! * `inline_module(&mut Module)` switches to mutating signature and
//!   gains an emit pass: every would-inline candidate that also clears
//!   the splice's narrow scope (single-block leaf + Ret terminator) is
//!   spliced in reverse `(blk, site)` order so coordinates remain
//!   valid. The `would_inline - inlined` gap exposes splice rejections
//!   (multi-block callee, non-Ret terminator, ...) to the attribution
//!   layer.
//! * `TORAJS_INLINER_OFF=1` env-gate short-circuits to dry-run:
//!   classification + stats still happen but no mutation occurs. Used
//!   for bisection when validating that a perf regression is or is not
//!   caused by the inliner.
//! * `transform_module` in `lib.rs` now invokes the inliner before the
//!   per-function `EgraphPass`. Identity-aliased values bound by the
//!   splice get GVN-collapsed by `elaborate.rs`'s existing identity
//!   handling.
//!
//! Future phases — Phase 2 generalises beyond single-block leaves
//! (block-split + Φ-node insertion) and threads `LoopAnalysis` so
//! callee cost is depth-weighted (matching the elaborator's existing
//! LICM weighting in `cost::scale_for_depth`).

mod emit;
mod order;
mod rewrite;
mod sole_site;
mod splice;
mod splice2;
mod splice2_emit;
mod stats;

use crate::cost::{Cost, cost_of_kind};
use torajs_core::ssa::{FuncId, Function, InstKind, Module, Operand, ValueId};

/// `(block index, site index, callee, call args, call result)` of one
/// call site the inliner splices
type InlineTarget = (usize, usize, FuncId, Vec<Operand>, Option<ValueId>);

pub use splice::{SpliceError, inline_single_block_leaf};
pub use splice2::{SpliceMultiError, inline_multi_block_leaf};
pub use stats::InlinerStats;

/// Budget configuration for `inline_module`. Defaults are conservative
/// starting points calibrated against LLVM `-O2` hint thresholds
/// (LLVM's `InlineThreshold` 225 corresponds to roughly 25 instructions
/// of mixed integer / load / branch cost in our `Cost` units). The
/// caller-cumulative budget is 3× that to give one caller room to
/// absorb a handful of small leaves without ballooning text size.
#[derive(Debug, Clone, Copy)]
pub struct InlineBudget {
    /// Single-callee cost ceiling. A callee whose body cost exceeds
    /// this is rejected regardless of caller hotness or remaining
    /// caller budget.
    pub callee_cost_ceiling: Cost,
    /// Per-caller cumulative inlined-cost budget. Once one caller's
    /// `would_inline` decisions sum past this, further sites for the
    /// same caller are skipped with `SkipReason::CallerBudgetExhausted`.
    pub caller_total_budget: Cost,
    /// Maximum recursion depth allowed via inlining (the GCC
    /// `max-inline-recursive-depth` shape). `0` rejects every direct
    /// self-recursive call site with `SkipReason::Recursion` — matching
    /// LLVM, which never inlines self-recursion. `d > 0` runs `d`
    /// rounds per caller: the first classifies every site; each later
    /// one classifies only the self-calls left in the body, with a
    /// fresh caller budget. Every self-site splices the caller's
    /// original body from the snapshot taken before its first splice,
    /// so one out-of-line frame covers `d + 1` levels of the recursion.
    /// History: depth 1 first regressed fib40 +8.3% on mini
    /// (2026-06-10, 3-run medians) because the multi-ret join slot's
    /// store/load stayed in the hot path with no promotion pass to
    /// clear it; with slot_forward + mem2reg + phi_promote the join
    /// slot dissolves into φ copies.
    pub max_recursion_depth: u32,
    /// Ceiling for a callee with one direct call site and no address
    /// taken (`sole_site.rs`), which is also not charged to the
    /// caller's budget.
    pub sole_site_ceiling: Cost,
}

impl Default for InlineBudget {
    fn default() -> Self {
        Self {
            callee_cost_ceiling: Cost::new(225),
            caller_total_budget: Cost::new(675),
            max_recursion_depth: 2,
            sole_site_ceiling: Cost::new(2000),
        }
    }
}

/// Per-call-site inlining decision. `Inline` is only reached when all
/// cost-benefit, budget, and recursion checks pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InlineDecision {
    /// Site should be inlined; carries the callee body cost (the value
    /// counted against the caller cumulative budget for subsequent
    /// sites in the same caller).
    Inline { callee_cost: Cost },
    /// Site rejected for the given reason; tracked in `InlinerStats`
    /// per-bucket so the next phase can attribute regressions /
    /// missed opportunities.
    Skip(SkipReason),
}

/// Reason a candidate call site was not inlined. Buckets line up with
/// counters in `InlinerStats` for direct attribution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// Callee body cost exceeded `InlineBudget::callee_cost_ceiling`.
    CalleeTooLarge,
    /// Caller's cumulative inlined-cost would exceed
    /// `InlineBudget::caller_total_budget` if this site fired.
    CallerBudgetExhausted,
    /// Callee is a declaration (`is_declaration()` — no blocks); there
    /// is no body to inline. Extern intrinsics and forward-declared
    /// imports land here.
    CalleeIsDeclaration,
    /// Direct self-recursion and `max_recursion_depth == 0`. Phase 0
    /// also lands here when a call would close a cycle of any depth
    /// (full call-graph cycle detector arrives with bounded recursive
    /// inlining in Phase 2+).
    Recursion,
    /// Call site is `InstKind::CallIndirect` (via function pointer).
    /// Phase 0 scope is direct calls only; indirect inlining requires
    /// devirtualization / call-graph analysis (Phase 2+).
    Indirect,
    /// r505 (A12) — the callee is the synthesized class prologue
    /// (`torajs_core::ast::CLASS_PROLOGUE_FN`): its `bl` in main is a
    /// link-judged site (assumed away with the fn when no class cell
    /// is observable), which only exists while the body stays its
    /// own fn. Inlining it would put the whole dynobj / anyvalue
    /// world back into main on every class program.
    LinkJudgedUnit,
    /// The callee is a synthesized per-class release fn
    /// (`Module::class_drop_fns`): lowering made it a
    /// unit so every site is one call, the self-referential field's
    /// release is a direct call to it, and the retain/release
    /// peephole pairs it by fid. Spliced back per site it would be
    /// the inline expansion it replaced.
    ClassDropUnit,
    /// The callee contains an `InstKind::Suspend`: its body parks and
    /// is re-entered through its own frame, which a splice into the
    /// caller would not have. Such a function still inlines others.
    Suspends,
}

/// Sum `cost_of_kind` over every instruction in every block of `func`.
///
/// Phase 0 uses a flat per-instruction sum; Phase 1 threads
/// `LoopAnalysis` so loop bodies multiply by `BR_PRED_FACTOR^depth`
/// (matching the elaborator's existing LICM weighting in
/// `cost::scale_for_depth`).
pub fn callee_body_cost(func: &Function) -> Cost {
    let mut total = Cost::ZERO;
    for blk in &func.blocks {
        for inst in &blk.insts {
            total = total.add(cost_of_kind(&inst.kind));
        }
    }
    total
}

/// Whether `func` parks anywhere (an `InstKind::Suspend` in its body).
fn suspends(func: &Function) -> bool {
    (func.blocks.iter().flat_map(|b| &b.insts)).any(|i| matches!(i.kind, InstKind::Suspend(..)))
}

/// Classify every `InstKind::Call` site in `module.funcs[caller_idx]`
/// against `budget`, returning per-site decisions paired with the
/// `(caller_blk_idx, site_idx)` coordinates needed by the Phase 1.0b
/// driver to dispatch to `splice::inline_single_block_leaf`.
/// `callee_costs[i]` must equal `callee_body_cost(&module.funcs[i])`
/// (precomputed by `inline_module_with_budget` so callee bodies are
/// scanned exactly once per pass).
///
/// Decisions are returned in source order. `CallIndirect` sites land
/// as `Skip(Indirect)` so the caller can count them toward
/// `InlinerStats::skipped_indirect` without re-scanning. With
/// `self_only` every site but a direct self-call is passed over.
fn classify_caller_sites(
    module: &Module,
    caller_idx: usize,
    budget: &InlineBudget,
    callee_costs: &[Cost],
    sole: &[bool],
    parks: &[bool],
    self_only: bool,
) -> Vec<(usize, usize, InlineDecision)> {
    let caller = &module.funcs[caller_idx];
    let mut decisions = Vec::new();
    let mut caller_spent = Cost::ZERO;
    for (blk_idx, blk) in caller.blocks.iter().enumerate() {
        for (site_idx, inst) in blk.insts.iter().enumerate() {
            match &inst.kind {
                InstKind::Call(callee_id, _)
                    if !self_only || callee_id.0 as usize == caller_idx =>
                {
                    let callee_ix = callee_id.0 as usize;
                    let callee = &module.funcs[callee_ix];
                    if callee.is_declaration() {
                        decisions.push((
                            blk_idx,
                            site_idx,
                            InlineDecision::Skip(SkipReason::CalleeIsDeclaration),
                        ));
                        continue;
                    }
                    if parks[callee_ix] {
                        decisions.push((
                            blk_idx,
                            site_idx,
                            InlineDecision::Skip(SkipReason::Suspends),
                        ));
                        continue;
                    }
                    if callee.name == torajs_core::ast::CLASS_PROLOGUE_FN {
                        decisions.push((
                            blk_idx,
                            site_idx,
                            InlineDecision::Skip(SkipReason::LinkJudgedUnit),
                        ));
                        continue;
                    }
                    if module.class_drop_fns.contains(callee_id) {
                        decisions.push((
                            blk_idx,
                            site_idx,
                            InlineDecision::Skip(SkipReason::ClassDropUnit),
                        ));
                        continue;
                    }
                    if callee_ix == caller_idx && budget.max_recursion_depth == 0 {
                        decisions.push((
                            blk_idx,
                            site_idx,
                            InlineDecision::Skip(SkipReason::Recursion),
                        ));
                        continue;
                    }
                    let callee_cost = callee_costs[callee_ix];
                    if sole[callee_ix] && callee_cost <= budget.sole_site_ceiling {
                        decisions.push((blk_idx, site_idx, InlineDecision::Inline { callee_cost }));
                        continue;
                    }
                    if callee_cost > budget.callee_cost_ceiling {
                        decisions.push((
                            blk_idx,
                            site_idx,
                            InlineDecision::Skip(SkipReason::CalleeTooLarge),
                        ));
                        continue;
                    }
                    let projected = caller_spent.add(callee_cost);
                    if projected > budget.caller_total_budget {
                        decisions.push((
                            blk_idx,
                            site_idx,
                            InlineDecision::Skip(SkipReason::CallerBudgetExhausted),
                        ));
                        continue;
                    }
                    caller_spent = projected;
                    decisions.push((blk_idx, site_idx, InlineDecision::Inline { callee_cost }));
                }
                InstKind::CallIndirect(_, _, _) if !self_only => {
                    decisions.push((
                        blk_idx,
                        site_idx,
                        InlineDecision::Skip(SkipReason::Indirect),
                    ));
                }
                _ => {}
            }
        }
    }
    decisions
}

/// Run the inliner pass over `module` with the default `InlineBudget`.
/// Phase 1.0b: emits via `splice::inline_single_block_leaf` for every
/// would-inline candidate that also satisfies the splice's narrow
/// scope (single-block leaf + Ret terminator). The `TORAJS_INLINER_OFF
/// =1` env-gate short-circuits to dry-run: classification + stats still
/// happen but no `Module::funcs` mutation occurs.
pub fn inline_module(module: &mut Module) -> InlinerStats {
    inline_module_with_budget(module, InlineBudget::default())
}

/// `inline_module` with a caller-supplied budget. Used in tests to
/// exercise individual skip-reason buckets, budget exhaustion
/// boundaries, and the splice emit path.
///
/// Algorithm:
///
/// 1. Pre-compute every function's body cost — `callee_body_cost` is
///    O(insts) and we'd otherwise re-scan once per caller × call site.
/// 2. For each caller in bottom-up call-graph order (callees first —
///    `order.rs`; its cost is re-priced once its own sites are done),
///    classify each call site against
///    the budget. The classifier yields `(blk, site, decision)`
///    triples; we tally stats from the decisions (this is the Phase 0
///    behaviour, untouched).
/// 3. Unless dry-run is requested, collect every `InlineDecision::
///    Inline` triple, clone the callee body (Rust borrow-check
///    requirement: `&module.funcs[callee]` cannot coexist with
///    `&mut module.funcs[caller]`), and dispatch to
///    `inline_single_block_leaf`. Splice errors are silent — they mean
///    the candidate cleared the cost-benefit filter but not the Phase 1
///    splice-shape filter (e.g. multi-block callee); the gap
///    `would_inline - inlined` exposes them to the bench attribution
///    layer.
/// 4. Sites are spliced in reverse `(blk, site)` order so the index
///    coordinates the classifier captured remain valid throughout the
///    emit pass.
pub fn inline_module_with_budget(module: &mut Module, budget: InlineBudget) -> InlinerStats {
    let dry_run = std::env::var("TORAJS_INLINER_OFF").as_deref() == Ok("1");
    let mut callee_costs: Vec<Cost> = module.funcs.iter().map(callee_body_cost).collect();
    let mut stats = InlinerStats::default();
    let sole = sole_site::sole_site_callees(module);
    // inlining never moves a `Suspend` into a function, so this holds
    // for the whole pass
    let parks: Vec<bool> = module.funcs.iter().map(suspends).collect();
    for caller_idx in order::bottom_up(module) {
        let mut self_snapshot = None;
        for round in 0..budget.max_recursion_depth.max(1) {
            // rounds past the first splice the original body into the
            // self-sites the previous round's clones carry
            if round > 0 && self_snapshot.is_none() {
                break;
            }
            let decisions = classify_caller_sites(
                module,
                caller_idx,
                &budget,
                &callee_costs,
                &sole,
                &parks,
                round > 0,
            );
            stats.tally(&decisions);
            if dry_run {
                break;
            }
            emit::emit_caller(
                module,
                caller_idx,
                &decisions,
                &mut self_snapshot,
                &budget,
                &sole,
                &mut stats,
            );
        }
        // its callers, visited later, price the body they will clone
        callee_costs[caller_idx] = callee_body_cost(&module.funcs[caller_idx]);
    }
    stats
}

#[cfg(test)]
mod tests;
