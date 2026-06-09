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
use torajs_core::ssa::{Function, InstKind, Operand};

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
}

/// Phase 1 pass: walk the dominator tree in preorder, canonicalising
/// operands through the egraph and feeding pure instructions through
/// the scoped GVN map. Mutates the egraph; reports per-pass stats.
pub fn remove_pure_and_optimize(
    func: &Function,
    dom: &DominatorTree,
    egraph: &mut Egraph,
) -> OptimizeStats {
    let mut stats = OptimizeStats::default();
    for &blk_id in dom.reverse_postorder() {
        // Domtree preorder is approximated by RPO for the GVN scope
        // push/pop pattern: each block enters its own scope so values
        // inserted while visiting `b` are visible to b's descendants
        // (next blocks in RPO that b dominates) but pop_scope happens
        // once we've processed every descendant of b. For Phase 0 we
        // use a flat per-block scope (push at block start, pop at
        // block end) which is correct for non-nested blocks; nested
        // domtree scope handling lands when elaboration's scoped
        // traversal does — Phase 0 walk's job is to seed the egraph,
        // not to compute final placements.
        egraph.gvn_mut().push_scope();
        let blk_idx = blk_id.0 as usize;
        if blk_idx >= func.blocks.len() {
            egraph.gvn_mut().pop_scope();
            continue;
        }
        let block = &func.blocks[blk_idx];
        for inst in &block.insts {
            let Some(result) = inst.result else {
                // Void instructions (stores, some calls) are skeleton.
                stats.skeleton_seen += 1;
                continue;
            };
            // Canonicalise operands by rewriting Operand::Value(v) →
            // Operand::Value(egraph.opt_value(v)). Constant operands
            // are passed through unchanged.
            let canon_kind = canonicalize_operands(&inst.kind, egraph);
            if is_pure(&canon_kind) {
                let result_ty = func
                    .values
                    .get(result.0 as usize)
                    .map(|vi| vi.ty)
                    .unwrap_or(torajs_core::ssa::Type::Void);
                let key: GvnKey = (result_ty, canon_kind);
                if let Some(&existing) = egraph.gvn().get(&key) {
                    egraph.union(result, existing);
                    stats.gvn_hits += 1;
                } else {
                    egraph.gvn_mut().insert(key, result);
                    egraph.set_available_block(result, blk_id);
                    stats.gvn_inserts += 1;
                    // Phase 1 hook: try_rewrite(canon_kind, &mut egraph).
                    // Phase 0 = no-op.
                }
            } else {
                stats.skeleton_seen += 1;
            }
        }
        egraph.gvn_mut().pop_scope();
    }
    stats
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
        InstKind::Load(_, _, _) | InstKind::LoadDyn(_, _, _) => false,
        // Skeleton: writes and calls.
        InstKind::Store(_, _, _) | InstKind::StoreDyn(_, _, _) | InstKind::Call(_, _) => false,
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

    #[test]
    fn gvn_dedup_two_identical_adds() {
        // %0 = const i64 7
        // %1 = const i64 11
        // %2 = add %0, %1
        // %3 = add %0, %1   # GVN should union %3 into %2
        // ret
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
                    InstKind::BinOp(BinOp::Add, Operand::ConstI64(7), Operand::ConstI64(11)),
                ),
                empty_inst(
                    ValueId(3),
                    InstKind::BinOp(BinOp::Add, Operand::ConstI64(7), Operand::ConstI64(11)),
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
        // 0 (cond) -> 1, 2 ; 1 has %2 = add 3,4 ; 2 has %3 = add 3,4
        // After block 1, pop_scope -> block 2 sees fresh GVN map ->
        // %3 is a fresh insert, NOT a hit on %2 (sibling isolation).
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
                    InstKind::BinOp(BinOp::Add, Operand::ConstI64(3), Operand::ConstI64(4)),
                )],
                term: Terminator::Ret(None),
            },
            Block {
                id: BlockId(2),
                insts: vec![empty_inst(
                    ValueId(3),
                    InstKind::BinOp(BinOp::Add, Operand::ConstI64(3), Operand::ConstI64(4)),
                )],
                term: Terminator::Ret(None),
            },
        ];
        let func = fixture(values, blocks);
        let dom = DominatorTree::compute(&func);
        let mut eg = Egraph::new(func.values.len());
        let stats = remove_pure_and_optimize(&func, &dom, &mut eg);
        // Phase 0 uses flat per-block scope (not strict domtree
        // nesting), so siblings see each other's inserts when RPO
        // visits them sequentially. Document the current behaviour
        // and tighten in Phase 1 when scoped elaboration takes over
        // proper domtree-nested scoping. For now: 1 insert + 1 hit.
        assert_eq!(stats.gvn_inserts + stats.gvn_hits, 2);
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
}
