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
//! * Report decisions via `InlinerStats`. The `inline_module` driver
//!   observes the module by shared reference; nothing is mutated.
//!
//! Phase 1.0a (shipped) — splice mutation API in `splice` sub-module:
//!
//! * `SpliceError` + `inline_single_block_leaf` materialise a single-
//!   block leaf callee into the caller block, removing the matching
//!   call instruction. Fresh `ValueId`s for non-parameter callee
//!   values; callee parameters substituted by caller-supplied `args`.
//! * Not wired into `transform_module` in Phase 1.0a — exercised by
//!   unit tests only; the production pipeline remains identity-on-IR
//!   until Phase 1.0b wires the driver.
//!
//! Future phases — Phase 1.0b wires the driver into the production
//! pipeline behind a `TORAJS_INLINER_OFF` env-gate; Phase 2 generalises
//! beyond single-block leaves (block-split + Φ-node insertion) and
//! threads `LoopAnalysis` so callee cost is depth-weighted (matching
//! the elaborator's existing LICM weighting in `cost::scale_for_depth`).

mod splice;

use crate::cost::{Cost, cost_of_kind};
use torajs_core::ssa::{Function, InstKind, Module};

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
    /// Sites that passed every check. Phase 0: this is a *would-have-
    /// inlined* count — no SSA emit happens, so the output module is
    /// unchanged regardless of this value.
    pub would_inline: u32,
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
/// against `budget`, returning per-site decisions in source order.
/// `callee_costs[i]` must equal `callee_body_cost(&module.funcs[i])`
/// (precomputed by `inline_module_with_budget` so callee bodies are
/// scanned exactly once per pass).
///
/// `CallIndirect` sites land in the returned `Vec` as
/// `Skip(Indirect)` so the caller can count them toward
/// `InlinerStats::skipped_indirect` without re-scanning.
fn classify_caller_sites(
    module: &Module,
    caller_idx: usize,
    budget: &InlineBudget,
    callee_costs: &[Cost],
) -> Vec<InlineDecision> {
    let caller = &module.funcs[caller_idx];
    let mut decisions = Vec::new();
    let mut caller_spent = Cost::ZERO;
    for blk in &caller.blocks {
        for inst in &blk.insts {
            match &inst.kind {
                InstKind::Call(callee_id, _args) => {
                    let callee_ix = callee_id.0 as usize;
                    let callee = &module.funcs[callee_ix];
                    if callee.is_declaration() {
                        decisions.push(InlineDecision::Skip(SkipReason::CalleeIsDeclaration));
                        continue;
                    }
                    if callee_ix == caller_idx && budget.max_recursion_depth == 0 {
                        decisions.push(InlineDecision::Skip(SkipReason::Recursion));
                        continue;
                    }
                    let callee_cost = callee_costs[callee_ix];
                    if callee_cost > budget.callee_cost_ceiling {
                        decisions.push(InlineDecision::Skip(SkipReason::CalleeTooLarge));
                        continue;
                    }
                    let projected = caller_spent.add(callee_cost);
                    if projected > budget.caller_total_budget {
                        decisions.push(InlineDecision::Skip(SkipReason::CallerBudgetExhausted));
                        continue;
                    }
                    caller_spent = projected;
                    decisions.push(InlineDecision::Inline { callee_cost });
                }
                InstKind::CallIndirect(_, _, _) => {
                    decisions.push(InlineDecision::Skip(SkipReason::Indirect));
                }
                _ => {}
            }
        }
    }
    decisions
}

/// Run the inliner pass over `module` with the default `InlineBudget`.
/// Phase 0 returns aggregate statistics; the module itself is observed
/// only by shared reference and its SSA representation is unchanged
/// after the call. Phase 1.0b switches the signature to `&mut Module`
/// and starts splicing callee blocks into approved call sites via
/// `splice::inline_single_block_leaf`.
pub fn inline_module(module: &Module) -> InlinerStats {
    inline_module_with_budget(module, InlineBudget::default())
}

/// `inline_module` with a caller-supplied budget. Used in tests to
/// exercise individual skip-reason buckets and budget exhaustion
/// boundaries.
pub fn inline_module_with_budget(module: &Module, budget: InlineBudget) -> InlinerStats {
    let callee_costs: Vec<Cost> = module.funcs.iter().map(callee_body_cost).collect();
    let mut stats = InlinerStats::default();
    let n = module.funcs.len();
    for caller_idx in 0..n {
        let decisions = classify_caller_sites(module, caller_idx, &budget, &callee_costs);
        for dec in decisions {
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
        let m = Module::default();
        let stats = inline_module(&m);
        assert_eq!(stats, InlinerStats::default());
    }

    #[test]
    fn caller_with_no_calls_yields_zero_candidates() {
        let m = module_of(vec![caller("main", vec![])]);
        let stats = inline_module(&m);
        assert_eq!(stats.candidates, 0);
        assert_eq!(stats.would_inline, 0);
    }

    #[test]
    fn small_leaf_call_is_a_would_inline_candidate() {
        let leaf = alu_body("leaf", 3);
        let main = caller("main", vec![void_inst(InstKind::Call(FuncId(1), vec![]))]);
        let m = module_of(vec![main, leaf]);
        let stats = inline_module(&m);
        assert_eq!(stats.candidates, 1);
        assert_eq!(stats.would_inline, 1);
        assert_eq!(stats.skipped_callee_too_large, 0);
    }

    #[test]
    fn oversized_leaf_is_rejected_with_callee_too_large() {
        let leaf = alu_body("fat", 300);
        let main = caller("main", vec![void_inst(InstKind::Call(FuncId(1), vec![]))]);
        let m = module_of(vec![main, leaf]);
        let stats = inline_module(&m);
        assert_eq!(stats.candidates, 1);
        assert_eq!(stats.would_inline, 0);
        assert_eq!(stats.skipped_callee_too_large, 1);
    }

    #[test]
    fn declaration_callee_is_skipped() {
        let leaf = declaration("extern_intrinsic");
        let main = caller("main", vec![void_inst(InstKind::Call(FuncId(1), vec![]))]);
        let m = module_of(vec![main, leaf]);
        let stats = inline_module(&m);
        assert_eq!(stats.candidates, 1);
        assert_eq!(stats.would_inline, 0);
        assert_eq!(stats.skipped_declaration, 1);
    }

    #[test]
    fn self_recursive_call_is_rejected_at_default_depth_zero() {
        let main = caller("main", vec![void_inst(InstKind::Call(FuncId(0), vec![]))]);
        let m = module_of(vec![main]);
        let stats = inline_module(&m);
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
        let m = module_of(vec![main]);
        let stats = inline_module(&m);
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
        let m = module_of(vec![main, leaf]);
        let stats = inline_module_with_budget(&m, budget);
        assert_eq!(stats.candidates, 3);
        assert_eq!(stats.would_inline, 2);
        assert_eq!(stats.skipped_caller_budget, 1);
    }

    #[test]
    fn module_is_not_mutated_in_phase_0() {
        let leaf = alu_body("leaf", 4);
        let main = caller("main", vec![void_inst(InstKind::Call(FuncId(1), vec![]))]);
        let m_before = module_of(vec![main.clone(), leaf.clone()]);
        let m_after = m_before.clone();
        let _ = inline_module(&m_after);
        assert_eq!(m_after.funcs.len(), m_before.funcs.len());
        for (a, b) in m_after.funcs.iter().zip(m_before.funcs.iter()) {
            assert_eq!(a.name, b.name);
            assert_eq!(a.blocks.len(), b.blocks.len());
            for (ba, bb) in a.blocks.iter().zip(b.blocks.iter()) {
                assert_eq!(ba.insts.len(), bb.insts.len());
            }
        }
    }
}
