//! The inliner's per-run counters, split from `mod.rs` at the
//! file-size line.

use super::{InlineDecision, SkipReason};

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
    /// successfully spliced into the caller body. Sum of
    /// `inlined_single_block` and `inlined_multi_block`. Always ≤
    /// `would_inline`. In dry-run mode (`TORAJS_INLINER_OFF=1`) this
    /// stays at 0 regardless of `would_inline`.
    pub inlined: u32,
    /// Subset of `inlined` that took the Phase 1.0a single-block-leaf
    /// fast path via `splice::inline_single_block_leaf`.
    pub inlined_single_block: u32,
    /// Subset of `inlined` that fell through to Phase 2.0a multi-block
    /// leaf via `splice2::inline_multi_block_leaf`. Real production
    /// code tends to land here because `ssa_lower` emits multi-block
    /// functions even for the smallest leaf bodies.
    pub inlined_multi_block: u32,
    /// Sites rejected by `SkipReason::CalleeTooLarge`.
    pub skipped_callee_too_large: u32,
    /// Sites rejected by `SkipReason::CallerBudgetExhausted`.
    pub skipped_caller_budget: u32,
    /// Sites rejected by `SkipReason::CalleeIsDeclaration`.
    pub skipped_declaration: u32,
    /// Sites rejected by `SkipReason::Recursion`.
    pub skipped_recursion: u32,
    /// Sites rejected by `SkipReason::LinkJudgedUnit`.
    pub skipped_link_judged_unit: u32,
    /// Sites rejected by `SkipReason::ClassDropUnit`.
    pub skipped_class_drop_unit: u32,
    /// Sites rejected by `SkipReason::Suspends`.
    pub skipped_suspends: u32,
    /// Indirect call sites observed. Tracked separately from
    /// `candidates` because at Phase 0 they are categorically out of
    /// scope rather than a per-site cost-benefit miss.
    pub skipped_indirect: u32,
    /// Sites approved by the classifier but skipped at emit because
    /// the callee's live body cost exceeded the ceiling by then — an
    /// earlier splice in the same pass (self-inline, or a callee
    /// processed earlier as a caller) grew it past the precomputed
    /// cost the classifier saw. Phase 2.1 stale-cost guard.
    pub skipped_live_cost: u32,
    /// Sole-site user functions whose one call was spliced: the
    /// out-of-line body is dropped (the fn becomes a declaration
    /// nothing references), so the dump and the per-fn stats show
    /// the live copy only and codegen skips the dead one.
    pub sole_bodies_dropped: u32,
}

impl InlinerStats {
    /// Count one classifier run's decisions.
    pub(super) fn tally(&mut self, decisions: &[(usize, usize, InlineDecision)]) {
        for (_, _, dec) in decisions {
            match dec {
                InlineDecision::Inline { .. } => {
                    self.candidates += 1;
                    self.would_inline += 1;
                }
                InlineDecision::Skip(reason) => match reason {
                    SkipReason::CalleeTooLarge => {
                        self.candidates += 1;
                        self.skipped_callee_too_large += 1;
                    }
                    SkipReason::CallerBudgetExhausted => {
                        self.candidates += 1;
                        self.skipped_caller_budget += 1;
                    }
                    SkipReason::CalleeIsDeclaration => {
                        self.candidates += 1;
                        self.skipped_declaration += 1;
                    }
                    SkipReason::Recursion => {
                        self.candidates += 1;
                        self.skipped_recursion += 1;
                    }
                    SkipReason::Indirect => {
                        self.skipped_indirect += 1;
                    }
                    SkipReason::LinkJudgedUnit => {
                        self.candidates += 1;
                        self.skipped_link_judged_unit += 1;
                    }
                    SkipReason::ClassDropUnit => {
                        self.candidates += 1;
                        self.skipped_class_drop_unit += 1;
                    }
                    SkipReason::Suspends => {
                        self.candidates += 1;
                        self.skipped_suspends += 1;
                    }
                },
            }
        }
    }
}
