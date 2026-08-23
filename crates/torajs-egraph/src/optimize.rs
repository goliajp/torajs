//! Phase 1 of the egraph pipeline — canonicalize + GVN.
//!
//! Algorithm (per Cranelift `egraph/mod.rs::EgraphPass::
//! remove_pure_and_optimize`):
//!
//!   for blk in domtree.preorder():
//!     gvn.push_scope()
//!     for inst in blk.insts:
//!       canonicalised_operands = rewrite each operand through
//!                                egraph.opt_value(...)
//!       if inst is pure:
//!         key = (inst.result_type, inst_with_canon_operands)
//!         if let Some(existing) = gvn.get(&key):
//!           # Redundant — union the existing value with this one,
//!           # mark inst removable (pure value floats out of the
//!           # block; elaboration decides whether to place it).
//!           egraph.union(inst.result, existing)
//!           mark_removed(inst)
//!         else:
//!           // First occurrence — record and (Phase 1) try rewrite
//!           // rules to grow the eclass with cheaper representations.
//!           gvn.insert(key, inst.result)
//!           egraph.set_available_block(inst.result, blk.id)
//!           try_rewrite(inst)  // Phase 0 = no-op; Phase 1 cluster
//!                              // commits hook into here.
//!       else:
//!         // Skeleton (side-effecting) instructions stay in the layout
//!         // in their original block. Their operands still go through
//!         // opt_value canonicalization.
//!         (handled by elaboration when it re-emits the layout)
//!     gvn.pop_scope()
//!
//! Phase 0 (this commit) lands the walk skeleton, operand
//! canonicalization, and pure-vs-skeleton classification. Rewrite
//! rule firing (`try_rewrite`) is a no-op until `rewrite.rs` (step 7)
//! ships; e-class growth happens only via the GVN-redundancy `union`
//! call.

use crate::dominator::DominatorTree;
use crate::egraph::{Egraph, GvnKey};
use crate::loop_analysis::{LoopAnalysis, NaturalLoop};
use crate::rewrite::{RewriteRegistry, apply_rewrites_recursive};
use torajs_core::ssa::{BlockId, Function, InstKind, Operand};

/// Outcome of running Phase 1 over one function. Mostly diagnostic;
/// the elaboration phase reads the mutated `Egraph`, not this struct.
#[derive(Debug, Default, Clone, Copy)]
pub struct OptimizeStats {
    /// Number of pure instructions that found a redundancy in the
    /// scoped GVN map and got unioned with the prior value.
    pub gvn_hits: u32,
    /// Number of pure instructions that were first-of-kind in their
    /// dominance scope and got recorded in the GVN map.
    pub gvn_inserts: u32,
    /// Number of skeleton (side-effecting) instructions visited and
    /// left in place for elaboration.
    pub skeleton_seen: u32,
    /// Number of rewrite rules that fired into an `Identity(Value(_))`
    /// form and aliased the result via `set_opt_value` (chunk 9a-2:
    /// AddZero, MulOne).
    pub identity_rewrites_fired: u32,
    /// Number of pure instructions that LICM marked for hoisting out
    /// of a containing loop — `set_available_block(canon, idom(header))`
    /// overrode the source block's GVN entry. Elaboration consumes the
    /// marker and emits the inst into the dominator's preheader slot.
    pub licm_hoisted: u32,
}

/// Phase 1 pass: walk the dominator tree in preorder, canonicalising
/// operands through the egraph and feeding pure instructions through
/// the scoped GVN map. Mutates the egraph; reports per-pass stats.
///
/// Walk shape: strict domtree preorder DFS. Each block opens a fresh
/// GVN scope, processes its instructions (so they see every GVN entry
/// inserted in any ancestor block on the idom chain), then recurses
/// into each immediate child via `DominatorTree::children` before
/// popping its scope. This gives the textbook scoped-GVN invariant —
/// a value inserted while visiting block `b` is visible across every
/// descendant subtree of `b`, but two siblings (whose only common
/// ancestor is their idom) cannot see each other's inserts. The prior
/// Phase 0 walk used a flat per-block scope (RPO loop with push/pop on
/// each block boundary), which kept siblings isolated by accident but
/// also hid every dominator's GVN entries from its descendants —
/// negating the whole point of scoped GVN. The dom-nested walk is a
/// prerequisite for LICM (loop-invariant code motion), which needs to
/// hoist values into a preheader block dominating the loop body.
pub fn remove_pure_and_optimize(
    func: &Function,
    dom: &DominatorTree,
    egraph: &mut Egraph,
) -> OptimizeStats {
    let mut stats = OptimizeStats::default();
    let registry = RewriteRegistry::with_builtins();
    let rpo = dom.reverse_postorder();
    if rpo.is_empty() {
        return stats;
    }
    // RPO's first entry is the function's entry block by construction
    // (DFS postorder starts at entry, postorder reverses so entry sits
    // at index 0). Start the dom-nested DFS from there.
    let entry = rpo[0];
    visit_dom_nested(func, dom, egraph, &registry, entry, &mut stats);

    // LICM second pass — uses GVN-canonicalised `available_block`
    // information to identify pure instructions whose every operand
    // resolves to a value defined outside the loop body, and marks
    // their canonical e-class root for hoisting into the loop's
    // preheader slot (idom of the loop header). Elaboration consumes
    // the marker and re-emits the inst there. Iterating loops in
    // innermost-first order (smaller body = inner since outer body ⊇
    // inner body in a reducible CFG) gives single-pass iterative
    // LICM: an inst hoisted out of an inner loop becomes a candidate
    // for the next outer loop on the same pass.
    let loop_info = LoopAnalysis::compute(func, dom);
    licm_hoist(func, dom, &loop_info, egraph, &mut stats);

    stats
}

/// Recursive domtree preorder visit. Opens a GVN scope, processes the
/// block's instructions (their inserts become visible to every
/// descendant in the recursion), recurses into each immediate dominated
/// child, then pops the scope so siblings see a fresh state.
fn visit_dom_nested(
    func: &Function,
    dom: &DominatorTree,
    egraph: &mut Egraph,
    registry: &RewriteRegistry,
    blk_id: BlockId,
    stats: &mut OptimizeStats,
) {
    egraph.gvn_mut().push_scope();
    let blk_idx = blk_id.0 as usize;
    if blk_idx < func.blocks.len() {
        let block = &func.blocks[blk_idx];
        for inst in &block.insts {
            process_inst(func, egraph, registry, blk_id, inst, stats);
        }
    }
    // Domtree DFS — clone the child list to release the immutable
    // borrow of `dom` before the recursive call (which takes
    // `&mut Egraph` and conflicts with nothing here, but the borrow
    // checker can't prove it while we hold a slice into `dom`).
    let children: Vec<BlockId> = dom.children(blk_id).to_vec();
    for child in children {
        visit_dom_nested(func, dom, egraph, registry, child, stats);
    }
    egraph.gvn_mut().pop_scope();
}

/// Per-instruction worker — canonicalise operands, fire rewrite rules,
/// and either GVN-dedup or insert. Pure factored body of the inner
/// loop; the dom-nested walk above keeps the scope discipline.
fn process_inst(
    func: &Function,
    egraph: &mut Egraph,
    registry: &RewriteRegistry,
    blk_id: BlockId,
    inst: &torajs_core::ssa::Inst,
    stats: &mut OptimizeStats,
) {
    let Some(result) = inst.result else {
        // Void instructions (stores, some calls) are skeleton.
        stats.skeleton_seen += 1;
        return;
    };
    // Canonicalise operands by rewriting Operand::Value(v) →
    // Operand::Value(egraph.opt_value(v)). Constant operands
    // are passed through unchanged.
    let canon_kind = canonicalize_operands(&inst.kind, egraph);
    if !is_pure(&canon_kind) {
        stats.skeleton_seen += 1;
        return;
    }
    let result_ty = func
        .values
        .get(result.0 as usize)
        .map(|vi| vi.ty)
        .unwrap_or(torajs_core::ssa::Type::Void);

    // P-OPT Phase 1 — fire rewrite rules. When a rule
    // produces `Identity(Value(target))` (the AddZero /
    // MulOne shape), alias `result` onto `target` and
    // skip GVN insert: downstream uses canonicalise to
    // `target` via `opt_value`, and elaborate never sees
    // an Identity inst because the rewrite never lands
    // back in the IR — the rewrite "subsumes" the
    // original. Other rewrite shapes (e.g. MulPow2ToShl)
    // fall through to GVN using the rewritten kind so
    // dedup keys off the cheaper form.
    let rewritten_kind = apply_rewrites_recursive(registry.rules(), &canon_kind);
    if let InstKind::Identity(op) = rewritten_kind {
        match op {
            Operand::Value(target_v) => egraph.set_opt_value(result, target_v),
            // Chunk 9a-3 — const-fold rules (SubSelf /
            // XorSelf / MulZero) RHS lands here as
            // `Identity(ConstI64(0))` etc. Stash the
            // literal on the eclass root so map_operand
            // propagates it into every downstream use.
            const_op => egraph.set_const(result, const_op),
        }
        stats.identity_rewrites_fired += 1;
        return;
    }
    let key: GvnKey = (result_ty, rewritten_kind);
    if let Some(&existing) = egraph.gvn().get(&key) {
        // directed: `existing` was inserted earlier in the domtree
        // walk (its def dominates this inst) — it must stay the class
        // leader. Plain by-id union can elect this later-defined
        // `result` as leader and rewrite earlier uses into a
        // use-before-def (post-inline ValueIds don't track program
        // order).
        egraph.union_into(result, existing);
        stats.gvn_hits += 1;
    } else {
        egraph.gvn_mut().insert(key, result);
        egraph.set_available_block(result, blk_id);
        stats.gvn_inserts += 1;
    }
}

/// LICM (loop-invariant code motion) second pass — walk each natural
/// loop innermost-first and mark every pure, loop-invariant instruction
/// for hoisting into the loop's preheader slot. Phase 1 records the
/// decision on the e-class via `set_available_block(canon, target)`;
/// the elaboration phase translates that into the actual emit-block
/// choice when rebuilding the function.
///
/// The hoist target is `idom(loop.header)` — in a reducible CFG this
/// is the natural preheader slot: it strictly dominates the loop body,
/// branches into the header (possibly via a shared chain), and is not
/// itself in the loop body. We do not synthesise a physical preheader
/// block; the canonical instruction simply joins the existing idom
/// block's tail (before its terminator). Subsequent passes that need a
/// dedicated preheader can run a CFG-restructuring step ahead of this.
///
/// Innermost-first iteration (`body.len()` ASC since outer body ⊇
/// inner body in a reducible CFG) gives single-pass iterative LICM:
/// once an instruction is hoisted out of its innermost containing
/// loop via `set_available_block`, its `available_block` lookup falls
/// outside that loop, so the next outer loop sees it as invariant and
/// hoists again. Two-layer nests resolve in one pass; deeper nests
/// resolve in nesting-depth passes — Phase 1 ships single-pass and
/// leaves nested re-iteration to a polish revisit.
fn licm_hoist(
    func: &Function,
    _dom: &DominatorTree,
    loop_info: &LoopAnalysis,
    egraph: &mut Egraph,
    stats: &mut OptimizeStats,
) {
    if loop_info.loops().is_empty() {
        return;
    }
    let mut loops: Vec<&NaturalLoop> = loop_info.loops().iter().collect();
    // Smaller body = inner (outer body ⊇ inner body in reducible CFG).
    loops.sort_by_key(|l| l.body.len());

    for lp in loops {
        let Some(target) = _dom.immediate_dominator(lp.header) else {
            // Loop whose header is the entry block (or unreachable) has
            // no preheader candidate — skip. Real-world JS rarely lands
            // here; the function always has an entry-block prologue.
            continue;
        };
        // Defensive: if the idom is itself in the loop body (irreducible
        // CFG edge), abandon this loop — hoisting into the body is no
        // gain. Reducible CFG (the common case for compiled JS) never
        // hits this branch.
        if lp.body.iter().any(|&b| b == target) {
            continue;
        }

        for &body_block in &lp.body {
            let bi = body_block.0 as usize;
            if bi >= func.blocks.len() {
                continue;
            }
            for inst in &func.blocks[bi].insts {
                let Some(result) = inst.result else {
                    continue;
                };
                if !is_pure(&inst.kind) {
                    continue;
                }
                let canon = egraph.opt_value(result);
                // Skip if the canon's emit slot is already outside this
                // loop — earlier inner-loop pass already hoisted it, or
                // it was never inside (canonical representative emitted
                // in a dominator block by GVN-redundancy collapse).
                match egraph.available_block(canon) {
                    Some(b) if lp.body.iter().any(|&x| x == b) => {}
                    _ => continue,
                }
                // Canonicalise operands through opt_value before
                // checking invariance — an operand may itself have
                // been GVN-unioned with a value defined outside the
                // loop, and the textbook invariant check operates on
                // the canonical representative.
                if !operands_loop_invariant(&inst.kind, lp, egraph, &func.params) {
                    continue;
                }
                // Mark for hoist. set_available_block overrides the
                // GVN-inserted source-block entry; elaboration consumes
                // the new target when rebuilding the function.
                egraph.set_available_block(canon, target);
                stats.licm_hoisted += 1;
            }
        }
    }
}

/// True iff every SSA Value operand of `kind` resolves (via opt_value)
/// to a block that lies outside `lp.body`. Constants are trivially
/// loop-invariant. A value with `available_block == None` is treated as
/// "defined before any block" (function parameter / entry-block prologue
/// before the optimiser has set its location) and is also invariant.
///
/// Variants without value operands (Alloca, GlobalRef, etc.) are not
/// LICM candidates — they are skeleton or have intrinsic block affinity;
/// `is_pure` already filters them out before this check fires.
fn operands_loop_invariant(
    kind: &InstKind,
    lp: &NaturalLoop,
    egraph: &mut Egraph,
    params: &[torajs_core::ssa::ValueId],
) -> bool {
    let mut check = |op: &Operand| -> bool {
        match op {
            Operand::Value(v) => {
                let canon = egraph.opt_value(*v);
                match egraph.available_block(canon) {
                    // None means the value was never GVN-inserted: it's
                    // either a function parameter (always defined before
                    // any loop body — invariant) or a skeleton (Call /
                    // Store / Alloca) result emitted somewhere we don't
                    // track. Only the parameter case is provably outside
                    // the loop; treat skeleton results conservatively as
                    // non-invariant.
                    None => params.iter().any(|p| *p == canon),
                    Some(b) => !lp.body.iter().any(|&x| x == b),
                }
            }
            // Constants (ConstI64, ConstBool, ConstF64, ConstPtrNull,
            // …) are invariant by construction.
            _ => true,
        }
    };
    match kind {
        InstKind::BinOp(_, l, r) => check(l) && check(r),
        InstKind::ICmp(_, l, r) => check(l) && check(r),
        InstKind::FCmp(_, l, r) => check(l) && check(r),
        InstKind::SiToFp(v)
        | InstKind::FpToSi(v)
        | InstKind::ZExtBoolToI64(v)
        | InstKind::ZExtI32ToI64(v)
        | InstKind::Neg(v)
        | InstKind::Ctpop(v)
        | InstKind::Identity(v) => check(v),
        // Load is filtered out by is_pure; including it here would be
        // unsafe without alias analysis (a Store inside the loop body
        // may invalidate the loaded location). Phase 1 conservatively
        // refuses to LICM-hoist any Load.
        _ => false,
    }
}

/// Rewrite each Value operand through the egraph's canonical mapping.
/// Constants pass through unchanged. The returned `InstKind` is the
/// canonicalised form that the GVN map keys off.
fn canonicalize_operands(kind: &InstKind, egraph: &mut Egraph) -> InstKind {
    let map_op = |op: &Operand, eg: &mut Egraph| -> Operand {
        match op {
            Operand::Value(v) => Operand::Value(eg.opt_value(*v)),
            other => *other,
        }
    };
    match kind {
        InstKind::BinOp(op, l, r) => {
            let l = map_op(l, egraph);
            let r = map_op(r, egraph);
            InstKind::BinOp(*op, l, r)
        }
        InstKind::ICmp(p, l, r) => {
            let l = map_op(l, egraph);
            let r = map_op(r, egraph);
            InstKind::ICmp(*p, l, r)
        }
        InstKind::FCmp(p, l, r) => {
            let l = map_op(l, egraph);
            let r = map_op(r, egraph);
            InstKind::FCmp(*p, l, r)
        }
        InstKind::Load(ty, addr, off) => InstKind::Load(*ty, map_op(addr, egraph), *off),
        InstKind::LoadDyn(ty, addr, off) => {
            InstKind::LoadDyn(*ty, map_op(addr, egraph), map_op(off, egraph))
        }
        InstKind::SiToFp(v) => InstKind::SiToFp(map_op(v, egraph)),
        InstKind::FpToSi(v) => InstKind::FpToSi(map_op(v, egraph)),
        InstKind::ZExtBoolToI64(v) => InstKind::ZExtBoolToI64(map_op(v, egraph)),
        InstKind::ZExtI32ToI64(v) => InstKind::ZExtI32ToI64(map_op(v, egraph)),
        InstKind::Identity(v) => InstKind::Identity(map_op(v, egraph)),
        InstKind::Neg(v) => InstKind::Neg(map_op(v, egraph)),
        InstKind::Ctpop(v) => InstKind::Ctpop(map_op(v, egraph)),
        // Side-effecting kinds (Store, StoreDyn, Call, Alloca,
        // AllocaBytes) are not GVN candidates — return as-is. They
        // pass through the walk but don't participate in
        // canonicalisation since they're not hashed into gvn_map.
        other => other.clone(),
    }
}

/// Pure instructions have no side effects and produce a value that
/// depends solely on operands. Two pure ops with identical kind +
/// canonicalised operands always produce the same result, so the
/// second is redundant and can be replaced via a `union`.
///
/// Skeleton ops (Store, StoreDyn, Call, Alloca, AllocaBytes) write
/// to memory or have observable effects; they stay in the layout in
/// their original block.
fn is_pure(kind: &InstKind) -> bool {
    match kind {
        // Pure: arithmetic, comparisons, type casts, zero-extends.
        // Two pure ops with identical kind + canonical operands are
        // guaranteed to produce the same value (no memory dependence,
        // no observable effect).
        InstKind::BinOp(_, _, _)
        | InstKind::ICmp(_, _, _)
        | InstKind::FCmp(_, _, _)
        | InstKind::SiToFp(_)
        | InstKind::FpToSi(_)
        | InstKind::ZExtBoolToI64(_)
        | InstKind::ZExtI32ToI64(_) => true,
        // `Identity(op)` is a placeholder rewrite-rule output that the
        // elaborator drops by aliasing its result to `op`. Treated as
        // pure so GVN tolerates seeing it in the canonicalised stream
        // (elaboration runs after GVN, so Identity may legitimately
        // appear in the e-class table once Phase 1 rules fire).
        InstKind::Identity(_) => true,
        // `Neg(op)` is a pure single-operand arithmetic op — no
        // memory effect, deterministic output. GVN-eligible.
        InstKind::Neg(_) => true,
        // `Ctpop(op)` — pure single-operand bit count, deterministic.
        InstKind::Ctpop(_) => true,
        // `Select(ty, c, t, e)` — pure conditional move, deterministic
        // in its three operands, no memory effect. GVN never actually
        // sees it (select formation runs after the egraph pass), but
        // `classify_pure` feeds rc_peephole's `rc_transparent`, and a
        // Select between an inc/drop pair must not break the elision
        // window.
        InstKind::Select(_, _, _, _) => true,
        // `CtpopRangeSum(start, bound, acc)` — deterministic in its
        // three operands and free of memory effects, but control-
        // equivalent to a whole counted loop (the emit is a canned
        // SIMD reduction loop). Treated like Call: not a GVN/LICM
        // candidate, opaque to rc_peephole windows, and an
        // `inst_emits_bl` clobber site for the allocator. GVN never
        // actually sees it (formation runs after the egraph pass).
        InstKind::CtpopRangeSum(_, _, _) => false,
        // `Load` / `LoadDyn` are NOT pure without alias analysis —
        // a syntactically identical `load %p+off` may yield a
        // DIFFERENT value if an intervening `Store` to %p+off
        // happened between the two loads (or to any aliasing
        // pointer). torajs ssa_lower routinely emits `load %p; store
        // new, %p; load %p` for `let x = ...; x++; ...` and
        // `arr.push(v); load(arr+8)` (chunk 9c pattern) — GVN-dedup
        // there silently substitutes the stale pre-store value and
        // returns wrong results. Phase 1 alias module enables Load
        // dedup safely. Phase 0 keeps Load as skeleton.
        InstKind::Load(_, _, _)
        | InstKind::LoadDyn(_, _, _)
        | InstKind::LoadDynScaled8(_, _, _) => false,
        // Skeleton: writes and calls.
        InstKind::Store(_, _, _)
        | InstKind::StoreDyn(_, _, _)
        | InstKind::StoreDynScaled8(_, _, _)
        | InstKind::Call(_, _) => false,
        // Alloca returns a unique pointer per invocation — two
        // syntactically identical Alloca ops produce DIFFERENT
        // pointers, so they must NOT be GVN-deduplicated. Treated as
        // skeleton to keep them in their original block.
        InstKind::Alloca(_) | InstKind::AllocaBytes(_) => false,
        // Catch-all: any future InstKind variant defaults to skeleton.
        // Conservative — adding a new variant won't silently let
        // miscompiles slip through GVN deduplication. The optimizer
        // author owning the new variant should add an explicit `true`
        // arm if it's pure.
        _ => false,
    }
}

/// Phase-0 sanity helper exposed to allow callers (and tests) to
/// confirm a particular `InstKind` would be GVN-candidate. Re-exported
/// so the integration layer can document GVN coverage.
pub fn classify_pure(kind: &InstKind) -> bool {
    is_pure(kind)
}

/// Unused yet but reserved for the rewrite-engine hook (`try_rewrite`).
/// Marker symbol so the rewrite module doesn't accidentally introduce
/// a private name collision when it lands in step 7.
#[allow(dead_code)]
const _PHASE1_REWRITE_HOOK: () = ();

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dominator::DominatorTree;
    use torajs_core::ssa::{
        BinOp, Block, BlockId, Function, IPred, Inst, Operand, Terminator, Type, ValueId, ValueInfo,
    };

    fn empty_inst(result: ValueId, kind: InstKind) -> Inst {
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
    fn int_value(name: &str) -> ValueInfo {
        ValueInfo {
            ty: Type::I64,
            name: Some(name.into()),
        }
    }

    fn fixture(values: Vec<ValueInfo>, blocks: Vec<Block>) -> Function {
        Function {
            name: "test".into(),
            params: vec![],
            ret: Type::Void,
            blocks,
            values,
            current_origin: None,
        }
    }

    fn fixture_with_params(
        params: Vec<ValueId>,
        values: Vec<ValueInfo>,
        blocks: Vec<Block>,
    ) -> Function {
        Function {
            name: "test".into(),
            params,
            ret: Type::Void,
            blocks,
            values,
            current_origin: None,
        }
    }

    #[test]
    fn gvn_dedup_two_identical_adds() {
        // %2 = add %0, %1
        // %3 = add %0, %1   # GVN should union %3 into %2
        // ret
        // (Use Value+Value operands — both-const operands would
        // const-fold to Identity in chunk 11c, bypassing GVN.)
        let values = vec![
            int_value("a"),
            int_value("b"),
            int_value("c"),
            int_value("d"),
        ];
        let blocks = vec![Block {
            id: BlockId(0),
            insts: vec![
                empty_inst(
                    ValueId(2),
                    InstKind::BinOp(
                        BinOp::Add,
                        Operand::Value(ValueId(0)),
                        Operand::Value(ValueId(1)),
                    ),
                ),
                empty_inst(
                    ValueId(3),
                    InstKind::BinOp(
                        BinOp::Add,
                        Operand::Value(ValueId(0)),
                        Operand::Value(ValueId(1)),
                    ),
                ),
            ],
            term: Terminator::Ret(None),
        }];
        let func = fixture(values, blocks);
        let dom = DominatorTree::compute(&func);
        let mut eg = Egraph::new(func.values.len());
        let stats = remove_pure_and_optimize(&func, &dom, &mut eg);
        assert_eq!(stats.gvn_inserts, 1, "first add inserted into GVN map");
        assert_eq!(stats.gvn_hits, 1, "second add unioned with first");
        assert!(eg.equiv(ValueId(2), ValueId(3)));
        // %3's opt_value resolves to %2 (lower-id UF root).
        assert_eq!(eg.opt_value(ValueId(3)), ValueId(2));
    }

    #[test]
    fn skeleton_calls_not_gvn_deduped() {
        // %0 = call f()   <- void call, skeleton
        // %1 = call f()   <- second call, also skeleton, NOT deduped
        let blocks = vec![Block {
            id: BlockId(0),
            insts: vec![
                void_inst(InstKind::Call(torajs_core::ssa::FuncId(0), vec![])),
                void_inst(InstKind::Call(torajs_core::ssa::FuncId(0), vec![])),
            ],
            term: Terminator::Ret(None),
        }];
        let func = fixture(vec![], blocks);
        let dom = DominatorTree::compute(&func);
        let mut eg = Egraph::new(0);
        let stats = remove_pure_and_optimize(&func, &dom, &mut eg);
        assert_eq!(stats.skeleton_seen, 2);
        assert_eq!(stats.gvn_hits, 0);
        assert_eq!(stats.gvn_inserts, 0);
    }

    #[test]
    fn canonicalize_walks_union_chain() {
        // Pre-seed an egraph with %0 unioned to %1, then check that
        // optimize canonicalises operand %0 to %1 before hashing.
        // %0, %1 = params; %2 = add %0, %3; %4 = add %1, %3
        // After pre-union(0,1): both adds canonicalise to (add %0,%3)
        // and should dedup.
        let values = vec![
            int_value("p0"),
            int_value("p1"),
            int_value("r2"),
            int_value("p3"),
            int_value("r4"),
        ];
        let blocks = vec![Block {
            id: BlockId(0),
            insts: vec![
                empty_inst(
                    ValueId(2),
                    InstKind::BinOp(
                        BinOp::Add,
                        Operand::Value(ValueId(0)),
                        Operand::Value(ValueId(3)),
                    ),
                ),
                empty_inst(
                    ValueId(4),
                    InstKind::BinOp(
                        BinOp::Add,
                        Operand::Value(ValueId(1)),
                        Operand::Value(ValueId(3)),
                    ),
                ),
            ],
            term: Terminator::Ret(None),
        }];
        let func = fixture(values, blocks);
        let dom = DominatorTree::compute(&func);
        let mut eg = Egraph::new(func.values.len());
        // Pre-union %0 and %1 (e.g. as if a prior rewrite proved them
        // equal). Lower id wins as UF root.
        assert!(eg.union(ValueId(0), ValueId(1)));
        let stats = remove_pure_and_optimize(&func, &dom, &mut eg);
        assert_eq!(stats.gvn_inserts, 1);
        assert_eq!(
            stats.gvn_hits, 1,
            "two adds collapse after canonicalization"
        );
        assert!(eg.equiv(ValueId(2), ValueId(4)));
    }

    #[test]
    fn gvn_does_not_leak_across_sibling_scopes() {
        // 0 (cond) -> 1, 2 ; 1 has %2 = add x,y ; 2 has %3 = add x,y
        // Dom-nested DFS: visit(0) push, process [], recurse {1, 2}.
        // visit(1) push, %2 inserts into GVN, pop. visit(2) push —
        // sees a scope state where %2's entry has been popped along
        // with block 1's scope, so %3 is a fresh insert, NOT a hit on
        // %2. Sibling isolation: 2 inserts + 0 hits.
        // (Use Value+Value operands — both-const operands would
        // const-fold to Identity in chunk 11c, bypassing GVN.)
        let values = vec![
            int_value("c"),
            int_value("x"),
            int_value("y"),
            int_value("z"),
        ];
        let blocks = vec![
            Block {
                id: BlockId(0),
                insts: vec![],
                term: Terminator::CondBr {
                    cond: Operand::ConstBool(true),
                    then_blk: BlockId(1),
                    else_blk: BlockId(2),
                },
            },
            Block {
                id: BlockId(1),
                insts: vec![empty_inst(
                    ValueId(2),
                    InstKind::BinOp(
                        BinOp::Add,
                        Operand::Value(ValueId(0)),
                        Operand::Value(ValueId(1)),
                    ),
                )],
                term: Terminator::Ret(None),
            },
            Block {
                id: BlockId(2),
                insts: vec![empty_inst(
                    ValueId(3),
                    InstKind::BinOp(
                        BinOp::Add,
                        Operand::Value(ValueId(0)),
                        Operand::Value(ValueId(1)),
                    ),
                )],
                term: Terminator::Ret(None),
            },
        ];
        let func = fixture(values, blocks);
        let dom = DominatorTree::compute(&func);
        let mut eg = Egraph::new(func.values.len());
        let stats = remove_pure_and_optimize(&func, &dom, &mut eg);
        // Strict dom-nested invariant — siblings must not GVN-dedup.
        assert_eq!(stats.gvn_inserts, 2, "each sibling inserts independently");
        assert_eq!(stats.gvn_hits, 0, "siblings must not see each other");
        assert!(
            !eg.equiv(ValueId(2), ValueId(3)),
            "sibling adds in disjoint branches must remain distinct values"
        );
    }

    #[test]
    fn gvn_value_inserted_in_dominator_visible_in_descendant() {
        // 0 -> 1 (ret). Block 0 is the immediate dominator of block 1.
        // %2 = add %0, %1 in block 0 (inserts into GVN map).
        // %3 = add %0, %1 in block 1 (descendant) — dom-nested DFS
        // keeps block 0's GVN entry on the scope stack while visiting
        // block 1, so %3 hits %2 and gets unioned. This is the textbook
        // dom-nested GVN invariant that the prior flat-scope walk
        // failed to honour (it popped block 0's entries before
        // entering block 1, hiding every dominator's GVN map).
        let values = vec![
            int_value("a"),
            int_value("b"),
            int_value("c"),
            int_value("d"),
        ];
        let blocks = vec![
            Block {
                id: BlockId(0),
                insts: vec![empty_inst(
                    ValueId(2),
                    InstKind::BinOp(
                        BinOp::Add,
                        Operand::Value(ValueId(0)),
                        Operand::Value(ValueId(1)),
                    ),
                )],
                term: Terminator::Br(BlockId(1)),
            },
            Block {
                id: BlockId(1),
                insts: vec![empty_inst(
                    ValueId(3),
                    InstKind::BinOp(
                        BinOp::Add,
                        Operand::Value(ValueId(0)),
                        Operand::Value(ValueId(1)),
                    ),
                )],
                term: Terminator::Ret(None),
            },
        ];
        let func = fixture(values, blocks);
        let dom = DominatorTree::compute(&func);
        let mut eg = Egraph::new(func.values.len());
        let stats = remove_pure_and_optimize(&func, &dom, &mut eg);
        assert_eq!(stats.gvn_inserts, 1, "dominator's add inserts once");
        assert_eq!(
            stats.gvn_hits, 1,
            "descendant's add hits the dominator's entry"
        );
        assert!(
            eg.equiv(ValueId(2), ValueId(3)),
            "dominator and dominated adds must be unioned"
        );
        assert_eq!(eg.opt_value(ValueId(3)), ValueId(2));
    }

    #[test]
    fn gvn_diamond_join_does_not_see_branch_inserts() {
        // 0 (cond) -> 1, 2; both -> 3 (ret).
        // Block 1 has %4 = add %0,%1 ; block 2 has %5 = add %0,%1.
        // Block 3 has %6 = add %0,%1. Block 3's idom is block 0 (the
        // diamond's pre-branch entry), so block 3 must NOT see block
        // 1's or block 2's inserts — they live in sibling subtrees
        // popped before block 3's scope opens.
        let values = vec![
            int_value("c"),
            int_value("x"),
            int_value("y"),
            int_value("r1"),
            int_value("r2"),
            int_value("r3"),
            int_value("r4"),
        ];
        let blocks = vec![
            Block {
                id: BlockId(0),
                insts: vec![],
                term: Terminator::CondBr {
                    cond: Operand::ConstBool(true),
                    then_blk: BlockId(1),
                    else_blk: BlockId(2),
                },
            },
            Block {
                id: BlockId(1),
                insts: vec![empty_inst(
                    ValueId(4),
                    InstKind::BinOp(
                        BinOp::Add,
                        Operand::Value(ValueId(1)),
                        Operand::Value(ValueId(2)),
                    ),
                )],
                term: Terminator::Br(BlockId(3)),
            },
            Block {
                id: BlockId(2),
                insts: vec![empty_inst(
                    ValueId(5),
                    InstKind::BinOp(
                        BinOp::Add,
                        Operand::Value(ValueId(1)),
                        Operand::Value(ValueId(2)),
                    ),
                )],
                term: Terminator::Br(BlockId(3)),
            },
            Block {
                id: BlockId(3),
                insts: vec![empty_inst(
                    ValueId(6),
                    InstKind::BinOp(
                        BinOp::Add,
                        Operand::Value(ValueId(1)),
                        Operand::Value(ValueId(2)),
                    ),
                )],
                term: Terminator::Ret(None),
            },
        ];
        let func = fixture(values, blocks);
        let dom = DominatorTree::compute(&func);
        let mut eg = Egraph::new(func.values.len());
        let stats = remove_pure_and_optimize(&func, &dom, &mut eg);
        // %4, %5, %6 each insert in their own scope; no cross-branch
        // GVN hits because block 3's idom is block 0, not block 1/2.
        assert_eq!(stats.gvn_inserts, 3);
        assert_eq!(stats.gvn_hits, 0);
        assert!(!eg.equiv(ValueId(4), ValueId(5)));
        assert!(!eg.equiv(ValueId(4), ValueId(6)));
        assert!(!eg.equiv(ValueId(5), ValueId(6)));
    }

    #[test]
    fn empty_function_safe() {
        let func = fixture(vec![], vec![]);
        let dom = DominatorTree::compute(&func);
        let mut eg = Egraph::new(0);
        let stats = remove_pure_and_optimize(&func, &dom, &mut eg);
        assert_eq!(stats.gvn_hits, 0);
        assert_eq!(stats.gvn_inserts, 0);
        assert_eq!(stats.skeleton_seen, 0);
    }

    #[test]
    fn add_zero_rewrite_aliases_result_to_operand() {
        // %0 = (param-shaped) value
        // %1 = add %0, 0     # AddZero fires → set_opt_value(%1, %0)
        // %2 = sub %1, %0    # %1 must canonicalise to %0 downstream
        let values = vec![int_value("a"), int_value("r1"), int_value("r2")];
        let blocks = vec![Block {
            id: BlockId(0),
            insts: vec![
                empty_inst(
                    ValueId(1),
                    InstKind::BinOp(BinOp::Add, Operand::Value(ValueId(0)), Operand::ConstI64(0)),
                ),
                empty_inst(
                    ValueId(2),
                    InstKind::BinOp(
                        BinOp::Sub,
                        Operand::Value(ValueId(1)),
                        Operand::Value(ValueId(0)),
                    ),
                ),
            ],
            term: Terminator::Ret(None),
        }];
        let func = fixture(values, blocks);
        let dom = DominatorTree::compute(&func);
        let mut eg = Egraph::new(func.values.len());
        let stats = remove_pure_and_optimize(&func, &dom, &mut eg);
        // 1× AddZero on `add %0 0` aliases %1→%0; the `sub %1 %0`
        // then canonicalises to `sub %0 %0` and SubSelf folds it to
        // 0 — chunk 9a-3 cascade.
        assert_eq!(stats.identity_rewrites_fired, 2);
        // opt_value(%1) resolves to %0 because the rewrite wired the alias.
        assert_eq!(eg.opt_value(ValueId(1)), ValueId(0));
        // %2 is const-folded to ConstI64(0).
        assert_eq!(eg.const_of(ValueId(2)), Some(Operand::ConstI64(0)));
    }

    #[test]
    fn sub_self_rewrite_folds_to_const_zero_propagated_to_uses() {
        // %0, %1 = (param-shaped) values
        // %2 = sub %0 %0   # SubSelf fires → set_const(%2, ConstI64(0))
        // %3 = add %2 %1   # the optimize/elaborate pipeline must
        //                   propagate 0 into %2's slot downstream
        let values = vec![
            int_value("a"),
            int_value("b"),
            int_value("r2"),
            int_value("r3"),
        ];
        let blocks = vec![Block {
            id: BlockId(0),
            insts: vec![
                empty_inst(
                    ValueId(2),
                    InstKind::BinOp(
                        BinOp::Sub,
                        Operand::Value(ValueId(0)),
                        Operand::Value(ValueId(0)),
                    ),
                ),
                empty_inst(
                    ValueId(3),
                    InstKind::BinOp(
                        BinOp::Add,
                        Operand::Value(ValueId(2)),
                        Operand::Value(ValueId(1)),
                    ),
                ),
            ],
            term: Terminator::Ret(None),
        }];
        let func = fixture(values, blocks);
        let dom = DominatorTree::compute(&func);
        let mut eg = Egraph::new(func.values.len());
        let stats = remove_pure_and_optimize(&func, &dom, &mut eg);
        assert_eq!(
            stats.identity_rewrites_fired, 1,
            "SubSelf must fire once on (sub %0 %0)"
        );
        // The eclass for %2 now carries ConstI64(0) so callers
        // resolve it as a literal.
        assert_eq!(eg.const_of(ValueId(2)), Some(Operand::ConstI64(0)));
    }

    #[test]
    fn mul_zero_rewrite_folds_to_const_zero() {
        // %0 = param-shaped
        // %1 = mul %0 0   # MulZero fires → set_const(%1, ConstI64(0))
        let values = vec![int_value("a"), int_value("r1")];
        let blocks = vec![Block {
            id: BlockId(0),
            insts: vec![empty_inst(
                ValueId(1),
                InstKind::BinOp(BinOp::Mul, Operand::Value(ValueId(0)), Operand::ConstI64(0)),
            )],
            term: Terminator::Ret(None),
        }];
        let func = fixture(values, blocks);
        let dom = DominatorTree::compute(&func);
        let mut eg = Egraph::new(func.values.len());
        let stats = remove_pure_and_optimize(&func, &dom, &mut eg);
        assert_eq!(stats.identity_rewrites_fired, 1);
        assert_eq!(eg.const_of(ValueId(1)), Some(Operand::ConstI64(0)));
    }

    #[test]
    fn mul_one_rewrite_aliases_result_to_operand() {
        // %0 = (param-shaped) value
        // %1 = mul %0, 1     # MulOne fires → set_opt_value(%1, %0)
        let values = vec![int_value("a"), int_value("r1")];
        let blocks = vec![Block {
            id: BlockId(0),
            insts: vec![empty_inst(
                ValueId(1),
                InstKind::BinOp(BinOp::Mul, Operand::Value(ValueId(0)), Operand::ConstI64(1)),
            )],
            term: Terminator::Ret(None),
        }];
        let func = fixture(values, blocks);
        let dom = DominatorTree::compute(&func);
        let mut eg = Egraph::new(func.values.len());
        let stats = remove_pure_and_optimize(&func, &dom, &mut eg);
        assert_eq!(stats.identity_rewrites_fired, 1);
        assert_eq!(eg.opt_value(ValueId(1)), ValueId(0));
    }

    #[test]
    fn classify_pure_lists_match_canonical_set() {
        assert!(classify_pure(&InstKind::BinOp(
            BinOp::Add,
            Operand::ConstI64(1),
            Operand::ConstI64(2),
        )));
        assert!(classify_pure(&InstKind::ICmp(
            IPred::Eq,
            Operand::ConstI64(0),
            Operand::ConstI64(0),
        )));
        // Load is NOT pure without alias analysis — see is_pure doc.
        assert!(!classify_pure(&InstKind::Load(
            Type::I64,
            Operand::ConstPtrNull,
            0,
        )));
        assert!(!classify_pure(&InstKind::Store(
            Operand::ConstI64(0),
            Operand::ConstPtrNull,
            0,
        )));
        assert!(!classify_pure(&InstKind::Call(
            torajs_core::ssa::FuncId(0),
            vec![],
        )));
        // Alloca explicitly NOT pure (unique pointer per invoke).
        assert!(!classify_pure(&InstKind::Alloca(Type::I64)));
    }

    #[test]
    fn licm_hoists_invariant_add_to_preheader() {
        // params: %0 = a, %1 = b (loop-invariant by virtue of being
        // function parameters).
        //
        //   block 0 (preheader slot) → br block 1 (header)
        //   block 1 (header)          → condbr → block 2 (body), block 3 (exit)
        //   block 2 (body)            : %2 = add %0, %1; br block 1
        //   block 3 (exit)            : ret
        //
        // LICM marks %2's available_block from block 2 (body) to block
        // 0 (preheader slot = idom(header=1)). Elaboration consumes that
        // marker to re-emit the inst in the dominator block.
        let values = vec![int_value("a"), int_value("b"), int_value("c")];
        let blocks = vec![
            Block {
                id: BlockId(0),
                insts: vec![],
                term: Terminator::Br(BlockId(1)),
            },
            Block {
                id: BlockId(1),
                insts: vec![],
                term: Terminator::CondBr {
                    cond: Operand::ConstBool(true),
                    then_blk: BlockId(2),
                    else_blk: BlockId(3),
                },
            },
            Block {
                id: BlockId(2),
                insts: vec![empty_inst(
                    ValueId(2),
                    InstKind::BinOp(
                        BinOp::Add,
                        Operand::Value(ValueId(0)),
                        Operand::Value(ValueId(1)),
                    ),
                )],
                term: Terminator::Br(BlockId(1)),
            },
            Block {
                id: BlockId(3),
                insts: vec![],
                term: Terminator::Ret(None),
            },
        ];
        let func = fixture_with_params(vec![ValueId(0), ValueId(1)], values, blocks);
        let dom = DominatorTree::compute(&func);
        let mut eg = Egraph::new(func.values.len());
        let stats = remove_pure_and_optimize(&func, &dom, &mut eg);
        assert_eq!(
            stats.licm_hoisted, 1,
            "loop-invariant add must be marked for hoisting once"
        );
        // idom(BlockId(1)) = BlockId(0) — the preheader slot.
        let canon = eg.opt_value(ValueId(2));
        assert_eq!(eg.available_block(canon), Some(BlockId(0)));
    }

    #[test]
    fn licm_does_not_hoist_when_operand_is_loop_internal_call() {
        // params: %0 = a
        //
        //   block 0 → br 1 (header)
        //   block 1 → condbr → 2 (body), 3 (exit)
        //   block 2: %1 = call f();          // skeleton — no available_block
        //            %2 = add %0, %1;        // operand %1 is loop-internal
        //            br 1
        //   block 3: ret
        //
        // %1 has no available_block (Call is skeleton) AND is not a
        // param, so the operand check returns false → %2 is not
        // hoisted. Conservative default: None + non-param = unknown.
        let values = vec![int_value("a"), int_value("call"), int_value("c")];
        let blocks = vec![
            Block {
                id: BlockId(0),
                insts: vec![],
                term: Terminator::Br(BlockId(1)),
            },
            Block {
                id: BlockId(1),
                insts: vec![],
                term: Terminator::CondBr {
                    cond: Operand::ConstBool(true),
                    then_blk: BlockId(2),
                    else_blk: BlockId(3),
                },
            },
            Block {
                id: BlockId(2),
                insts: vec![
                    empty_inst(
                        ValueId(1),
                        InstKind::Call(torajs_core::ssa::FuncId(0), vec![]),
                    ),
                    empty_inst(
                        ValueId(2),
                        InstKind::BinOp(
                            BinOp::Add,
                            Operand::Value(ValueId(0)),
                            Operand::Value(ValueId(1)),
                        ),
                    ),
                ],
                term: Terminator::Br(BlockId(1)),
            },
            Block {
                id: BlockId(3),
                insts: vec![],
                term: Terminator::Ret(None),
            },
        ];
        let func = fixture_with_params(vec![ValueId(0)], values, blocks);
        let dom = DominatorTree::compute(&func);
        let mut eg = Egraph::new(func.values.len());
        let stats = remove_pure_and_optimize(&func, &dom, &mut eg);
        assert_eq!(
            stats.licm_hoisted, 0,
            "add depending on loop-internal call must not hoist"
        );
    }

    #[test]
    fn licm_no_loops_no_hoists() {
        // Straight-line program — no loops, no LICM. Sanity check that
        // the pass is a no-op when LoopAnalysis returns an empty list.
        let values = vec![int_value("a"), int_value("b"), int_value("c")];
        let blocks = vec![Block {
            id: BlockId(0),
            insts: vec![empty_inst(
                ValueId(2),
                InstKind::BinOp(
                    BinOp::Add,
                    Operand::Value(ValueId(0)),
                    Operand::Value(ValueId(1)),
                ),
            )],
            term: Terminator::Ret(None),
        }];
        let func = fixture_with_params(vec![ValueId(0), ValueId(1)], values, blocks);
        let dom = DominatorTree::compute(&func);
        let mut eg = Egraph::new(func.values.len());
        let stats = remove_pure_and_optimize(&func, &dom, &mut eg);
        assert_eq!(stats.licm_hoisted, 0);
    }
}
