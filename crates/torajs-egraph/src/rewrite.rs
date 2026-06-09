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
//!
//! Phase 2 chunk 10a adds `CommutativeCanon` — a strictly-directed
//! single-shot swap that lands any ConstI64 operand on the RHS for
//! the five commutative integer ops (Add/Mul/And/Or/Xor). The
//! fixed-point predicate is "LHS is Const && RHS is not Const", so
//! one swap reaches canonical form and the rule self-disarms (no
//! commutativity explosion). Sequenced **first** in `BUILTIN_RULES`
//! so the identity / const-fold rules see normalised input; their
//! current both-sides match arms stay correct (Phase 2 follow-up
//! will simplify them to single-side once the canon invariant is
//! relied on). Floating-point FAdd/FMul are excluded — IEEE 754
//! commutativity is non-strict around NaN sign-bit propagation;
//! revisit under a fast-math flag.

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
//
// Phase 2 chunk 10b simplified the match to RHS-only after
// CommutativeCanon (chunk 10a) became the registry's first rule —
// `(add 0 x)` and `(mul 1 x)` are now canonicalised to the RHS
// form before these rules see them. The LHS-Const arm is
// unreachable and was dropped.

/// `(add x 0) → Identity(x)` — additive identity. Relies on
/// CommutativeCanon to land the const on the RHS.
pub struct AddZero;

impl Rewrite for AddZero {
    fn name(&self) -> &'static str {
        "add_zero_to_identity"
    }
    fn try_apply(&self, lhs: &InstKind) -> Option<InstKind> {
        if let InstKind::BinOp(BinOp::Add, a, Operand::ConstI64(0)) = lhs {
            return Some(InstKind::Identity(*a));
        }
        None
    }
}

/// `(mul x 1) → Identity(x)` — multiplicative identity. Relies on
/// CommutativeCanon to land the const on the RHS.
pub struct MulOne;

impl Rewrite for MulOne {
    fn name(&self) -> &'static str {
        "mul_one_to_identity"
    }
    fn try_apply(&self, lhs: &InstKind) -> Option<InstKind> {
        if let InstKind::BinOp(BinOp::Mul, a, Operand::ConstI64(1)) = lhs {
            return Some(InstKind::Identity(*a));
        }
        None
    }
}

// ---- Phase 1 chunk 9a-3 — const-folding arithmetic identities ----
//
// These RHS forms are `Identity(ConstI64(0))`; optimize.rs detects
// the const operand and calls `set_const` on the eclass root, so
// downstream `map_operand` propagates `0` into every use. No new
// `InstKind` variant or codegen path is needed.

/// `(sub x x) → 0` — self-subtraction identity.
pub struct SubSelf;

impl Rewrite for SubSelf {
    fn name(&self) -> &'static str {
        "sub_self_to_zero"
    }
    fn try_apply(&self, lhs: &InstKind) -> Option<InstKind> {
        if let InstKind::BinOp(BinOp::Sub, a, b) = lhs
            && a == b
        {
            return Some(InstKind::Identity(Operand::ConstI64(0)));
        }
        None
    }
}

/// `(xor x x) → 0` — self-xor identity.
pub struct XorSelf;

impl Rewrite for XorSelf {
    fn name(&self) -> &'static str {
        "xor_self_to_zero"
    }
    fn try_apply(&self, lhs: &InstKind) -> Option<InstKind> {
        if let InstKind::BinOp(BinOp::Xor, a, b) = lhs
            && a == b
        {
            return Some(InstKind::Identity(Operand::ConstI64(0)));
        }
        None
    }
}

/// `(mul x 0) → 0` — zero-annihilator. Relies on CommutativeCanon
/// to land the const on the RHS.
pub struct MulZero;

impl Rewrite for MulZero {
    fn name(&self) -> &'static str {
        "mul_zero_to_zero"
    }
    fn try_apply(&self, lhs: &InstKind) -> Option<InstKind> {
        if let InstKind::BinOp(BinOp::Mul, _, Operand::ConstI64(0)) = lhs {
            return Some(InstKind::Identity(Operand::ConstI64(0)));
        }
        None
    }
}

// ---- Phase 2 chunk 10c — extended identity / annihilator rules ----
//
// Six rules follow the chunk-10a/b pattern: CommutativeCanon
// guarantees that any ConstI64 operand of And/Or/Xor lands on the
// RHS, so each rule matches the single RHS-Const arm. Sub is not
// commutative; `sub x 0` already has the const on the RHS by SSA
// emission convention (no canon needed), and `sub 0 x → -x` is a
// separate negate rewrite, deferred to a later chunk.
//
// Rule body: textbook identity / annihilator —
//   sub x 0  → x         (additive identity, RHS-only by SSA shape)
//   xor x 0  → x         (xor identity)
//   or  x 0  → x         (or identity)
//   and x -1 → x         (all-ones bitwise identity, i64 -1 = 0xFF…F)
//   and x 0  → 0         (zero annihilator on And)
//   or  x -1 → -1        (all-ones annihilator on Or)

/// `(sub x 0) → Identity(x)` — subtractive zero identity.
pub struct SubZero;

impl Rewrite for SubZero {
    fn name(&self) -> &'static str {
        "sub_zero_to_identity"
    }
    fn try_apply(&self, lhs: &InstKind) -> Option<InstKind> {
        if let InstKind::BinOp(BinOp::Sub, a, Operand::ConstI64(0)) = lhs {
            return Some(InstKind::Identity(*a));
        }
        None
    }
}

/// `(xor x 0) → Identity(x)` — xor zero identity. Relies on
/// CommutativeCanon to land the const on the RHS.
pub struct XorZero;

impl Rewrite for XorZero {
    fn name(&self) -> &'static str {
        "xor_zero_to_identity"
    }
    fn try_apply(&self, lhs: &InstKind) -> Option<InstKind> {
        if let InstKind::BinOp(BinOp::Xor, a, Operand::ConstI64(0)) = lhs {
            return Some(InstKind::Identity(*a));
        }
        None
    }
}

/// `(or x 0) → Identity(x)` — or zero identity. Relies on
/// CommutativeCanon to land the const on the RHS.
pub struct OrZero;

impl Rewrite for OrZero {
    fn name(&self) -> &'static str {
        "or_zero_to_identity"
    }
    fn try_apply(&self, lhs: &InstKind) -> Option<InstKind> {
        if let InstKind::BinOp(BinOp::Or, a, Operand::ConstI64(0)) = lhs {
            return Some(InstKind::Identity(*a));
        }
        None
    }
}

/// `(and x -1) → Identity(x)` — bitwise all-ones identity for i64
/// (`-1` two's-complement is `0xFFFF_FFFF_FFFF_FFFF`). Relies on
/// CommutativeCanon to land the const on the RHS.
pub struct AndAllOnes;

impl Rewrite for AndAllOnes {
    fn name(&self) -> &'static str {
        "and_all_ones_to_identity"
    }
    fn try_apply(&self, lhs: &InstKind) -> Option<InstKind> {
        if let InstKind::BinOp(BinOp::And, a, Operand::ConstI64(-1)) = lhs {
            return Some(InstKind::Identity(*a));
        }
        None
    }
}

/// `(and x 0) → 0` — And-with-zero annihilator. Relies on
/// CommutativeCanon to land the const on the RHS.
pub struct AndZero;

impl Rewrite for AndZero {
    fn name(&self) -> &'static str {
        "and_zero_to_zero"
    }
    fn try_apply(&self, lhs: &InstKind) -> Option<InstKind> {
        if let InstKind::BinOp(BinOp::And, _, Operand::ConstI64(0)) = lhs {
            return Some(InstKind::Identity(Operand::ConstI64(0)));
        }
        None
    }
}

/// `(or x -1) → -1` — Or-with-all-ones annihilator. Relies on
/// CommutativeCanon to land the const on the RHS.
pub struct OrAllOnes;

impl Rewrite for OrAllOnes {
    fn name(&self) -> &'static str {
        "or_all_ones_to_all_ones"
    }
    fn try_apply(&self, lhs: &InstKind) -> Option<InstKind> {
        if let InstKind::BinOp(BinOp::Or, _, Operand::ConstI64(-1)) = lhs {
            return Some(InstKind::Identity(Operand::ConstI64(-1)));
        }
        None
    }
}

// ---- Phase 2 chunk 10a — commutative canonicalisation ----
//
// Strictly-directed single-shot swap. For the five commutative
// integer binops (Add/Mul/And/Or/Xor), if LHS is ConstI64 and RHS
// is not ConstI64, swap so the const lands on the right. The
// "RHS is not Const" guard makes the rule self-disarm after one
// fire — no commutativity explosion. Two-const cases (e.g.
// `add 3 5`) are not rewritten here; a future const-fold rule
// computes them.
//
// Floating-point FAdd/FMul are excluded — IEEE 754 commutativity
// is non-strict around NaN sign-bit propagation; revisit under a
// fast-math flag.

/// Canonicalise commutative integer binops by moving any ConstI64
/// operand to the RHS.
pub struct CommutativeCanon;

impl Rewrite for CommutativeCanon {
    fn name(&self) -> &'static str {
        "commutative_canon"
    }
    fn try_apply(&self, lhs: &InstKind) -> Option<InstKind> {
        if let InstKind::BinOp(op, a, b) = lhs
            && matches!(
                op,
                BinOp::Add | BinOp::Mul | BinOp::And | BinOp::Or | BinOp::Xor
            )
            && matches!(a, Operand::ConstI64(_))
            && !matches!(b, Operand::ConstI64(_))
        {
            return Some(InstKind::BinOp(*op, *b, *a));
        }
        None
    }
}

// Static rule references — registry holds `&'static dyn Rewrite`.
static COMMUTATIVE_CANON: CommutativeCanon = CommutativeCanon;
static MUL_POW2: MulPow2ToShl = MulPow2ToShl;
static ADD_ZERO: AddZero = AddZero;
static MUL_ONE: MulOne = MulOne;
static SUB_SELF: SubSelf = SubSelf;
static XOR_SELF: XorSelf = XorSelf;
static MUL_ZERO: MulZero = MulZero;
static SUB_ZERO: SubZero = SubZero;
static XOR_ZERO: XorZero = XorZero;
static OR_ZERO: OrZero = OrZero;
static AND_ALL_ONES: AndAllOnes = AndAllOnes;
static AND_ZERO: AndZero = AndZero;
static OR_ALL_ONES: OrAllOnes = OrAllOnes;

/// Built-in rule set. Order = priority — first match wins in
/// `apply_rewrites`. Identity / const-fold rules fire first
/// (zero-cost alias / literal propagation); strength-reduction
/// rules fire on what's left.
///
/// - Phase 0 shipped MulPow2ToShl.
/// - Phase 1 chunk 9a-2 added AddZero + MulOne.
/// - Phase 1 chunk 9a-3 adds SubSelf + XorSelf + MulZero (const-fold
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
pub static BUILTIN_RULES: &[&'static dyn Rewrite] = &[
    &COMMUTATIVE_CANON,
    &ADD_ZERO,
    &MUL_ONE,
    &MUL_ZERO,
    &SUB_ZERO,
    &XOR_ZERO,
    &OR_ZERO,
    &AND_ALL_ONES,
    &AND_ZERO,
    &OR_ALL_ONES,
    &SUB_SELF,
    &XOR_SELF,
    &MUL_POW2,
];

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
    fn add_zero_rewrites_to_identity_rhs_only_after_canon() {
        // (add x 0) → Identity(x)
        let lhs = InstKind::BinOp(BinOp::Add, val(4), Operand::ConstI64(0));
        assert_eq!(
            AddZero.try_apply(&lhs),
            Some(InstKind::Identity(Operand::Value(ValueId(4))))
        );
        // (add 0 x) — chunk 10b drops the LHS-Const arm; CommutativeCanon
        // (registry first) lands the const on the RHS before AddZero
        // sees it. Direct call no longer fires.
        let lhs2 = InstKind::BinOp(BinOp::Add, Operand::ConstI64(0), val(7));
        assert!(AddZero.try_apply(&lhs2).is_none());
        // (add x 1) — doesn't fire.
        let nope = InstKind::BinOp(BinOp::Add, val(4), Operand::ConstI64(1));
        assert!(AddZero.try_apply(&nope).is_none());
        // (sub x 0) — wrong op, doesn't fire.
        let other_op = InstKind::BinOp(BinOp::Sub, val(4), Operand::ConstI64(0));
        assert!(AddZero.try_apply(&other_op).is_none());
    }

    #[test]
    fn mul_one_rewrites_to_identity_rhs_only_after_canon() {
        // (mul x 1) → Identity(x)
        let lhs = InstKind::BinOp(BinOp::Mul, val(4), Operand::ConstI64(1));
        assert_eq!(
            MulOne.try_apply(&lhs),
            Some(InstKind::Identity(Operand::Value(ValueId(4))))
        );
        // (mul 1 x) — chunk 10b drops the LHS-Const arm; canon swaps
        // first. Direct call no longer fires.
        let lhs2 = InstKind::BinOp(BinOp::Mul, Operand::ConstI64(1), val(9));
        assert!(MulOne.try_apply(&lhs2).is_none());
        // (mul x 0) — doesn't fire (MulZero is a Phase 1 chunk 9a-3 rule).
        let zero = InstKind::BinOp(BinOp::Mul, val(4), Operand::ConstI64(0));
        assert!(MulOne.try_apply(&zero).is_none());
        // (mul x 2) — doesn't fire (MulPow2ToShl handles it).
        let two = InstKind::BinOp(BinOp::Mul, val(4), Operand::ConstI64(2));
        assert!(MulOne.try_apply(&two).is_none());
    }

    #[test]
    fn sub_self_folds_to_const_zero() {
        let lhs = InstKind::BinOp(BinOp::Sub, val(4), val(4));
        assert_eq!(
            SubSelf.try_apply(&lhs),
            Some(InstKind::Identity(Operand::ConstI64(0)))
        );
        // sub %a %b where a != b — no fire.
        let mixed = InstKind::BinOp(BinOp::Sub, val(4), val(5));
        assert!(SubSelf.try_apply(&mixed).is_none());
        // sub const const equal — also fires (constants compare by value).
        let const_eq = InstKind::BinOp(BinOp::Sub, Operand::ConstI64(7), Operand::ConstI64(7));
        assert_eq!(
            SubSelf.try_apply(&const_eq),
            Some(InstKind::Identity(Operand::ConstI64(0)))
        );
    }

    #[test]
    fn xor_self_folds_to_const_zero() {
        let lhs = InstKind::BinOp(BinOp::Xor, val(4), val(4));
        assert_eq!(
            XorSelf.try_apply(&lhs),
            Some(InstKind::Identity(Operand::ConstI64(0)))
        );
        let mixed = InstKind::BinOp(BinOp::Xor, val(4), val(5));
        assert!(XorSelf.try_apply(&mixed).is_none());
        // wrong op doesn't fire.
        let sub = InstKind::BinOp(BinOp::Sub, val(4), val(4));
        assert!(XorSelf.try_apply(&sub).is_none());
    }

    #[test]
    fn mul_zero_folds_rhs_only_after_canon() {
        let rhs_zero = InstKind::BinOp(BinOp::Mul, val(4), Operand::ConstI64(0));
        assert_eq!(
            MulZero.try_apply(&rhs_zero),
            Some(InstKind::Identity(Operand::ConstI64(0)))
        );
        // chunk 10b drops the LHS-Const arm; canon swaps first.
        let lhs_zero = InstKind::BinOp(BinOp::Mul, Operand::ConstI64(0), val(4));
        assert!(MulZero.try_apply(&lhs_zero).is_none());
        // mul x 1 — doesn't fire (MulOne handles).
        let one = InstKind::BinOp(BinOp::Mul, val(4), Operand::ConstI64(1));
        assert!(MulZero.try_apply(&one).is_none());
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

    // ---- Phase 2 chunk 10a — CommutativeCanon ----

    #[test]
    fn commutative_canon_swaps_const_to_rhs_for_all_five_ops() {
        for op in [BinOp::Add, BinOp::Mul, BinOp::And, BinOp::Or, BinOp::Xor] {
            let lhs = InstKind::BinOp(op, Operand::ConstI64(7), val(4));
            let rhs = CommutativeCanon
                .try_apply(&lhs)
                .unwrap_or_else(|| panic!("CommutativeCanon must fire on {op:?}"));
            assert_eq!(rhs, InstKind::BinOp(op, val(4), Operand::ConstI64(7)));
        }
    }

    #[test]
    fn commutative_canon_already_canonical_no_change() {
        // const already on RHS — rule must not fire.
        let lhs = InstKind::BinOp(BinOp::Add, val(4), Operand::ConstI64(7));
        assert_eq!(CommutativeCanon.try_apply(&lhs), None);
    }

    #[test]
    fn commutative_canon_skips_non_commutative_ops() {
        // Sub/SDiv/Shl etc. must NOT swap even when LHS is const.
        for op in [BinOp::Sub, BinOp::SDiv, BinOp::Shl, BinOp::AShr] {
            let lhs = InstKind::BinOp(op, Operand::ConstI64(7), val(4));
            assert_eq!(
                CommutativeCanon.try_apply(&lhs),
                None,
                "must not swap {op:?}"
            );
        }
    }

    #[test]
    fn commutative_canon_skips_two_const_operands() {
        // `add 3 5` is a const-fold candidate, not a canon candidate.
        let lhs = InstKind::BinOp(BinOp::Add, Operand::ConstI64(3), Operand::ConstI64(5));
        assert_eq!(CommutativeCanon.try_apply(&lhs), None);
    }

    #[test]
    fn commutative_canon_skips_two_value_operands() {
        // Both Value — no const to canonicalise; rule must not fire.
        let lhs = InstKind::BinOp(BinOp::Add, val(4), val(9));
        assert_eq!(CommutativeCanon.try_apply(&lhs), None);
    }

    #[test]
    fn commutative_canon_self_disarms_on_recursive_apply() {
        // apply_rewrites_recursive must reach a fixed point in exactly
        // one step — no swap/swap/swap loop within REWRITE_LIMIT.
        let lhs = InstKind::BinOp(BinOp::Mul, Operand::ConstI64(5), val(4));
        let canon = vec![&COMMUTATIVE_CANON as &dyn Rewrite];
        let out = apply_rewrites_recursive(&canon, &lhs);
        assert_eq!(
            out,
            InstKind::BinOp(BinOp::Mul, val(4), Operand::ConstI64(5))
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

    // ---- Phase 2 chunk 10c — extended identity / annihilator ----

    #[test]
    fn sub_zero_rewrites_to_identity() {
        let lhs = InstKind::BinOp(BinOp::Sub, val(4), Operand::ConstI64(0));
        assert_eq!(
            SubZero.try_apply(&lhs),
            Some(InstKind::Identity(Operand::Value(ValueId(4))))
        );
        // `sub 0 x` — Sub isn't commutative; rule must NOT fire (this
        // is the `0 - x = -x` negate case, deferred to a later rule).
        let lhs2 = InstKind::BinOp(BinOp::Sub, Operand::ConstI64(0), val(7));
        assert_eq!(SubZero.try_apply(&lhs2), None);
        // wrong op doesn't fire.
        let add = InstKind::BinOp(BinOp::Add, val(4), Operand::ConstI64(0));
        assert_eq!(SubZero.try_apply(&add), None);
        // non-zero const doesn't fire.
        let one = InstKind::BinOp(BinOp::Sub, val(4), Operand::ConstI64(1));
        assert_eq!(SubZero.try_apply(&one), None);
    }

    #[test]
    fn xor_zero_rewrites_to_identity_rhs_only_after_canon() {
        let lhs = InstKind::BinOp(BinOp::Xor, val(4), Operand::ConstI64(0));
        assert_eq!(
            XorZero.try_apply(&lhs),
            Some(InstKind::Identity(Operand::Value(ValueId(4))))
        );
        // LHS-Const direct-call no longer fires; canon swaps first.
        let lhs2 = InstKind::BinOp(BinOp::Xor, Operand::ConstI64(0), val(7));
        assert!(XorZero.try_apply(&lhs2).is_none());
        // non-zero const doesn't fire.
        let nz = InstKind::BinOp(BinOp::Xor, val(4), Operand::ConstI64(1));
        assert!(XorZero.try_apply(&nz).is_none());
    }

    #[test]
    fn or_zero_rewrites_to_identity_rhs_only_after_canon() {
        let lhs = InstKind::BinOp(BinOp::Or, val(4), Operand::ConstI64(0));
        assert_eq!(
            OrZero.try_apply(&lhs),
            Some(InstKind::Identity(Operand::Value(ValueId(4))))
        );
        let lhs2 = InstKind::BinOp(BinOp::Or, Operand::ConstI64(0), val(7));
        assert!(OrZero.try_apply(&lhs2).is_none());
    }

    #[test]
    fn and_all_ones_rewrites_to_identity_rhs_only_after_canon() {
        let lhs = InstKind::BinOp(BinOp::And, val(4), Operand::ConstI64(-1));
        assert_eq!(
            AndAllOnes.try_apply(&lhs),
            Some(InstKind::Identity(Operand::Value(ValueId(4))))
        );
        // LHS-Const direct-call no longer fires; canon swaps first.
        let lhs2 = InstKind::BinOp(BinOp::And, Operand::ConstI64(-1), val(7));
        assert!(AndAllOnes.try_apply(&lhs2).is_none());
        // `and x 0` is AndZero territory, not AndAllOnes.
        let zero = InstKind::BinOp(BinOp::And, val(4), Operand::ConstI64(0));
        assert!(AndAllOnes.try_apply(&zero).is_none());
    }

    #[test]
    fn and_zero_folds_to_zero_rhs_only_after_canon() {
        let lhs = InstKind::BinOp(BinOp::And, val(4), Operand::ConstI64(0));
        assert_eq!(
            AndZero.try_apply(&lhs),
            Some(InstKind::Identity(Operand::ConstI64(0)))
        );
        let lhs2 = InstKind::BinOp(BinOp::And, Operand::ConstI64(0), val(7));
        assert!(AndZero.try_apply(&lhs2).is_none());
        // `and x -1` is AndAllOnes territory.
        let neg1 = InstKind::BinOp(BinOp::And, val(4), Operand::ConstI64(-1));
        assert!(AndZero.try_apply(&neg1).is_none());
    }

    #[test]
    fn or_all_ones_folds_to_all_ones_rhs_only_after_canon() {
        let lhs = InstKind::BinOp(BinOp::Or, val(4), Operand::ConstI64(-1));
        assert_eq!(
            OrAllOnes.try_apply(&lhs),
            Some(InstKind::Identity(Operand::ConstI64(-1)))
        );
        let lhs2 = InstKind::BinOp(BinOp::Or, Operand::ConstI64(-1), val(7));
        assert!(OrAllOnes.try_apply(&lhs2).is_none());
        // `or x 0` is OrZero territory.
        let zero = InstKind::BinOp(BinOp::Or, val(4), Operand::ConstI64(0));
        assert!(OrAllOnes.try_apply(&zero).is_none());
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
}
