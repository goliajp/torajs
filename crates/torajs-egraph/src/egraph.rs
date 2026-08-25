//! Implicit e-graph — `ValueId` space reused as the eclass index,
//! with a Union-Find structure tracking equivalence classes formed
//! when rewrites unify an original value with a rewritten one.
//!
//! Design choice (per cfallin "acyclic e-graph" 2026-04): we do NOT
//! build the classical e-graph two-level structure (e-nodes +
//! e-classes + parent-pointer rebuilding). Instead, every SSA
//! `ValueId` is its own singleton eclass until a rewrite unions it
//! with another. The Union-Find leader-pointer (`leader[v]`) is the
//! representative; `find(v)` walks pointers with path compression.
//!
//! Per-eclass bookkeeping kept here:
//! - `value_to_opt_value[v]` — the canonicalized representative the
//!   optimize phase should currently use in place of `v`. Updated
//!   whenever `union` runs or a rewrite fires.
//! - `available_block[v]` — the dominator-tree block in which the
//!   value is computed and therefore available to descendants (used
//!   by elaboration for LICM placement).
//! - `eclass_size[v]` — number of e-nodes in the class. Capped at
//!   `ECLASS_ENODE_LIMIT` to keep extraction cost bounded.
//!
//! Phase 0 (this commit) ships the scaffold + Union-Find primitives
//! only; the rewrite engine that actually grows e-classes lands in
//! `rewrite.rs` (step 7).

use crate::scope_map::ScopedHashMap;
use std::collections::HashMap;
use torajs_core::ssa::{BlockId, InstKind, Operand, Type, ValueId};

/// Max e-nodes per equivalence class. When a rewrite would push the
/// class past this limit, the new node is dropped (or, future:
/// extraction picks the lowest-cost subset). Matches Cranelift's
/// `egraph.rs` `ECLASS_ENODE_LIMIT = 5`; tuned by them via Sightglass
/// bench, holds for torajs's Apple-Silicon target until proven otherwise.
pub const ECLASS_ENODE_LIMIT: u8 = 5;

/// Max recursive rewrite depth when a rule's RHS is itself eligible
/// for further rules. Hard cap so a pathological rule set can't
/// exhaust the stack. Matches Cranelift's `REWRITE_LIMIT = 5`.
pub const REWRITE_LIMIT: u8 = 5;

/// GVN canonicalisation key — (result type, instruction signature).
/// Two pure ops with the same key and the same already-canonicalised
/// operands are guaranteed to produce the same value, so the second
/// occurrence is replaced by a use of the first.
///
/// `InstKind` is the entire instruction shape *including* its operand
/// values; canonicalisation rewrites operands through `value_to_opt_value`
/// before hashing, so two textually different `BinOp(Add, %a, %b)` and
/// `BinOp(Add, %a', %b')` collapse iff `find(%a) == find(%a')` etc.
pub type GvnKey = (Type, InstKind);

/// The e-graph state for a single function. Lives for the duration of
/// the optimize+elaborate pass; not reused across functions (the
/// state shapes are SSA-value-indexed and SSA value ids differ per fn).
#[derive(Debug)]
pub struct Egraph {
    /// Union-Find leader pointer. `leader[v]` is the immediate parent
    /// in the equivalence-class tree; the root has `leader[v] == v`.
    /// `find` walks with path compression so amortised α(N) per op.
    leader: Vec<ValueId>,
    /// Canonical representative each `ValueId` should be rewritten to
    /// by the optimize phase. Distinct from `leader` because the
    /// "representative we'd like elaboration to extract" can differ
    /// from the strict UF root (cost-based decisions). For Phase 0 +
    /// the no-rule baseline these are identical; Phase 1 rewrite rules
    /// diverge them.
    value_to_opt_value: Vec<ValueId>,
    /// Per-value dominator-tree block at which the value is computed.
    /// `None` means "not yet placed" (a pure value waiting for
    /// elaboration to decide). Optimize phase sets this; elaboration
    /// reads it for LICM eligibility.
    available_block: Vec<Option<BlockId>>,
    /// E-class size = number of equivalent e-nodes that unioned into
    /// the class. Capped at `ECLASS_ENODE_LIMIT`. Bookkept on the
    /// UF *root*, not every member (so reads need a `find` first).
    eclass_size: HashMap<ValueId, u8>,
    /// GVN map — when the optimize phase encounters a pure instruction,
    /// it hashes (Type, InstKind-with-canonicalised-operands) here and
    /// reuses any prior value. Scoped so a value seen in a dominator
    /// is reused throughout its descendants but invisible to siblings
    /// (the source of automatic CSE/GVN/LICM via dominance).
    gvn_map: ScopedHashMap<GvnKey, ValueId>,
    /// Const-folded representative for an SSA value (chunk 9a-3).
    /// When a rewrite rule produces `Identity(Const*)` (e.g.
    /// SubSelf → 0, MulZero → 0), optimize calls `set_const(result,
    /// const_op)` here; elaborate's `map_operand` then propagates
    /// `Operand::Value(v)` → `const_op` so downstream uses fold to
    /// the literal without us having to add an `InstKind::Const`
    /// variant + cross-crate codegen cascade. Indexed by UF root —
    /// reads go through `find` first.
    value_to_const: HashMap<ValueId, Operand>,
    /// Non-Identity rewrite-rule output for an instruction's result
    /// (mandelbrot decomposition D4 engine gap): the optimize phase
    /// computed a cheaper equivalent kind (MulPow2ToShl's Shl,
    /// FMulTwoToAdd's FAdd) but until this map existed only the GVN
    /// key saw it — elaboration re-emitted the original kind and the
    /// strength reduction never reached the IR. Elaborate now prefers
    /// this kind (re-canonicalising its operands at emission time).
    /// Indexed by the defining inst's result ValueId, not the UF root
    /// — it describes that one instruction's emission, not the class.
    rewritten_kind: HashMap<ValueId, InstKind>,
}

impl Egraph {
    /// Build an empty e-graph sized for `n_values` SSA ValueIds. Every
    /// value starts as its own singleton class (`leader[v] = v`,
    /// `eclass_size[v] = 1`).
    pub fn new(n_values: usize) -> Self {
        let leader: Vec<ValueId> = (0..n_values as u32).map(ValueId).collect();
        let value_to_opt_value = leader.clone();
        Self {
            leader,
            value_to_opt_value,
            available_block: vec![None; n_values],
            eclass_size: HashMap::new(),
            gvn_map: ScopedHashMap::new(),
            value_to_const: HashMap::new(),
            rewritten_kind: HashMap::new(),
        }
    }

    /// Record the cheaper kind a non-Identity rewrite produced for
    /// this instruction; elaboration emits it in place of the source
    /// kind. See the `rewritten_kind` field doc.
    pub fn set_rewritten_kind(&mut self, v: ValueId, kind: InstKind) {
        self.rewritten_kind.insert(v, kind);
    }

    /// The rewrite-rule replacement kind for the inst defining `v`,
    /// if a rule fired. By defining ValueId (not UF root).
    pub fn rewritten_kind_of(&self, v: ValueId) -> Option<&InstKind> {
        self.rewritten_kind.get(&v)
    }

    /// Number of distinct SSA ValueIds tracked. Equals
    /// `function.values.len()` at construction; grows if the optimize
    /// phase mints new values during rewrite (via `extend`).
    pub fn len(&self) -> usize {
        self.leader.len()
    }

    /// True when no values are tracked. Mostly for unit tests on
    /// empty functions.
    pub fn is_empty(&self) -> bool {
        self.leader.is_empty()
    }

    /// Add `n` fresh singleton ValueIds at the end of the table.
    /// Returns the first new id. Used by the rewrite phase when a
    /// rule's RHS introduces a new SSA value.
    pub fn extend(&mut self, n: usize) -> ValueId {
        let start = self.leader.len() as u32;
        for i in 0..n as u32 {
            let v = ValueId(start + i);
            self.leader.push(v);
            self.value_to_opt_value.push(v);
            self.available_block.push(None);
        }
        ValueId(start)
    }

    /// Union-Find `find` with path compression. Returns the UF root
    /// of `v`'s equivalence class.
    pub fn find(&mut self, v: ValueId) -> ValueId {
        let mut cur = v;
        // Walk to root without mutating, then second pass compresses.
        loop {
            let parent = self.leader[cur.0 as usize];
            if parent == cur {
                break;
            }
            cur = parent;
        }
        let root = cur;
        // Path compression: rewrite every leader on the path to root.
        let mut walker = v;
        while walker != root {
            let next = self.leader[walker.0 as usize];
            self.leader[walker.0 as usize] = root;
            walker = next;
        }
        root
    }

    /// Union `a` and `b` into one equivalence class. Returns `true`
    /// if the two classes were previously distinct (a real merge
    /// happened). Respects `ECLASS_ENODE_LIMIT` — refuses the merge
    /// if combined class size would exceed it.
    pub fn union(&mut self, a: ValueId, b: ValueId) -> bool {
        let ra = self.find(a);
        let rb = self.find(b);
        if ra == rb {
            return false;
        }
        let size_a = self.eclass_size_or_one(ra);
        let size_b = self.eclass_size_or_one(rb);
        if size_a as u16 + size_b as u16 > ECLASS_ENODE_LIMIT as u16 {
            return false; // refuse — keep extraction cost bounded
        }
        // Union by-id: smaller id becomes root for determinism (avoids
        // hash-order-dependent leader choices in tests / replay).
        let (root, child) = if ra.0 < rb.0 { (ra, rb) } else { (rb, ra) };
        self.leader[child.0 as usize] = root;
        self.eclass_size.insert(root, size_a + size_b);
        self.eclass_size.remove(&child);
        true
    }

    /// Union with an explicit representative: `root` stays the class
    /// leader, `child` joins it. The GVN dedup path MUST use this —
    /// its `existing` value was inserted earlier in the domtree walk,
    /// so its def dominates the current inst, while plain by-id
    /// `union` can elect a later-defined value as leader (the inliner
    /// reverse-splices call sites, so ValueId order no longer tracks
    /// program order) and canonicalise earlier uses into a
    /// use-before-def. Same e-class size cap as `union`.
    pub fn union_into(&mut self, child: ValueId, root: ValueId) -> bool {
        let rc = self.find(child);
        let rr = self.find(root);
        if rc == rr {
            return false;
        }
        let size_c = self.eclass_size_or_one(rc);
        let size_r = self.eclass_size_or_one(rr);
        if size_c as u16 + size_r as u16 > ECLASS_ENODE_LIMIT as u16 {
            return false; // refuse — keep extraction cost bounded
        }
        self.leader[rc.0 as usize] = rr;
        self.eclass_size.insert(rr, size_c + size_r);
        self.eclass_size.remove(&rc);
        true
    }

    /// E-class size for the class containing `v`. Singleton = 1.
    pub fn eclass_size(&mut self, v: ValueId) -> u8 {
        let r = self.find(v);
        self.eclass_size_or_one(r)
    }

    /// True iff `a` and `b` are in the same equivalence class.
    pub fn equiv(&mut self, a: ValueId, b: ValueId) -> bool {
        self.find(a) == self.find(b)
    }

    /// Canonical value that the optimize phase should rewrite uses of
    /// `v` to. Defaults to `find(v)` (the UF root) but rewrite rules
    /// may install a different value (cost-better representative).
    pub fn opt_value(&mut self, v: ValueId) -> ValueId {
        let r = self.find(v);
        let stored = self.value_to_opt_value[r.0 as usize];
        // Recursive resolution: opt_value may itself have been
        // re-rewritten. Short walk in practice.
        if stored == r {
            r
        } else {
            self.opt_value(stored)
        }
    }

    /// P-OPT Phase 1 chunk 9a-3 — install a constant representative
    /// for `v`'s class root. Used by rewrite rules whose RHS is a
    /// literal value (e.g. `SubSelf` / `XorSelf` → 0, `MulZero` → 0).
    /// elaborate's `map_operand` checks `const_of` before falling back
    /// to `Operand::Value`, propagating the literal into every use
    /// without us minting an `InstKind::Const` variant.
    pub fn set_const(&mut self, v: ValueId, op: Operand) {
        let r = self.find(v);
        self.value_to_const.insert(r, op);
    }

    /// Lookup the constant representative for `v` if a rewrite rule
    /// has folded it. Returns `None` for ordinary values.
    pub fn const_of(&mut self, v: ValueId) -> Option<Operand> {
        let r = self.find(v);
        self.value_to_const.get(&r).copied()
    }

    /// Install a preferred canonical value for `v`'s class root. Used
    /// by rewrite rules when the RHS is "clearly better" than the LHS
    /// (Cranelift's `subsume` semantics).
    pub fn set_opt_value(&mut self, v: ValueId, target: ValueId) {
        let r = self.find(v);
        self.value_to_opt_value[r.0 as usize] = target;
    }

    /// Dominator-tree block at which `v` is currently placed, if any.
    pub fn available_block(&self, v: ValueId) -> Option<BlockId> {
        self.available_block.get(v.0 as usize).copied().flatten()
    }

    /// Record that `v` is available at block `b`. Set by the optimize
    /// phase; elaboration honors it (won't sink a value out of its
    /// available scope).
    pub fn set_available_block(&mut self, v: ValueId, b: BlockId) {
        let i = v.0 as usize;
        if i < self.available_block.len() {
            self.available_block[i] = Some(b);
        }
    }

    /// Borrow the GVN map (read access). Optimize phase inserts via
    /// `gvn_mut`.
    pub fn gvn(&self) -> &ScopedHashMap<GvnKey, ValueId> {
        &self.gvn_map
    }

    /// Mutable borrow of the GVN map. Optimize phase uses this to
    /// `push_scope` / `pop_scope` per domtree node and `insert` newly
    /// seen pure ops.
    pub fn gvn_mut(&mut self) -> &mut ScopedHashMap<GvnKey, ValueId> {
        &mut self.gvn_map
    }

    // ---- internals -------------------------------------------------------

    fn eclass_size_or_one(&self, root: ValueId) -> u8 {
        self.eclass_size.get(&root).copied().unwrap_or(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::ValueId;

    #[test]
    fn fresh_egraph_singletons() {
        let mut eg = Egraph::new(5);
        assert_eq!(eg.len(), 5);
        for v in 0..5u32 {
            let vid = ValueId(v);
            assert_eq!(eg.find(vid), vid);
            assert_eq!(eg.opt_value(vid), vid);
            assert_eq!(eg.eclass_size(vid), 1);
        }
    }

    #[test]
    fn union_merges_classes() {
        let mut eg = Egraph::new(4);
        assert!(eg.union(ValueId(0), ValueId(1)));
        assert!(eg.equiv(ValueId(0), ValueId(1)));
        assert_eq!(eg.eclass_size(ValueId(0)), 2);
        // Second union of already-equivalent values is a no-op.
        assert!(!eg.union(ValueId(0), ValueId(1)));
        // Distinct classes still distinct.
        assert!(!eg.equiv(ValueId(0), ValueId(2)));
    }

    #[test]
    fn union_into_keeps_explicit_root() {
        // the GVN-order contract: `existing` (earlier in the domtree
        // walk) stays leader even when the joining value has a
        // smaller id — by-id union would invert this and create
        // use-before-def canonicalisations post-inline.
        let mut eg = Egraph::new(30);
        assert!(eg.union_into(ValueId(5), ValueId(20)));
        assert_eq!(eg.find(ValueId(5)), ValueId(20));
        assert_eq!(eg.find(ValueId(20)), ValueId(20));
        // joining into an existing class keeps the same root
        assert!(eg.union_into(ValueId(3), ValueId(5)));
        assert_eq!(eg.find(ValueId(3)), ValueId(20));
        assert_eq!(eg.eclass_size(ValueId(20)), 3);
        // no-op on already-equivalent values
        assert!(!eg.union_into(ValueId(3), ValueId(20)));
    }

    #[test]
    fn union_by_smaller_id_for_determinism() {
        let mut eg = Egraph::new(3);
        eg.union(ValueId(2), ValueId(0));
        // Root should be the lower id, regardless of arg order.
        assert_eq!(eg.find(ValueId(2)), ValueId(0));
        assert_eq!(eg.find(ValueId(0)), ValueId(0));
        // Same shape with reversed args.
        let mut eg2 = Egraph::new(3);
        eg2.union(ValueId(0), ValueId(2));
        assert_eq!(eg2.find(ValueId(2)), ValueId(0));
    }

    #[test]
    fn path_compression_flattens_chain() {
        let mut eg = Egraph::new(5);
        // Build a chain by carefully unioning step-by-step:
        // 0 ↔ 1, 0 ↔ 2, 0 ↔ 3, 0 ↔ 4 — root is always 0.
        for v in 1..5u32 {
            eg.union(ValueId(0), ValueId(v));
        }
        // After find, leader[*] should all point directly to 0.
        for v in 1..5u32 {
            eg.find(ValueId(v));
            assert_eq!(eg.leader[v as usize], ValueId(0));
        }
    }

    #[test]
    fn eclass_limit_refuses_oversized_union() {
        let mut eg = Egraph::new(10);
        // Combine 0..4 into one class — total 5, exactly at limit.
        for v in 1..5u32 {
            assert!(eg.union(ValueId(0), ValueId(v)));
        }
        assert_eq!(eg.eclass_size(ValueId(0)), 5);
        // Adding ANY more must be rejected (5 + 1 > 5).
        assert!(!eg.union(ValueId(0), ValueId(5)));
        assert!(!eg.equiv(ValueId(0), ValueId(5)));
        assert_eq!(eg.eclass_size(ValueId(0)), 5);
    }

    #[test]
    fn extend_grows_value_table() {
        let mut eg = Egraph::new(3);
        let first_new = eg.extend(4);
        assert_eq!(first_new, ValueId(3));
        assert_eq!(eg.len(), 7);
        // New ids are singletons.
        for v in 3..7u32 {
            assert_eq!(eg.find(ValueId(v)), ValueId(v));
            assert_eq!(eg.eclass_size(ValueId(v)), 1);
        }
    }

    #[test]
    fn opt_value_default_is_uf_root() {
        let mut eg = Egraph::new(4);
        eg.union(ValueId(0), ValueId(1));
        assert_eq!(eg.opt_value(ValueId(1)), ValueId(0));
        // Override with explicit set_opt_value.
        eg.set_opt_value(ValueId(0), ValueId(3));
        assert_eq!(eg.opt_value(ValueId(0)), ValueId(3));
        // Following through unions: 1 → 0 → 3.
        assert_eq!(eg.opt_value(ValueId(1)), ValueId(3));
    }

    #[test]
    fn available_block_round_trip() {
        let mut eg = Egraph::new(3);
        assert_eq!(eg.available_block(ValueId(0)), None);
        eg.set_available_block(ValueId(0), BlockId(7));
        assert_eq!(eg.available_block(ValueId(0)), Some(BlockId(7)));
    }

    #[test]
    fn gvn_map_scope_lifecycle() {
        use torajs_core::ssa::{BinOp, Operand, Type};
        let mut eg = Egraph::new(3);
        let key: GvnKey = (
            Type::I64,
            InstKind::BinOp(BinOp::Add, Operand::ConstI64(1), Operand::ConstI64(2)),
        );
        eg.gvn_mut().insert(key.clone(), ValueId(0));
        assert_eq!(eg.gvn().get(&key), Some(&ValueId(0)));
        eg.gvn_mut().push_scope();
        eg.gvn_mut().insert(key.clone(), ValueId(1));
        assert_eq!(eg.gvn().get(&key), Some(&ValueId(1)));
        eg.gvn_mut().pop_scope();
        assert_eq!(eg.gvn().get(&key), Some(&ValueId(0)));
    }

    #[test]
    fn empty_egraph_safe() {
        let mut eg = Egraph::new(0);
        assert!(eg.is_empty());
        assert_eq!(eg.len(), 0);
        // extend on empty returns the first id.
        let v = eg.extend(2);
        assert_eq!(v, ValueId(0));
        assert_eq!(eg.len(), 2);
    }
}
