//! torajs-egraph — Cranelift-form acyclic e-graph mid-end optimizer.
//!
//! Three-phase pipeline (RFC 20260609-torajs-codegen-optimizer §2.3, ported
//! from `wasmtime/cranelift/codegen/src/egraph/`):
//!
//! 1. **Canonicalize + GVN**: dominator-preorder walk, scoped GVN map,
//!    eager rewrite via ISLE-inspired rule macros. Pure operators float
//!    out of the layout into an implicit egraph (SSA value space reused +
//!    surgical union-node spines).
//! 2. **Rewrite**: rule fires create `Op::Union(orig, rewritten)` chains
//!    capped at `ECLASS_ENODE_LIMIT`. Recursion bounded by `REWRITE_LIMIT`.
//! 3. **Elaborate**: scoped elaboration converts the egraph back to SSA-
//!    canonical form. Scope-stack memoization → GVN / CSE / LICM all fall
//!    out from domtree-aware placement, no separate passes.
//!
//! Phase 0 (this commit and following 3-4 commits) ships substrate
//! scaffold only — no rules, no production wiring. `EgraphPass::run`
//! is identity (round-trip-equivalent) until Phase 1 lands the first
//! rule cluster.

pub mod cost;
pub mod dominator;
pub mod scope_map;
