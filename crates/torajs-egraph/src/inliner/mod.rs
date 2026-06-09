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

mod splice;

use crate::cost::{Cost, cost_of_kind};
use torajs_core::ssa::{FuncId, Function, InstKind, Module, Operand, ValueId};

pub use splice::{SpliceError, inline_single_block_leaf};

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
    /// Maximum recursion depth allowed via inlining. Phase 0 default is
    /// `0` — direct self-recursive call sites are always rejected with
    /// `SkipReason::Recursion`. A non-zero value reserves API surface
    /// for Phase 2+ bounded recursive inlining; the call-graph cycle
    /// detector that pairs with it is not in this commit.
    pub max_recursion_depth: u32,
}

impl Default for InlineBudget {
    fn default() -> Self {
        Self {
            callee_cost_ceiling: Cost::new(225),
            caller_total_budget: Cost::new(675),
            max_recursion_depth: 0,
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
}

/// Aggregate diagnostic counters across one `inline_module` run.
/// Reported to the integration layer for logging and regression
/// tracking. Not used to gate correctness.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct InlinerStats {
    /// Direct call sites considered for inlining (`InstKind::Call(...)`
    /// occurrences in every caller body). `CallIndirect` sites are
    /// counted toward `skipped_indirect` and NOT toward `candidates`
    /// (an indirect site is not a candidate at Phase 0).
    pub candidates: u32,
    /// Sites that passed every check at the classifier (cost-benefit
    /// budget + recursion + indirect filters). A would-inline decision
    /// is necessary but not sufficient for actual emit: Phase 1's
    /// `splice` module further requires single-block leaf shape, so
    /// `inlined` ≤ `would_inline` at all times. Inspect the gap to
    /// attribute splice rejections (multi-block callee, non-Ret
    /// terminator, etc.).
    pub would_inline: u32,
    /// Sites that not only passed the classifier but were also
    /// successfully spliced into the caller body via
    /// `splice::inline_single_block_leaf`. Always ≤ `would_inline`.
    /// In dry-run mode (`TORAJS_INLINER_OFF=1`) this stays at 0
    /// regardless of `would_inline`.
    pub inlined: u32,
    /// Sites rejected by `SkipReason::CalleeTooLarge`.
    pub skipped_callee_too_large: u32,
    /// Sites rejected by `SkipReason::CallerBudgetExhausted`.
    pub skipped_caller_budget: u32,
    /// Sites rejected by `SkipReason::CalleeIsDeclaration`.
    pub skipped_declaration: u32,
    /// Sites rejected by `SkipReason::Recursion`.
    pub skipped_recursion: u32,
    /// Indirect call sites observed. Tracked separately from
    /// `candidates` because at Phase 0 they are categorically out of
    /// scope rather than a per-site cost-benefit miss.
    pub skipped_indirect: u32,
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
/// `InlinerStats::skipped_indirect` without re-scanning.
fn classify_caller_sites(
    module: &Module,
    caller_idx: usize,
    budget: &InlineBudget,
    callee_costs: &[Cost],
) -> Vec<(usize, usize, InlineDecision)> {
    let caller = &module.funcs[caller_idx];
    let mut decisions = Vec::new();
    let mut caller_spent = Cost::ZERO;
    for (blk_idx, blk) in caller.blocks.iter().enumerate() {
        for (site_idx, inst) in blk.insts.iter().enumerate() {
            match &inst.kind {
                InstKind::Call(callee_id, _args) => {
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
                    if callee_ix == caller_idx && budget.max_recursion_depth == 0 {
                        decisions.push((
                            blk_idx,
                            site_idx,
                            InlineDecision::Skip(SkipReason::Recursion),
                        ));
                        continue;
                    }
                    let callee_cost = callee_costs[callee_ix];
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
                InstKind::CallIndirect(_, _, _) => {
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
/// 2. For each caller in source order, classify each call site against
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
    let callee_costs: Vec<Cost> = module.funcs.iter().map(callee_body_cost).collect();
    let mut stats = InlinerStats::default();
    let n = module.funcs.len();
    for caller_idx in 0..n {
        let decisions = classify_caller_sites(module, caller_idx, &budget, &callee_costs);
        // Pass A — tally classifier stats. Reads decisions only.
        for (_, _, dec) in &decisions {
            match dec {
                InlineDecision::Inline { .. } => {
                    stats.candidates += 1;
                    stats.would_inline += 1;
                }
                InlineDecision::Skip(reason) => match reason {
                    SkipReason::CalleeTooLarge => {
                        stats.candidates += 1;
                        stats.skipped_callee_too_large += 1;
                    }
                    SkipReason::CallerBudgetExhausted => {
                        stats.candidates += 1;
                        stats.skipped_caller_budget += 1;
                    }
                    SkipReason::CalleeIsDeclaration => {
                        stats.candidates += 1;
                        stats.skipped_declaration += 1;
                    }
                    SkipReason::Recursion => {
                        stats.candidates += 1;
                        stats.skipped_recursion += 1;
                    }
                    SkipReason::Indirect => {
                        stats.skipped_indirect += 1;
                    }
                },
            }
        }
        if dry_run {
            continue;
        }
        // Pass B — collect Inline targets with full splice payload.
        let mut targets: Vec<(usize, usize, FuncId, Vec<Operand>, Option<ValueId>)> = Vec::new();
        for (blk_idx, site_idx, dec) in &decisions {
            if matches!(dec, InlineDecision::Inline { .. }) {
                let inst = &module.funcs[caller_idx].blocks[*blk_idx].insts[*site_idx];
                if let InstKind::Call(callee_id, args) = &inst.kind {
                    targets.push((*blk_idx, *site_idx, *callee_id, args.clone(), inst.result));
                }
            }
        }
        // Reverse-sort so deeper sites splice first; lower-index
        // targets keep their captured `site_idx` valid.
        targets.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
        for (blk_idx, site_idx, callee_id, args, result_value) in targets {
            let callee_clone = module.funcs[callee_id.0 as usize].clone();
            let caller = &mut module.funcs[caller_idx];
            if inline_single_block_leaf(
                caller,
                blk_idx,
                site_idx,
                &callee_clone,
                &args,
                result_value,
            )
            .is_ok()
            {
                stats.inlined += 1;
            }
        }
    }
    stats
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{
        BinOp, Block, BlockId, FuncId, Function, Inst, InstKind, Module, Operand, SigId,
        Terminator, Type, ValueId, ValueInfo,
    };

    fn val_inst(result: ValueId, kind: InstKind) -> Inst {
        Inst {
            result: Some(result),
            kind,
            origin: None,
        }
    }

    fn void_inst(kind: InstKind) -> Inst {
        Inst {
            result: None,
            kind,
            origin: None,
        }
    }

    /// Build a function with `n_ops` trivial integer adds in one block
    /// — `ALU_COST` is 1 per add, so body cost equals `n_ops`.
    fn alu_body(name: &str, n_ops: u32) -> Function {
        let mut values = vec![
            ValueInfo {
                ty: Type::I64,
                name: None,
            };
            n_ops as usize
        ];
        values.push(ValueInfo {
            ty: Type::I64,
            name: None,
        });
        let insts = (0..n_ops)
            .map(|i| {
                val_inst(
                    ValueId(i),
                    InstKind::BinOp(BinOp::Add, Operand::ConstI64(1), Operand::ConstI64(1)),
                )
            })
            .collect();
        Function {
            name: name.into(),
            params: vec![],
            ret: Type::Void,
            blocks: vec![Block {
                id: BlockId(0),
                insts,
                term: Terminator::Ret(None),
            }],
            values,
            current_origin: None,
        }
    }

    fn declaration(name: &str) -> Function {
        Function {
            name: name.into(),
            params: vec![],
            ret: Type::Void,
            blocks: vec![],
            values: vec![],
            current_origin: None,
        }
    }

    fn caller(name: &str, insts: Vec<Inst>) -> Function {
        Function {
            name: name.into(),
            params: vec![],
            ret: Type::Void,
            blocks: vec![Block {
                id: BlockId(0),
                insts,
                term: Terminator::Ret(None),
            }],
            values: vec![],
            current_origin: None,
        }
    }

    fn module_of(funcs: Vec<Function>) -> Module {
        let mut m = Module::default();
        m.funcs = funcs;
        m
    }

    #[test]
    fn callee_body_cost_sums_alu_ops() {
        let f = alu_body("leaf", 5);
        assert_eq!(callee_body_cost(&f), Cost::new(5));
    }

    #[test]
    fn empty_module_returns_zero_stats() {
        let mut m = Module::default();
        let stats = inline_module(&mut m);
        assert_eq!(stats, InlinerStats::default());
    }

    #[test]
    fn caller_with_no_calls_yields_zero_candidates() {
        let mut m = module_of(vec![caller("main", vec![])]);
        let stats = inline_module(&mut m);
        assert_eq!(stats.candidates, 0);
        assert_eq!(stats.would_inline, 0);
    }

    #[test]
    fn small_leaf_call_is_a_would_inline_candidate() {
        let leaf = alu_body("leaf", 3);
        let main = caller("main", vec![void_inst(InstKind::Call(FuncId(1), vec![]))]);
        let mut m = module_of(vec![main, leaf]);
        let stats = inline_module(&mut m);
        assert_eq!(stats.candidates, 1);
        assert_eq!(stats.would_inline, 1);
        assert_eq!(stats.skipped_callee_too_large, 0);
    }

    #[test]
    fn oversized_leaf_is_rejected_with_callee_too_large() {
        let leaf = alu_body("fat", 300);
        let main = caller("main", vec![void_inst(InstKind::Call(FuncId(1), vec![]))]);
        let mut m = module_of(vec![main, leaf]);
        let stats = inline_module(&mut m);
        assert_eq!(stats.candidates, 1);
        assert_eq!(stats.would_inline, 0);
        assert_eq!(stats.skipped_callee_too_large, 1);
    }

    #[test]
    fn declaration_callee_is_skipped() {
        let leaf = declaration("extern_intrinsic");
        let main = caller("main", vec![void_inst(InstKind::Call(FuncId(1), vec![]))]);
        let mut m = module_of(vec![main, leaf]);
        let stats = inline_module(&mut m);
        assert_eq!(stats.candidates, 1);
        assert_eq!(stats.would_inline, 0);
        assert_eq!(stats.skipped_declaration, 1);
    }

    #[test]
    fn self_recursive_call_is_rejected_at_default_depth_zero() {
        let main = caller("main", vec![void_inst(InstKind::Call(FuncId(0), vec![]))]);
        let mut m = module_of(vec![main]);
        let stats = inline_module(&mut m);
        assert_eq!(stats.candidates, 1);
        assert_eq!(stats.skipped_recursion, 1);
        assert_eq!(stats.would_inline, 0);
    }

    #[test]
    fn indirect_call_is_skipped_without_consuming_candidate_slot() {
        let main = caller(
            "main",
            vec![void_inst(InstKind::CallIndirect(
                SigId(0),
                Operand::ConstI64(0),
                vec![],
            ))],
        );
        let mut m = module_of(vec![main]);
        let stats = inline_module(&mut m);
        assert_eq!(stats.candidates, 0);
        assert_eq!(stats.skipped_indirect, 1);
        assert_eq!(stats.would_inline, 0);
    }

    #[test]
    fn caller_budget_exhausts_after_repeated_inlines() {
        let leaf = alu_body("med", 100);
        let main = caller(
            "main",
            vec![
                void_inst(InstKind::Call(FuncId(1), vec![])),
                void_inst(InstKind::Call(FuncId(1), vec![])),
                void_inst(InstKind::Call(FuncId(1), vec![])),
            ],
        );
        let budget = InlineBudget {
            callee_cost_ceiling: Cost::new(200),
            caller_total_budget: Cost::new(250),
            max_recursion_depth: 0,
        };
        let mut m = module_of(vec![main, leaf]);
        let stats = inline_module_with_budget(&mut m, budget);
        assert_eq!(stats.candidates, 3);
        assert_eq!(stats.would_inline, 2);
        assert_eq!(stats.skipped_caller_budget, 1);
    }

    // ---- Phase 1.0b emit-side tests ----

    #[test]
    fn single_block_leaf_call_is_actually_inlined() {
        // Leaf: 4 ALU adds, void return. Caller: one void call site.
        // Expected after Phase 1.0b: main's block body grows from 1
        // (the call inst) to 4 (the 4 inlined add insts), and the
        // call instruction itself is gone.
        let leaf = alu_body("leaf", 4);
        let main = caller("main", vec![void_inst(InstKind::Call(FuncId(1), vec![]))]);
        let mut m = module_of(vec![main, leaf]);
        let stats = inline_module(&mut m);
        assert_eq!(stats.would_inline, 1);
        assert_eq!(stats.inlined, 1, "single-block leaf passes splice");
        assert_eq!(
            m.funcs[0].blocks[0].insts.len(),
            4,
            "call inst replaced by 4 leaf adds"
        );
        for inst in &m.funcs[0].blocks[0].insts {
            assert!(
                matches!(inst.kind, InstKind::BinOp(BinOp::Add, _, _)),
                "every inlined inst is the leaf Add"
            );
        }
    }

    #[test]
    fn multi_block_callee_passes_classifier_but_splice_fails() {
        // Multi-block leaf — cheap by cost (no body work), so the
        // classifier produces a would_inline decision. The splice then
        // rejects it with NotSingleBlock, so inlined stays 0 and the
        // would_inline minus inlined gap is the attribution surface.
        let multi = Function {
            name: "two_blocks".into(),
            params: vec![],
            ret: Type::Void,
            blocks: vec![
                Block {
                    id: BlockId(0),
                    insts: vec![],
                    term: Terminator::Br(BlockId(1)),
                },
                Block {
                    id: BlockId(1),
                    insts: vec![],
                    term: Terminator::Ret(None),
                },
            ],
            values: vec![],
            current_origin: None,
        };
        let main = caller("main", vec![void_inst(InstKind::Call(FuncId(1), vec![]))]);
        let mut m = module_of(vec![main, multi]);
        let before_inst_count = m.funcs[0].blocks[0].insts.len();
        let stats = inline_module(&mut m);
        assert_eq!(stats.would_inline, 1);
        assert_eq!(stats.inlined, 0, "splice fails on multi-block callee");
        assert_eq!(
            m.funcs[0].blocks[0].insts.len(),
            before_inst_count,
            "caller block unchanged when splice fails"
        );
    }

    #[test]
    fn dry_run_env_gate_skips_emit_but_classifies() {
        // TORAJS_INLINER_OFF=1 → stats still reflect classification,
        // but no module mutation happens (inlined stays 0). Mirrors
        // the existing TORAJS_EGRAPH_OFF gate pattern in lib.rs::tests.
        //
        // SAFETY: env mutation is process-global; we set+remove in a
        // controlled scope and rely on cargo nextest's per-test
        // process isolation (.claude/rules/torajs-autorun-pipeline.md
        // "test 判定" section).
        unsafe {
            std::env::set_var("TORAJS_INLINER_OFF", "1");
        }
        let leaf = alu_body("leaf", 4);
        let main = caller("main", vec![void_inst(InstKind::Call(FuncId(1), vec![]))]);
        let mut m = module_of(vec![main, leaf]);
        let before_inst_count = m.funcs[0].blocks[0].insts.len();
        let stats = inline_module(&mut m);
        unsafe {
            std::env::remove_var("TORAJS_INLINER_OFF");
        }
        assert_eq!(stats.would_inline, 1, "classification still happens");
        assert_eq!(stats.inlined, 0, "dry-run does not emit");
        assert_eq!(
            m.funcs[0].blocks[0].insts.len(),
            before_inst_count,
            "module unchanged in dry-run"
        );
    }
}
