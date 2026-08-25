//! ISLE-inspired rule engine — directed simplification rewrites that
//! grow each e-class with cheaper alternatives, scored by `cost.rs`
//! and selected by elaboration.
//!
//! Architecture (per cfallin 2026-04 "acyclic e-graph"):
//!
//! - Rules are **directed** — LHS is the existing pattern, RHS is the
//!   strict improvement. The only "looking like" associativity /
//!   commutativity rule is `CommutativeCanon` (chunk 10a), which is
//!   strictly single-shot (self-disarms once the const is on the
//!   RHS) and exists to underwrite single-side identity matches.
//!   No equality-saturation rules.
//! - Rules fire **eagerly** during `optimize.rs`'s walk, not via
//!   equality saturation. Empirically (cfallin §2) eager rewrites
//!   miss only ~2 in 4M nodes — cheap insurance.
//! - Recursion is bounded at `REWRITE_LIMIT` (5, per `egraph.rs`).
//!
//! Module layout (Phase 2 chunk 11a split):
//!
//! - `canon`              — `CommutativeCanon` (must run first).
//! - `arith_identity`     — `AddZero` / `MulOne` / `MulZero` /
//!                          `SubZero`.
//! - `bitwise_identity`   — `XorZero` / `OrZero` / `AndAllOnes` /
//!                          `AndZero` / `OrAllOnes`.
//! - `self_fold`          — `SubSelf` / `XorSelf`.
//! - `strength_reduction` — `MulPow2ToShl`.
//!
//! This module hosts the `Rewrite` trait, the `apply_rewrites*` free
//! fns, the `RewriteRegistry`, and the `BUILTIN_RULES` table that
//! orders the rules into the registry's default lineup.

use crate::egraph::REWRITE_LIMIT;
use torajs_core::ssa::InstKind;

pub mod arith_identity;
pub mod bitwise_identity;
pub mod canon;
pub mod const_fold;
pub mod self_fold;
pub mod strength_reduction;

pub use arith_identity::{AddZero, MulOne, MulZero, SubNegate, SubZero};
pub use bitwise_identity::{AndAllOnes, AndZero, OrAllOnes, OrZero, XorZero};
pub use canon::CommutativeCanon;
pub use const_fold::BinOpConstFold;
pub use self_fold::{SubSelf, XorSelf};
pub use strength_reduction::{FMulTwoToAdd, MulPow2ToShl};

use arith_identity::{ADD_ZERO, MUL_ONE, MUL_ZERO, SUB_NEGATE, SUB_ZERO};
use bitwise_identity::{AND_ALL_ONES, AND_ZERO, OR_ALL_ONES, OR_ZERO, XOR_ZERO};
use canon::COMMUTATIVE_CANON;
use const_fold::BIN_OP_CONST_FOLD;
use self_fold::{SUB_SELF, XOR_SELF};
use strength_reduction::{FMUL_TWO, MUL_POW2};

/// One directed simplification rule. `try_apply` returns `Some(rhs)`
/// when the rule matches `lhs` and produces a strictly better (per
/// `cost.rs`) replacement. The egraph layer unions `lhs.result` with
/// a fresh ValueId computing `rhs` and records `rhs` as the
/// `opt_value` representative.
///
/// Returning `None` means "rule doesn't apply" — no change.
pub trait Rewrite: Sync {
    /// Human-readable name (used in diagnostics + `RewriteRegistry`
    /// indexing).
    fn name(&self) -> &'static str;

    /// Match `lhs`. If the pattern fires, return the replacement
    /// `InstKind`. Pure function — no egraph mutation here; the
    /// optimizer wraps the return.
    fn try_apply(&self, lhs: &InstKind) -> Option<InstKind>;
}

/// Apply every rule in `rules` against `lhs` once. Returns the first
/// match's RHS (rules are listed in priority order). Caller is
/// responsible for the recursion budget — typically tracks depth and
/// stops at `REWRITE_LIMIT` per Cranelift's `egraph.rs`.
pub fn apply_rewrites(rules: &[&dyn Rewrite], lhs: &InstKind) -> Option<InstKind> {
    for rule in rules {
        if let Some(rhs) = rule.try_apply(lhs) {
            return Some(rhs);
        }
    }
    None
}

/// Recursive variant — fires the first matching rule, then re-runs
/// the registry against the RHS, up to `REWRITE_LIMIT` levels deep.
/// Returns the final canonicalised form. If no rule fires at any
/// level, returns `lhs.clone()` (caller can compare to detect "no
/// rewrite happened").
pub fn apply_rewrites_recursive(rules: &[&dyn Rewrite], lhs: &InstKind) -> InstKind {
    let mut current = lhs.clone();
    let mut depth: u8 = 0;
    while depth < REWRITE_LIMIT {
        match apply_rewrites(rules, &current) {
            Some(next) => {
                current = next;
                depth += 1;
            }
            None => break,
        }
    }
    current
}

// ---- registry -------------------------------------------------------------

/// Named rule list. The optimize walk gets a `&RewriteRegistry` and
/// asks it for the currently-active rules. Future: per-cluster rule
/// groups (arithmetic / branch / phi / inline) selectable by flag.
pub struct RewriteRegistry {
    rules: Vec<&'static dyn Rewrite>,
}

impl Default for RewriteRegistry {
    fn default() -> Self {
        Self::with_builtins()
    }
}

impl RewriteRegistry {
    /// Empty registry — for tests that want to verify the walk
    /// behaves correctly with no rules at all.
    pub fn empty() -> Self {
        Self { rules: Vec::new() }
    }

    /// Registry pre-loaded with `BUILTIN_RULES`. The default integration
    /// (cmd_build_new wiring in Phase 1) uses this.
    pub fn with_builtins() -> Self {
        Self {
            rules: BUILTIN_RULES.to_vec(),
        }
    }

    /// All currently-active rules, in priority order.
    pub fn rules(&self) -> &[&'static dyn Rewrite] {
        &self.rules
    }

    /// Append a custom rule. Used by tests + future rule clusters.
    pub fn push(&mut self, rule: &'static dyn Rewrite) {
        self.rules.push(rule);
    }
}

/// Built-in rule set. Order = priority — first match wins in
/// `apply_rewrites`. Identity / const-fold rules fire first
/// (zero-cost alias / literal propagation); strength-reduction
/// rules fire on what's left.
///
/// - Phase 0 shipped MulPow2ToShl.
/// - Phase 1 chunk 9a-2 added AddZero + MulOne.
/// - Phase 1 chunk 9a-3 added SubSelf + XorSelf + MulZero (const-fold
///   via the eclass `value_to_const` map; no `InstKind::Const`
///   variant needed).
/// - Phase 2 chunk 10a prepends CommutativeCanon so identity /
///   const-fold rules see normalised operand order (const on RHS).
/// - Phase 2 chunk 10b simplifies AddZero / MulOne / MulZero to
///   match the RHS-Const arm only, relying on the canon invariant
///   established in chunk 10a.
/// - Phase 2 chunk 10c adds SubZero / XorZero / OrZero / AndAllOnes
///   / AndZero / OrAllOnes — RHS-Const identity / annihilator rules
///   for Sub / Xor / Or / And, completing the textbook arithmetic +
///   bitwise identity coverage. All six rely on the canon invariant
///   (except SubZero which is RHS-only by SSA emission shape).
/// - Phase 2 chunk 11b adds SubNegate (`sub 0 x → Neg(x)`), the
///   sister of SubZero handling the LHS-zero arm. Order in the
///   list doesn't matter w.r.t. SubZero (patterns are disjoint).
/// - Phase 2 chunk 11c adds BinOpConstFold (`(op ConstI64 ConstI64)
///   → Identity(ConstI64(folded))`) covering all 11 integer BinOps.
///   Placed before strength-reduction; pattern is `(Const, Const)` —
///   disjoint with identity/annihilator (`Value, Const`) and self-fold
///   (`Value, same Value`), so order is correctness-neutral.
pub static BUILTIN_RULES: &[&'static dyn Rewrite] = &[
    &COMMUTATIVE_CANON,
    &ADD_ZERO,
    &MUL_ONE,
    &MUL_ZERO,
    &SUB_ZERO,
    &SUB_NEGATE,
    &XOR_ZERO,
    &OR_ZERO,
    &AND_ALL_ONES,
    &AND_ZERO,
    &OR_ALL_ONES,
    &SUB_SELF,
    &XOR_SELF,
    &BIN_OP_CONST_FOLD,
    &MUL_POW2,
    &FMUL_TWO,
];

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{BinOp, Operand, ValueId};

    fn val(n: u32) -> Operand {
        Operand::Value(ValueId(n))
    }

    #[test]
    fn registry_iteration_finds_match() {
        let reg = RewriteRegistry::with_builtins();
        // mul 8 → shl 3 via MulPow2ToShl.
        let lhs = InstKind::BinOp(BinOp::Mul, val(4), Operand::ConstI64(8));
        let rhs = apply_rewrites(reg.rules(), &lhs).expect("MulPow2 must fire");
        assert_eq!(
            rhs,
            InstKind::BinOp(BinOp::Shl, val(4), Operand::ConstI64(3))
        );
        // Non-matching pattern (mul by 3): no rule fires.
        let lhs2 = InstKind::BinOp(BinOp::Mul, val(4), Operand::ConstI64(3));
        assert!(apply_rewrites(reg.rules(), &lhs2).is_none());
        // Empty registry returns None even on a normally-matching pattern.
        let empty = RewriteRegistry::empty();
        assert!(apply_rewrites(empty.rules(), &lhs).is_none());
    }

    #[test]
    fn apply_rewrites_recursive_terminates_at_fixed_point() {
        let reg = RewriteRegistry::with_builtins();
        // mul %v 8 → shl %v 3 → no further rewrite (shl is the fixed
        // point under the current ruleset).
        let lhs = InstKind::BinOp(BinOp::Mul, val(2), Operand::ConstI64(8));
        let final_form = apply_rewrites_recursive(reg.rules(), &lhs);
        assert_eq!(
            final_form,
            InstKind::BinOp(BinOp::Shl, val(2), Operand::ConstI64(3))
        );
    }

    #[test]
    fn recursion_bounded_by_rewrite_limit() {
        // Pathological rule that always rewrites to itself with a
        // ValueId bump — verifies the depth cap exits cleanly.
        struct InfiniteBump;
        impl Rewrite for InfiniteBump {
            fn name(&self) -> &'static str {
                "infinite_bump"
            }
            fn try_apply(&self, lhs: &InstKind) -> Option<InstKind> {
                if let InstKind::BinOp(op, Operand::Value(v), r) = lhs {
                    Some(InstKind::BinOp(*op, Operand::Value(ValueId(v.0 + 1)), *r))
                } else {
                    None
                }
            }
        }
        static BUMP: InfiniteBump = InfiniteBump;
        let rules: Vec<&'static dyn Rewrite> = vec![&BUMP];
        let lhs = InstKind::BinOp(BinOp::Add, val(0), Operand::ConstI64(1));
        let result = apply_rewrites_recursive(&rules, &lhs);
        // After REWRITE_LIMIT (5) firings the value id reaches 5.
        assert_eq!(
            result,
            InstKind::BinOp(BinOp::Add, val(REWRITE_LIMIT as u32), Operand::ConstI64(1))
        );
    }

    #[test]
    fn builtin_registry_includes_add_zero_and_mul_one() {
        let reg = RewriteRegistry::with_builtins();
        // add 0 fires through the registry.
        let add = InstKind::BinOp(BinOp::Add, val(4), Operand::ConstI64(0));
        assert_eq!(
            apply_rewrites(reg.rules(), &add),
            Some(InstKind::Identity(Operand::Value(ValueId(4))))
        );
        // mul 1 fires through the registry.
        let mul = InstKind::BinOp(BinOp::Mul, val(4), Operand::ConstI64(1));
        assert_eq!(
            apply_rewrites(reg.rules(), &mul),
            Some(InstKind::Identity(Operand::Value(ValueId(4))))
        );
    }

    #[test]
    fn custom_rule_via_push() {
        // Verify the registry accepts a Phase 1 rule plugin via
        // `push` (used by future cluster wiring + tests).
        struct AddSwap;
        impl Rewrite for AddSwap {
            fn name(&self) -> &'static str {
                "add_swap_canonical"
            }
            fn try_apply(&self, lhs: &InstKind) -> Option<InstKind> {
                // Canonicalise add so constants sit on the right.
                if let InstKind::BinOp(BinOp::Add, l @ Operand::ConstI64(_), r) = lhs
                    && !matches!(r, Operand::ConstI64(_))
                {
                    return Some(InstKind::BinOp(BinOp::Add, *r, *l));
                }
                None
            }
        }
        static SWAP: AddSwap = AddSwap;
        let mut reg = RewriteRegistry::empty();
        reg.push(&SWAP);
        let lhs = InstKind::BinOp(BinOp::Add, Operand::ConstI64(7), val(9));
        let rhs = apply_rewrites(reg.rules(), &lhs).expect("AddSwap must fire");
        assert_eq!(
            rhs,
            InstKind::BinOp(BinOp::Add, val(9), Operand::ConstI64(7))
        );
    }

    #[test]
    fn commutative_canon_chains_into_add_zero_through_registry() {
        // `add 0 %v` — CommutativeCanon swaps to `add %v 0`, AddZero
        // then fires and produces `Identity(%v)`. Verifies rule
        // ordering (CommutativeCanon first) + chain.
        let reg = RewriteRegistry::with_builtins();
        let lhs = InstKind::BinOp(BinOp::Add, Operand::ConstI64(0), val(4));
        let out = apply_rewrites_recursive(reg.rules(), &lhs);
        assert_eq!(out, InstKind::Identity(Operand::Value(ValueId(4))));
    }

    #[test]
    fn commutative_canon_chains_into_mul_zero_through_registry() {
        // `mul 0 %v` — CommutativeCanon swaps to `mul %v 0`, MulZero
        // then fires and produces `Identity(ConstI64(0))`.
        let reg = RewriteRegistry::with_builtins();
        let lhs = InstKind::BinOp(BinOp::Mul, Operand::ConstI64(0), val(4));
        let out = apply_rewrites_recursive(reg.rules(), &lhs);
        assert_eq!(out, InstKind::Identity(Operand::ConstI64(0)));
    }

    #[test]
    fn commutative_canon_runs_before_other_rules_in_builtin_order() {
        // First rule in BUILTIN_RULES must be CommutativeCanon — its
        // priority underwrites the single-side simplification of
        // Phase 2+ identity rules.
        let reg = RewriteRegistry::with_builtins();
        assert_eq!(
            reg.rules().first().expect("non-empty registry").name(),
            "commutative_canon"
        );
    }

    #[test]
    fn chunk_10c_rules_chain_through_registry_with_canon() {
        let reg = RewriteRegistry::with_builtins();
        // `xor 0 %v` — canon swaps → `xor %v 0` → XorZero → Identity(%v).
        let xor = InstKind::BinOp(BinOp::Xor, Operand::ConstI64(0), val(4));
        assert_eq!(
            apply_rewrites_recursive(reg.rules(), &xor),
            InstKind::Identity(Operand::Value(ValueId(4)))
        );
        // `or 0 %v` — canon swaps → `or %v 0` → OrZero → Identity(%v).
        let or_ = InstKind::BinOp(BinOp::Or, Operand::ConstI64(0), val(4));
        assert_eq!(
            apply_rewrites_recursive(reg.rules(), &or_),
            InstKind::Identity(Operand::Value(ValueId(4)))
        );
        // `and -1 %v` — canon swaps → `and %v -1` → AndAllOnes → Identity(%v).
        let and_ = InstKind::BinOp(BinOp::And, Operand::ConstI64(-1), val(4));
        assert_eq!(
            apply_rewrites_recursive(reg.rules(), &and_),
            InstKind::Identity(Operand::Value(ValueId(4)))
        );
        // `and 0 %v` — canon swaps → `and %v 0` → AndZero → Identity(0).
        let and_z = InstKind::BinOp(BinOp::And, Operand::ConstI64(0), val(4));
        assert_eq!(
            apply_rewrites_recursive(reg.rules(), &and_z),
            InstKind::Identity(Operand::ConstI64(0))
        );
        // `or -1 %v` — canon swaps → `or %v -1` → OrAllOnes → Identity(-1).
        let or_ones = InstKind::BinOp(BinOp::Or, Operand::ConstI64(-1), val(4));
        assert_eq!(
            apply_rewrites_recursive(reg.rules(), &or_ones),
            InstKind::Identity(Operand::ConstI64(-1))
        );
        // `sub %v 0` — SubZero direct (no canon needed, Sub non-commutative).
        let sub_z = InstKind::BinOp(BinOp::Sub, val(4), Operand::ConstI64(0));
        assert_eq!(
            apply_rewrites_recursive(reg.rules(), &sub_z),
            InstKind::Identity(Operand::Value(ValueId(4)))
        );
    }

    #[test]
    fn binop_const_fold_fires_through_registry() {
        // `add 3 5` — both const, neither identity nor self-fold can
        // fire; BinOpConstFold catches it directly. Canon is a no-op
        // when both operands are const.
        let reg = RewriteRegistry::with_builtins();
        let add = InstKind::BinOp(BinOp::Add, Operand::ConstI64(3), Operand::ConstI64(5));
        assert_eq!(
            apply_rewrites_recursive(reg.rules(), &add),
            InstKind::Identity(Operand::ConstI64(8))
        );
        // `sdiv 20 4` → Identity(5) — signed division const-fold.
        let sdiv = InstKind::BinOp(BinOp::SDiv, Operand::ConstI64(20), Operand::ConstI64(4));
        assert_eq!(
            apply_rewrites_recursive(reg.rules(), &sdiv),
            InstKind::Identity(Operand::ConstI64(5))
        );
        // `shl 1 8` → Identity(256) — shift in-range const-fold.
        let shl = InstKind::BinOp(BinOp::Shl, Operand::ConstI64(1), Operand::ConstI64(8));
        assert_eq!(
            apply_rewrites_recursive(reg.rules(), &shl),
            InstKind::Identity(Operand::ConstI64(256))
        );
        // `sdiv 5 0` — bail; no rule should fire, recursive walk
        // returns the original.
        let sdiv0 = InstKind::BinOp(BinOp::SDiv, Operand::ConstI64(5), Operand::ConstI64(0));
        assert_eq!(apply_rewrites_recursive(reg.rules(), &sdiv0), sdiv0);
        // `add 0 0` → Identity(0) — even the degenerate all-zero case
        // const-folds (AddZero would fire if RHS were Value, but both
        // const goes through const-fold instead).
        let zz = InstKind::BinOp(BinOp::Add, Operand::ConstI64(0), Operand::ConstI64(0));
        assert_eq!(
            apply_rewrites_recursive(reg.rules(), &zz),
            InstKind::Identity(Operand::ConstI64(0))
        );
    }

    #[test]
    fn sub_negate_fires_through_registry_lhs_zero_only() {
        // `sub 0 %v` → Neg(%v) — chunk 11b SubNegate rule. Sub is
        // non-commutative so canon is a no-op here; SubNegate fires
        // directly on the LHS-zero pattern. Neg has no further
        // rewrite, so the recursive walk stops there.
        let reg = RewriteRegistry::with_builtins();
        let lhs = InstKind::BinOp(BinOp::Sub, Operand::ConstI64(0), val(7));
        let out = apply_rewrites_recursive(reg.rules(), &lhs);
        assert_eq!(out, InstKind::Neg(Operand::Value(ValueId(7))));
        // `sub %v 0` still goes through SubZero, NOT SubNegate.
        let rhs_z = InstKind::BinOp(BinOp::Sub, val(7), Operand::ConstI64(0));
        let out2 = apply_rewrites_recursive(reg.rules(), &rhs_z);
        assert_eq!(out2, InstKind::Identity(Operand::Value(ValueId(7))));
    }
}
