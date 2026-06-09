//! ISLE-inspired rule engine — directed simplification rewrites that
//! grow each e-class with cheaper alternatives, scored by `cost.rs`
//! and selected by elaboration.
//!
//! Architecture (per cfallin 2026-04 "acyclic e-graph"):
//!
//! - Rules are **directed** — LHS is the existing pattern, RHS is the
//!   strict improvement. No associativity/commutativity rules (those
//!   explode the e-class without measurable wins; Cranelift's same
//!   stance after measuring on Sightglass).
//! - Rules fire **eagerly** during `optimize.rs`'s walk, not via
//!   equality saturation. Empirically (cfallin §2) eager rewrites
//!   miss only ~2 in 4M nodes — cheap insurance.
//! - Recursion is bounded at `REWRITE_LIMIT` (5, per `egraph.rs`).
//!
//! Phase 0 (this commit) ships:
//! 1. `Rewrite` trait — the per-rule API
//! 2. `apply_rewrites` + `apply_rewrites_recursive` — eager-fire with
//!    `REWRITE_LIMIT` recursion budget
//! 3. `RewriteRegistry` — named rule set with priority order
//! 4. `MulPow2ToShl` — first genuine rule (strength reduction
//!    `mul x 2^k → shl x k`), demonstrating the rule API end-to-end
//!
//! **Why only one builtin rule in Phase 0**: most arithmetic
//! identities (`add x 0 → x`, `mul x 1 → x`, `sub x x → 0`,
//! `xor x x → 0`, `mul x 0 → 0`) need to express "use this Operand
//! as the result directly". The current `InstKind` enum can't
//! represent that — all variants produce a fresh computation. The
//! Phase 0 + 1 boundary commit adds an `InstKind::Identity(Operand)`
//! variant + integration plumbing in `optimize.rs` to translate
//! `Identity(op)` into `egraph.set_opt_value(result, op_value)`.
//! Once that lands, the 5 placeholder rules become real rewrites.
//! `MulPow2ToShl` works in Phase 0 because it genuinely changes the
//! BinOp shape (Mul→Shl with new const operand).
//!
//! Phase 1 (next clusters) extends with: `InstKind::Identity` + the 5
//! deferred arithmetic rules, phi simplification, SimplifyCFG-style
//! branch folding, more arithmetic identities, Inlining trigger.

use crate::egraph::REWRITE_LIMIT;
use torajs_core::ssa::{BinOp, InstKind, Operand};

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

// ---- built-in rule: strength reduction (mul → shl) ------------------------
//
// This is the Phase 0 demo rule. Each `Rewrite` impl is a small
// struct so the registry can hold `&'static dyn Rewrite`. Match
// patterns are by-value on `InstKind` (Operand derives PartialEq +
// Hash; integer literals compare by exact value).

/// `(mul x 2^k) → (shl x k)` — strength reduction. Fires for
/// power-of-two right-hand constants only (left-hand version
/// `(mul 2^k x)` first canonicalises through a `mul_by_const_swap`
/// future Phase 1 rule).
///
/// This is the **only genuine rewrite rule in Phase 0** — see module
/// doc for why arithmetic-identity rules are deferred to Phase 1.
pub struct MulPow2ToShl;

impl Rewrite for MulPow2ToShl {
    fn name(&self) -> &'static str {
        "mul_pow2_to_shl"
    }
    fn try_apply(&self, lhs: &InstKind) -> Option<InstKind> {
        // Excludes c==1 (shl by 0 is a no-op — MulOne handles that as
        // an identity rewrite instead) so the rule registry's
        // identity-first priority doesn't get racy.
        if let InstKind::BinOp(BinOp::Mul, x, Operand::ConstI64(c)) = lhs
            && *c > 1
            && (*c as u64).is_power_of_two()
        {
            let k = (*c as u64).trailing_zeros() as i64;
            return Some(InstKind::BinOp(BinOp::Shl, *x, Operand::ConstI64(k)));
        }
        None
    }
}

// ---- Phase 1 chunk 9a-2 — Identity-Value arithmetic identities ----
//
// Both rules rewrite to `Identity(x)` so the optimizer aliases
// `result` onto `x` via `set_opt_value` and never emits an inst.
// Each tries both operand orderings (commutative); a future
// `add_swap_canonical` rule will canonicalise constants to the RHS
// and let these match a single side.

/// `(add x 0) | (add 0 x) → Identity(x)` — additive identity.
pub struct AddZero;

impl Rewrite for AddZero {
    fn name(&self) -> &'static str {
        "add_zero_to_identity"
    }
    fn try_apply(&self, lhs: &InstKind) -> Option<InstKind> {
        if let InstKind::BinOp(BinOp::Add, a, b) = lhs {
            if matches!(b, Operand::ConstI64(0)) {
                return Some(InstKind::Identity(*a));
            }
            if matches!(a, Operand::ConstI64(0)) {
                return Some(InstKind::Identity(*b));
            }
        }
        None
    }
}

/// `(mul x 1) | (mul 1 x) → Identity(x)` — multiplicative identity.
pub struct MulOne;

impl Rewrite for MulOne {
    fn name(&self) -> &'static str {
        "mul_one_to_identity"
    }
    fn try_apply(&self, lhs: &InstKind) -> Option<InstKind> {
        if let InstKind::BinOp(BinOp::Mul, a, b) = lhs {
            if matches!(b, Operand::ConstI64(1)) {
                return Some(InstKind::Identity(*a));
            }
            if matches!(a, Operand::ConstI64(1)) {
                return Some(InstKind::Identity(*b));
            }
        }
        None
    }
}

// Static rule references — registry holds `&'static dyn Rewrite`.
static MUL_POW2: MulPow2ToShl = MulPow2ToShl;
static ADD_ZERO: AddZero = AddZero;
static MUL_ONE: MulOne = MulOne;

/// Built-in rule set. Order = priority — first match wins in
/// `apply_rewrites`. Identity rules fire first (zero-cost alias);
/// strength-reduction rules fire on what's left.
///
/// Phase 0 shipped MulPow2ToShl; Phase 1 chunk 9a-2 adds AddZero +
/// MulOne. Const-folding rules (SubSelf / XorSelf / MulZero) land
/// in chunk 9a-3 once `InstKind::Const` / constant-propagation
/// plumbing exists.
pub static BUILTIN_RULES: &[&'static dyn Rewrite] = &[&ADD_ZERO, &MUL_ONE, &MUL_POW2];

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::ValueId;

    fn val(n: u32) -> Operand {
        Operand::Value(ValueId(n))
    }

    #[test]
    fn mul_pow2_strength_reduces() {
        // mul %v 8 → shl %v 3
        let lhs = InstKind::BinOp(BinOp::Mul, val(4), Operand::ConstI64(8));
        let rhs = MulPow2ToShl.try_apply(&lhs).expect("must fire on 2^3");
        assert_eq!(
            rhs,
            InstKind::BinOp(BinOp::Shl, val(4), Operand::ConstI64(3))
        );
        // mul %v 1024 → shl %v 10
        let lhs2 = InstKind::BinOp(BinOp::Mul, val(4), Operand::ConstI64(1024));
        let rhs2 = MulPow2ToShl.try_apply(&lhs2).expect("must fire on 2^10");
        assert_eq!(
            rhs2,
            InstKind::BinOp(BinOp::Shl, val(4), Operand::ConstI64(10))
        );
        // Non-power-of-two doesn't fire.
        let lhs3 = InstKind::BinOp(BinOp::Mul, val(4), Operand::ConstI64(3));
        assert!(MulPow2ToShl.try_apply(&lhs3).is_none());
        // Zero doesn't fire (we require c > 0; mul-zero rule lands
        // in Phase 1 alongside InstKind::Identity).
        let lhs_zero = InstKind::BinOp(BinOp::Mul, val(4), Operand::ConstI64(0));
        assert!(MulPow2ToShl.try_apply(&lhs_zero).is_none());
        // Negative constant — also doesn't fire (sign isn't a
        // power-of-two by the u64-cast check; semantic mul by -8
        // would need sub/neg combination).
        let lhs_neg = InstKind::BinOp(BinOp::Mul, val(4), Operand::ConstI64(-8));
        assert!(MulPow2ToShl.try_apply(&lhs_neg).is_none());
        // Sub doesn't match the mul-pow2 rule.
        let sub = InstKind::BinOp(BinOp::Sub, val(4), Operand::ConstI64(8));
        assert!(MulPow2ToShl.try_apply(&sub).is_none());
    }

    #[test]
    fn registry_iteration_finds_match() {
        let reg = RewriteRegistry::with_builtins();
        // mul 8 → shl 3 via MulPow2ToShl (the Phase 0 only builtin).
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
        // point under the Phase 0 ruleset).
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
    fn add_zero_rewrites_to_identity_both_sides() {
        // (add x 0) → Identity(x)
        let lhs = InstKind::BinOp(BinOp::Add, val(4), Operand::ConstI64(0));
        assert_eq!(
            AddZero.try_apply(&lhs),
            Some(InstKind::Identity(Operand::Value(ValueId(4))))
        );
        // (add 0 x) → Identity(x)
        let lhs2 = InstKind::BinOp(BinOp::Add, Operand::ConstI64(0), val(7));
        assert_eq!(
            AddZero.try_apply(&lhs2),
            Some(InstKind::Identity(Operand::Value(ValueId(7))))
        );
        // (add x 1) — doesn't fire.
        let nope = InstKind::BinOp(BinOp::Add, val(4), Operand::ConstI64(1));
        assert!(AddZero.try_apply(&nope).is_none());
        // (sub x 0) — wrong op, doesn't fire.
        let other_op = InstKind::BinOp(BinOp::Sub, val(4), Operand::ConstI64(0));
        assert!(AddZero.try_apply(&other_op).is_none());
    }

    #[test]
    fn mul_one_rewrites_to_identity_both_sides() {
        // (mul x 1) → Identity(x)
        let lhs = InstKind::BinOp(BinOp::Mul, val(4), Operand::ConstI64(1));
        assert_eq!(
            MulOne.try_apply(&lhs),
            Some(InstKind::Identity(Operand::Value(ValueId(4))))
        );
        // (mul 1 x) → Identity(x)
        let lhs2 = InstKind::BinOp(BinOp::Mul, Operand::ConstI64(1), val(9));
        assert_eq!(
            MulOne.try_apply(&lhs2),
            Some(InstKind::Identity(Operand::Value(ValueId(9))))
        );
        // (mul x 0) — doesn't fire (MulZero is a Phase 1 chunk 9a-3 rule).
        let zero = InstKind::BinOp(BinOp::Mul, val(4), Operand::ConstI64(0));
        assert!(MulOne.try_apply(&zero).is_none());
        // (mul x 2) — doesn't fire (MulPow2ToShl handles it).
        let two = InstKind::BinOp(BinOp::Mul, val(4), Operand::ConstI64(2));
        assert!(MulOne.try_apply(&two).is_none());
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
}
