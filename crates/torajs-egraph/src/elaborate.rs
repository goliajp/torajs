//! Phase 2 of the egraph pipeline — scoped elaboration.
//!
//! Walks the dominator tree of one `Function` in preorder, re-emitting
//! a new `Function` whose instructions reference the canonical
//! (UF-rooted, opt_value-resolved) representative for each operand.
//! GVN-redundant pure instructions whose canonical replacement is
//! already elaborated in an enclosing dominance scope are dropped.
//!
//! Algorithm (per Cranelift `wasmtime/cranelift/codegen/src/egraph/
//! elaborate.rs::Elaborator`):
//!
//! ```text
//! for blk in domtree.preorder():
//!     elaborated.push_scope()
//!     for inst in blk.insts:
//!         kind' = rewrite each Value(v) operand to Value(egraph.opt_value(v))
//!         if inst.result is Some(res):
//!             canon = egraph.opt_value(res)
//!             if canon != res and elaborated.contains(canon):
//!                 # redundant — already emitted in a dominator scope
//!                 continue
//!             elaborated.insert(canon, blk.id)
//!         emit(Inst { result, kind: kind', origin })
//!     blk.term' = rewrite Value operands in terminator
//!     # recurse to child blocks in domtree
//!     elaborated.pop_scope()
//! ```
//!
//! Scope nesting tracks dominance exactly — a value elaborated in a
//! dominator is visible (and reusable) in every descendant block, and
//! invisible to siblings. That's the single mechanism behind GVN, CSE
//! and (Phase 1+) LICM in this design — no per-pass machinery.
//!
//! **Phase 0** (this commit) ships round-trip-equivalent elaboration:
//! no rewrite rules fire in `optimize::remove_pure_and_optimize`, so
//! no `union`s happen, so `egraph.opt_value(v) == v` for every value
//! and every instruction emits unchanged. The conformance gate must
//! see identical TS-runtime behaviour with the egraph pass switched
//! on vs off (gated then by the since-retired `TORAJS_NEW_PIPELINE`
//! flag; the pipeline is now the only path).
//!
//! **LICM** (loop-invariant code motion) — hoisting pure values to a
//! shallower-loop-nest dominator block — is the next milestone. The
//! data plumbing for it is here (`LoopAnalysis` + `Cost::scale_for_depth`)
//! but the placement decision lands in Phase 1.

use crate::dominator::DominatorTree;
use crate::egraph::Egraph;
use crate::loop_analysis::LoopAnalysis;
use crate::scope_map::ScopedHashMap;
use torajs_core::ssa::{Block, BlockId, Function, Inst, InstKind, Operand, Terminator, ValueId};

/// Per-pass diagnostic counters. Reported to the integration layer for
/// logging / regression tracking; not used to gate correctness.
#[derive(Debug, Default, Clone, Copy)]
pub struct ElaborateStats {
    /// Instructions kept in the output function.
    pub emitted: u32,
    /// Instructions dropped because their result was GVN-unioned onto
    /// a value already elaborated in an enclosing dominance scope.
    pub gvn_skipped: u32,
    /// Instructions dropped because they were `InstKind::Identity(op)`
    /// placeholders emitted by rewrite rules — their result is aliased
    /// to `op` via `egraph.set_opt_value` and never reaches codegen.
    pub identity_dropped: u32,
    /// Number of blocks visited by the domtree preorder walk.
    pub blocks_visited: u32,
    /// Number of `Terminator::CondBr` folded to `Terminator::Br`
    /// because the condition canonicalised to a `ConstBool`
    /// (`if (true)` / `if (false)`) or both arms targeted the same
    /// block. Chunk 9b — P-OPT Phase 1 Cluster B.
    pub branches_folded: u32,
    /// Number of instructions whose emit position was relocated to a
    /// dominator block because Phase 1's LICM pass had marked their
    /// canonical e-class root with a new `available_block`. The inst
    /// counts toward `emitted` (it's still in the output), but this
    /// counter tracks how many went somewhere other than their
    /// source block. Chunk 13 — P-OPT Phase 2 LICM proper.
    pub licm_relocated: u32,
}

/// Run the elaboration pass.
///
/// The input `Function` is consumed by reference; its `values` table is
/// cloned verbatim into the output (Phase 0 mints no new SSA values,
/// only drops redundant ones). The returned `Function` has the same
/// block layout; reachable blocks keep their terminator shape (modulo
/// branch folding) and `insts` may shrink (via GVN drop) or have
/// operands canonicalised, while unreachable blocks are detached
/// (emptied, terminator pinned to `Unreachable`).
///
/// `_loop_info` is plumbed through unused in Phase 0; Phase 1 LICM
/// consumes it to decide hoist placement.
pub fn elaborate(
    func: &Function,
    dom: &DominatorTree,
    _loop_info: &LoopAnalysis,
    egraph: &mut Egraph,
) -> (Function, ElaborateStats) {
    let mut stats = ElaborateStats::default();
    let n = func.blocks.len();
    if n == 0 {
        return (func.clone(), stats);
    }

    // Invert idom edges → per-block list of immediate domtree children.
    // Sorted by `BlockId` so the DFS visits children in deterministic
    // order independent of how `idom` was built.
    let mut children: Vec<Vec<BlockId>> = vec![Vec::new(); n];
    for bi in 0..n {
        let bid = BlockId(bi as u32);
        if let Some(idom) = dom.immediate_dominator(bid) {
            let i = idom.0 as usize;
            if i < n {
                children[i].push(bid);
            }
        }
    }
    for kids in children.iter_mut() {
        kids.sort_by_key(|b| b.0);
    }

    // Pre-allocate output blocks so per-block fills land at the
    // matching index. Terminators get overwritten during the DFS
    // visit; insts start empty and grow as we emit.
    let mut out_blocks: Vec<Block> = func
        .blocks
        .iter()
        .map(|b| Block {
            id: b.id,
            insts: Vec::new(),
            term: b.term.clone(),
        })
        .collect();

    // Per-eclass elaboration scope. Key = canonical (opt_value) result;
    // value = the block at which it was emitted. Lookup answers "is
    // this value already available in an enclosing dominance scope?".
    let mut elaborated: ScopedHashMap<ValueId, BlockId> = ScopedHashMap::new();

    // Domtree DFS via an explicit stack of Enter/Exit markers — Exit
    // pops the scope opened on Enter. Children are pushed in reverse
    // so the lowest BlockId is popped first.
    enum Visit {
        Enter(BlockId),
        Exit,
    }
    let mut stack: Vec<Visit> = Vec::with_capacity(n * 2);
    stack.push(Visit::Enter(BlockId(0)));

    while let Some(v) = stack.pop() {
        match v {
            Visit::Enter(bid) => {
                let bi = bid.0 as usize;
                if bi >= n {
                    continue;
                }
                elaborated.push_scope();
                stats.blocks_visited += 1;

                let src = &func.blocks[bi];
                let mut new_insts: Vec<Inst> = Vec::with_capacity(src.insts.len());
                for inst in &src.insts {
                    // A non-Identity rewrite rule replaced this
                    // inst's kind during optimize (Shl for mul-pow2,
                    // FAdd for fmul-by-2) — emit the cheaper form.
                    // Operands re-canonicalise here because unions
                    // may have grown since the rule fired.
                    let base_kind = inst
                        .result
                        .and_then(|r| egraph.rewritten_kind_of(r).cloned())
                        .unwrap_or_else(|| inst.kind.clone());
                    let new_kind = canonicalize_operands(&base_kind, egraph);

                    // P-OPT Phase 1 — rewrite-rule `Identity(op)`
                    // placeholders are dropped here: alias the result
                    // to `op` via `set_opt_value` so downstream use-
                    // sites canonicalise straight to it, then skip
                    // emission so codegen never sees the marker.
                    if let InstKind::Identity(op) = &new_kind {
                        if let (Some(res), Operand::Value(target_v)) = (inst.result, op) {
                            egraph.set_opt_value(res, *target_v);
                        }
                        stats.identity_dropped += 1;
                        continue;
                    }

                    // LICM hoist redirect target — Phase 1 may have
                    // marked the canon's `available_block` to a strict
                    // dominator block (the loop's preheader slot). If
                    // so, redirect this inst's emission there instead
                    // of the source block. Otherwise emit normally in
                    // this block.
                    let mut emit_block = bid;
                    if let Some(res) = inst.result {
                        let canon = egraph.opt_value(res);
                        if canon != res && elaborated.contains_key(&canon) {
                            // Redundant: the canonical representative
                            // was emitted in a dominator scope; users
                            // already pick it up via opt_value
                            // canonicalisation, so skip this inst.
                            stats.gvn_skipped += 1;
                            continue;
                        }
                        if let Some(target) = egraph.available_block(canon) {
                            if target != bid && dom.dominates(target, bid) {
                                emit_block = target;
                                stats.licm_relocated += 1;
                            }
                        }
                        elaborated.insert(canon, emit_block);
                    }
                    let new_inst = Inst {
                        result: inst.result,
                        kind: new_kind,
                        origin: inst.origin,
                    };
                    if emit_block == bid {
                        new_insts.push(new_inst);
                    } else {
                        // Append into the dominator's already-finalised
                        // block tail — sits before its terminator, the
                        // textbook preheader slot.
                        let ti = emit_block.0 as usize;
                        if ti < out_blocks.len() {
                            out_blocks[ti].insts.push(new_inst);
                        } else {
                            // Defensive — should never trigger because
                            // dom.dominates would have returned false
                            // for an out-of-range target. Keep the
                            // inst in the source block as a fallback.
                            new_insts.push(new_inst);
                        }
                    }
                    stats.emitted += 1;
                }
                out_blocks[bi].insts = new_insts;
                out_blocks[bi].term = rewrite_terminator(&src.term, egraph, &mut stats);

                // Exit before children so it pops AFTER they finish.
                stack.push(Visit::Exit);
                for &child in children[bi].iter().rev() {
                    stack.push(Visit::Enter(child));
                }
            }
            Visit::Exit => {
                elaborated.pop_scope();
            }
        }
    }

    // Blocks unreachable from BlockId(0) are detached from the CFG —
    // no edge reaches them, so their contents can never execute.
    // Empty them and pin `Terminator::Unreachable` (the ctpop_idiom /
    // ctpop_range_sum detach idiom) instead of cloning their insts
    // verbatim: passes that run before elaboration walk the whole
    // block list (the inliner in particular replaces calls with
    // `InstKind::Identity` result bindings in any block), while the
    // egraph optimize/elaborate walks are domtree DFS and never
    // canonicalise dead-block contents — a verbatim clone leaks
    // un-elaborated Identity insts straight into liveness, which
    // rejects them by contract.
    for bi in 1..n {
        let bid = BlockId(bi as u32);
        if dom.immediate_dominator(bid).is_none() {
            // Never visited by the DFS: insts is still the empty
            // prealloc (LICM redirects only target dominators, which
            // are reachable by definition). Pin the terminator so the
            // detached block carries no operand uses either.
            out_blocks[bi].insts.clear();
            out_blocks[bi].term = Terminator::Unreachable;
        }
    }

    let func_out = Function {
        name: func.name.clone(),
        params: func.params.clone(),
        ret: func.ret,
        blocks: out_blocks,
        values: func.values.clone(),
        current_origin: func.current_origin,
    };
    (func_out, stats)
}

/// Rewrite every `Operand::Value(v)` operand of an `InstKind` through
/// `egraph.opt_value(v)`. Non-Value operands (constants) pass through.
///
/// This is the inverse-of-clone path: every `InstKind` variant carrying
/// an SSA value is enumerated explicitly so newly added variants don't
/// silently miss canonicalisation. Variants with no SSA-value payload
/// fall to the catch-all clone arm.
fn canonicalize_operands(kind: &InstKind, egraph: &mut Egraph) -> InstKind {
    match kind {
        InstKind::BinOp(op, l, r) => {
            let l = map_operand(l, egraph);
            let r = map_operand(r, egraph);
            InstKind::BinOp(*op, l, r)
        }
        InstKind::ICmp(p, l, r) => {
            let l = map_operand(l, egraph);
            let r = map_operand(r, egraph);
            InstKind::ICmp(*p, l, r)
        }
        InstKind::FCmp(p, l, r) => {
            let l = map_operand(l, egraph);
            let r = map_operand(r, egraph);
            InstKind::FCmp(*p, l, r)
        }
        InstKind::Call(fid, args) => {
            let new_args: Vec<Operand> = args.iter().map(|a| map_operand(a, egraph)).collect();
            InstKind::Call(*fid, new_args)
        }
        InstKind::CallIndirect(sig, ptr, args) => {
            let new_ptr = map_operand(ptr, egraph);
            let new_args: Vec<Operand> = args.iter().map(|a| map_operand(a, egraph)).collect();
            InstKind::CallIndirect(*sig, new_ptr, new_args)
        }
        InstKind::Load(ty, addr, off) => InstKind::Load(*ty, map_operand(addr, egraph), *off),
        InstKind::Store(val, addr, off) => {
            InstKind::Store(map_operand(val, egraph), map_operand(addr, egraph), *off)
        }
        InstKind::LoadDyn(ty, addr, off) => {
            InstKind::LoadDyn(*ty, map_operand(addr, egraph), map_operand(off, egraph))
        }
        InstKind::StoreDyn(val, addr, off) => InstKind::StoreDyn(
            map_operand(val, egraph),
            map_operand(addr, egraph),
            map_operand(off, egraph),
        ),
        InstKind::LoadDynScaled8(ty, addr, idx) => {
            InstKind::LoadDynScaled8(*ty, map_operand(addr, egraph), map_operand(idx, egraph))
        }
        InstKind::LoadU8Dyn(addr, idx) => {
            InstKind::LoadU8Dyn(map_operand(addr, egraph), map_operand(idx, egraph))
        }
        InstKind::StoreDynScaled8(val, addr, idx) => InstKind::StoreDynScaled8(
            map_operand(val, egraph),
            map_operand(addr, egraph),
            map_operand(idx, egraph),
        ),
        InstKind::SiToFp(v) => InstKind::SiToFp(map_operand(v, egraph)),
        InstKind::FpToSi(v) => InstKind::FpToSi(map_operand(v, egraph)),
        InstKind::ZExtBoolToI64(v) => InstKind::ZExtBoolToI64(map_operand(v, egraph)),
        InstKind::ZExtI32ToI64(v) => InstKind::ZExtI32ToI64(map_operand(v, egraph)),
        InstKind::Identity(v) => InstKind::Identity(map_operand(v, egraph)),
        InstKind::Neg(v) => InstKind::Neg(map_operand(v, egraph)),
        InstKind::Ctpop(v) => InstKind::Ctpop(map_operand(v, egraph)),
        InstKind::Copy(_, _) => unreachable!(
            "InstKind::Copy reached the egraph elaborator — mem2reg's φ \
             destruction introduces Copy only after the egraph pass"
        ),
        InstKind::Select(_, _, _, _) => unreachable!(
            "InstKind::Select reached the egraph elaborator — select \
             formation introduces Select only after the egraph pass"
        ),
        InstKind::CtpopRangeSum(_, _, _) => unreachable!(
            "InstKind::CtpopRangeSum reached the egraph elaborator — \
             ctpop-range-sum formation introduces it only after the \
             egraph pass"
        ),
        InstKind::BitCastF64ToI64(v) => InstKind::BitCastF64ToI64(map_operand(v, egraph)),
        InstKind::BitCastI64ToF64(v) => InstKind::BitCastI64ToF64(map_operand(v, egraph)),
        InstKind::IntToPtr(v) => InstKind::IntToPtr(map_operand(v, egraph)),
        InstKind::PtrToInt(v) => InstKind::PtrToInt(map_operand(v, egraph)),
        InstKind::TruncI64ToBool(v) => InstKind::TruncI64ToBool(map_operand(v, egraph)),
        // No SSA Value payload — just clone.
        InstKind::Alloca(_)
        | InstKind::AllocaBytes(_)
        | InstKind::StringRef(_)
        | InstKind::StaticStrRef(_)
        | InstKind::GlobalRef(_)
        | InstKind::FnAddr(_) => kind.clone(),
    }
}

/// Canonicalise SSA value operands in a `Terminator`. CondBr `cond`
/// and Ret value are subject to rewrite; target BlockIds stay as-is.
///
/// Chunk 9b — Cluster B branch folding: when the canonicalised cond
/// is `ConstBool(true)`/`ConstBool(false)`, or when both arms target
/// the same block, the `CondBr` is rewritten to a single-target `Br`
/// and `stats.branches_folded` increments. The block that the fold
/// drops on the floor stays in the layout this round (the domtree was
/// computed before the fold, so the DFS still visits and elaborates
/// it); blocks that were already unreachable coming in are detached
/// by the loop after the DFS.
fn rewrite_terminator(
    term: &Terminator,
    egraph: &mut Egraph,
    stats: &mut ElaborateStats,
) -> Terminator {
    match term {
        Terminator::Br(b) => Terminator::Br(*b),
        Terminator::CondBr {
            cond,
            then_blk,
            else_blk,
        } => {
            let cond_canon = map_operand(cond, egraph);
            match cond_canon {
                Operand::ConstBool(true) => {
                    stats.branches_folded += 1;
                    Terminator::Br(*then_blk)
                }
                Operand::ConstBool(false) => {
                    stats.branches_folded += 1;
                    Terminator::Br(*else_blk)
                }
                _ if then_blk == else_blk => {
                    stats.branches_folded += 1;
                    Terminator::Br(*then_blk)
                }
                _ => Terminator::CondBr {
                    cond: cond_canon,
                    then_blk: *then_blk,
                    else_blk: *else_blk,
                },
            }
        }
        Terminator::Ret(Some(op)) => Terminator::Ret(Some(map_operand(op, egraph))),
        Terminator::Ret(None) => Terminator::Ret(None),
        Terminator::Unreachable => Terminator::Unreachable,
    }
}

#[inline]
fn map_operand(op: &Operand, egraph: &mut Egraph) -> Operand {
    match op {
        Operand::Value(v) => {
            // P-OPT Phase 1 chunk 9a-3 — const-folded values
            // propagate as literal operands. Falls back to the
            // canonical Value when no rule has wired a constant.
            let canon = egraph.opt_value(*v);
            if let Some(const_op) = egraph.const_of(canon) {
                const_op
            } else {
                Operand::Value(canon)
            }
        }
        other => *other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::optimize::remove_pure_and_optimize;
    use torajs_core::ssa::{
        BinOp, Block, BlockId, Function, IPred, Inst, Operand, Terminator, Type, ValueId, ValueInfo,
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
    fn vinfo(name: &str, ty: Type) -> ValueInfo {
        ValueInfo {
            ty,
            name: Some(name.into()),
        }
    }
    fn func(values: Vec<ValueInfo>, blocks: Vec<Block>) -> Function {
        Function {
            name: "test".into(),
            params: vec![],
            ret: Type::Void,
            blocks,
            values,
            current_origin: None,
        }
    }

    fn func_with_params(
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
    fn empty_function_round_trips_safely() {
        let f = func(vec![], vec![]);
        let dom = DominatorTree::compute(&f);
        let lp = LoopAnalysis::compute(&f, &dom);
        let mut eg = Egraph::new(0);
        let (out, stats) = elaborate(&f, &dom, &lp, &mut eg);
        assert_eq!(out.blocks.len(), 0);
        assert_eq!(stats.emitted, 0);
        assert_eq!(stats.blocks_visited, 0);
    }

    #[test]
    fn round_trip_when_no_unions() {
        // Two distinct adds + a void call + ret. No rewrites fire; the
        // output function must mirror the input exactly (same inst
        // count, same kinds, same terminator).
        let values = vec![
            vinfo("a", Type::I64),
            vinfo("b", Type::I64),
            vinfo("c", Type::I64),
            vinfo("d", Type::I64),
        ];
        let blocks = vec![Block {
            id: BlockId(0),
            insts: vec![
                val_inst(
                    ValueId(2),
                    InstKind::BinOp(BinOp::Add, Operand::ConstI64(1), Operand::ConstI64(2)),
                ),
                val_inst(
                    ValueId(3),
                    InstKind::BinOp(BinOp::Sub, Operand::ConstI64(9), Operand::ConstI64(4)),
                ),
                void_inst(InstKind::Call(torajs_core::ssa::FuncId(0), vec![])),
            ],
            term: Terminator::Ret(None),
        }];
        let f = func(values, blocks);
        let dom = DominatorTree::compute(&f);
        let lp = LoopAnalysis::compute(&f, &dom);
        let mut eg = Egraph::new(f.values.len());
        let (out, stats) = elaborate(&f, &dom, &lp, &mut eg);
        assert_eq!(out.blocks.len(), 1);
        assert_eq!(out.blocks[0].insts.len(), 3);
        assert_eq!(stats.emitted, 3);
        assert_eq!(stats.gvn_skipped, 0);
        assert_eq!(stats.blocks_visited, 1);
    }

    #[test]
    fn gvn_dedup_drops_second_add_in_same_block() {
        // Pre-seed the egraph with the GVN union the optimize phase
        // would produce: two identical pure adds in one block.
        // Elaborate must keep the first and drop the second.
        // (Use Value+Value operands — both-const operands would
        // const-fold to Identity, bypassing the GVN dedup path.)
        let values = vec![
            vinfo("a", Type::I64),
            vinfo("b", Type::I64),
            vinfo("c", Type::I64),
            vinfo("d", Type::I64),
        ];
        let blocks = vec![Block {
            id: BlockId(0),
            insts: vec![
                val_inst(
                    ValueId(2),
                    InstKind::BinOp(
                        BinOp::Add,
                        Operand::Value(ValueId(0)),
                        Operand::Value(ValueId(1)),
                    ),
                ),
                val_inst(
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
        let f = func(values, blocks);
        let dom = DominatorTree::compute(&f);
        let lp = LoopAnalysis::compute(&f, &dom);
        let mut eg = Egraph::new(f.values.len());
        // Run optimize first so the GVN union is recorded.
        let _ = remove_pure_and_optimize(&f, &dom, &mut eg);
        let (out, stats) = elaborate(&f, &dom, &lp, &mut eg);
        assert_eq!(out.blocks.len(), 1);
        assert_eq!(
            out.blocks[0].insts.len(),
            1,
            "second redundant add must be dropped"
        );
        assert_eq!(stats.emitted, 1);
        assert_eq!(stats.gvn_skipped, 1);
    }

    #[test]
    fn operand_canonicalisation_follows_union() {
        // Pre-union %0 ↔ %1 (lower wins). A downstream BinOp(%1, %3)
        // must elaborate to BinOp(%0, %3).
        let values = vec![
            vinfo("p0", Type::I64),
            vinfo("p1", Type::I64),
            vinfo("r2", Type::I64),
            vinfo("p3", Type::I64),
        ];
        let blocks = vec![Block {
            id: BlockId(0),
            insts: vec![val_inst(
                ValueId(2),
                InstKind::BinOp(
                    BinOp::Add,
                    Operand::Value(ValueId(1)),
                    Operand::Value(ValueId(3)),
                ),
            )],
            term: Terminator::Ret(Some(Operand::Value(ValueId(2)))),
        }];
        let f = func(values, blocks);
        let dom = DominatorTree::compute(&f);
        let lp = LoopAnalysis::compute(&f, &dom);
        let mut eg = Egraph::new(f.values.len());
        assert!(eg.union(ValueId(0), ValueId(1)));
        let (out, _) = elaborate(&f, &dom, &lp, &mut eg);
        assert_eq!(out.blocks[0].insts.len(), 1);
        match &out.blocks[0].insts[0].kind {
            InstKind::BinOp(BinOp::Add, l, r) => {
                assert_eq!(*l, Operand::Value(ValueId(0)));
                assert_eq!(*r, Operand::Value(ValueId(3)));
            }
            other => panic!("expected BinOp::Add, got {other:?}"),
        }
    }

    #[test]
    fn terminator_value_operands_canonicalised() {
        // Ret(Value(%1)) with %0 ↔ %1 union should rewrite to Ret(%0).
        // CondBr cond Operand::Value also rewrites.
        let values = vec![
            vinfo("a", Type::I64),
            vinfo("b", Type::I64),
            vinfo("c", Type::Bool),
        ];
        let blocks = vec![
            Block {
                id: BlockId(0),
                insts: vec![],
                term: Terminator::CondBr {
                    cond: Operand::Value(ValueId(2)),
                    then_blk: BlockId(1),
                    else_blk: BlockId(2),
                },
            },
            Block {
                id: BlockId(1),
                insts: vec![],
                term: Terminator::Ret(Some(Operand::Value(ValueId(1)))),
            },
            Block {
                id: BlockId(2),
                insts: vec![],
                term: Terminator::Ret(None),
            },
        ];
        let f = func(values, blocks);
        let dom = DominatorTree::compute(&f);
        let lp = LoopAnalysis::compute(&f, &dom);
        let mut eg = Egraph::new(f.values.len());
        assert!(eg.union(ValueId(0), ValueId(1)));
        let (out, _) = elaborate(&f, &dom, &lp, &mut eg);
        // Ret in block 1 picks up the union root.
        match &out.blocks[1].term {
            Terminator::Ret(Some(Operand::Value(v))) => assert_eq!(*v, ValueId(0)),
            other => panic!("expected Ret(Value(0)), got {other:?}"),
        }
        // CondBr cond is not unioned (no rewrite); just round-trips.
        match &out.blocks[0].term {
            Terminator::CondBr { cond, .. } => assert_eq!(*cond, Operand::Value(ValueId(2))),
            other => panic!("expected CondBr, got {other:?}"),
        }
    }

    #[test]
    fn skeleton_inst_stays_in_layout() {
        // A void call (skeleton) is never deduped even when textually
        // identical to a prior one — elaborate must emit both.
        let values = vec![vinfo("r", Type::I64)];
        let blocks = vec![Block {
            id: BlockId(0),
            insts: vec![
                void_inst(InstKind::Call(torajs_core::ssa::FuncId(0), vec![])),
                void_inst(InstKind::Call(torajs_core::ssa::FuncId(0), vec![])),
                val_inst(
                    ValueId(0),
                    InstKind::ICmp(IPred::Eq, Operand::ConstI64(0), Operand::ConstI64(0)),
                ),
            ],
            term: Terminator::Ret(None),
        }];
        let f = func(values, blocks);
        let dom = DominatorTree::compute(&f);
        let lp = LoopAnalysis::compute(&f, &dom);
        let mut eg = Egraph::new(f.values.len());
        let (out, stats) = elaborate(&f, &dom, &lp, &mut eg);
        assert_eq!(out.blocks[0].insts.len(), 3);
        assert_eq!(stats.emitted, 3);
        assert_eq!(stats.gvn_skipped, 0);
    }

    #[test]
    fn multi_block_domtree_dfs_visits_all_reachable() {
        // 0 → {1, 2}, 1 → 3, 2 → 3 (diamond)
        // Each block has one trivial add. Without unions all 4 emit.
        let values = vec![
            vinfo("a", Type::I64),
            vinfo("b", Type::I64),
            vinfo("c", Type::I64),
            vinfo("d", Type::I64),
        ];
        let mk_add = |res: u32, off: i64| {
            val_inst(
                ValueId(res),
                InstKind::BinOp(
                    BinOp::Add,
                    Operand::ConstI64(off),
                    Operand::ConstI64(off + 1),
                ),
            )
        };
        let blocks = vec![
            Block {
                id: BlockId(0),
                insts: vec![mk_add(0, 1)],
                term: Terminator::CondBr {
                    cond: Operand::ConstBool(true),
                    then_blk: BlockId(1),
                    else_blk: BlockId(2),
                },
            },
            Block {
                id: BlockId(1),
                insts: vec![mk_add(1, 5)],
                term: Terminator::Br(BlockId(3)),
            },
            Block {
                id: BlockId(2),
                insts: vec![mk_add(2, 9)],
                term: Terminator::Br(BlockId(3)),
            },
            Block {
                id: BlockId(3),
                insts: vec![mk_add(3, 13)],
                term: Terminator::Ret(None),
            },
        ];
        let f = func(values, blocks);
        let dom = DominatorTree::compute(&f);
        let lp = LoopAnalysis::compute(&f, &dom);
        let mut eg = Egraph::new(f.values.len());
        let (out, stats) = elaborate(&f, &dom, &lp, &mut eg);
        assert_eq!(stats.blocks_visited, 4);
        assert_eq!(stats.emitted, 4);
        assert_eq!(stats.gvn_skipped, 0);
        // Layout preserved — every block has exactly one inst.
        for b in &out.blocks {
            assert_eq!(b.insts.len(), 1);
        }
    }

    #[test]
    fn identity_inst_dropped_and_result_aliased_to_operand() {
        // Setup mimics a Phase 1 rewrite rule that turned `%2 = add %0, 0`
        // into `%2 = id %0`. Elaborate must drop the Identity inst and
        // alias %2's opt_value onto %0, so a downstream use `%3 = sub %2, %1`
        // canonicalises to `sub %0, %1`.
        let values = vec![
            vinfo("p0", Type::I64),
            vinfo("p1", Type::I64),
            vinfo("r2", Type::I64),
            vinfo("r3", Type::I64),
        ];
        let blocks = vec![Block {
            id: BlockId(0),
            insts: vec![
                val_inst(ValueId(2), InstKind::Identity(Operand::Value(ValueId(0)))),
                val_inst(
                    ValueId(3),
                    InstKind::BinOp(
                        BinOp::Sub,
                        Operand::Value(ValueId(2)),
                        Operand::Value(ValueId(1)),
                    ),
                ),
            ],
            term: Terminator::Ret(Some(Operand::Value(ValueId(3)))),
        }];
        let f = func(values, blocks);
        let dom = DominatorTree::compute(&f);
        let lp = LoopAnalysis::compute(&f, &dom);
        let mut eg = Egraph::new(f.values.len());
        let (out, stats) = elaborate(&f, &dom, &lp, &mut eg);
        assert_eq!(
            stats.identity_dropped, 1,
            "Identity placeholder must be counted as dropped"
        );
        // Only the Sub survives in the emitted block — Identity was skipped.
        assert_eq!(out.blocks[0].insts.len(), 1);
        match &out.blocks[0].insts[0].kind {
            InstKind::BinOp(BinOp::Sub, l, r) => {
                assert_eq!(
                    *l,
                    Operand::Value(ValueId(0)),
                    "Sub LHS must be canonicalised through Identity alias"
                );
                assert_eq!(*r, Operand::Value(ValueId(1)));
            }
            other => panic!("expected BinOp::Sub, got {other:?}"),
        }
        // opt_value(%2) now resolves to %0 because Identity wired the alias.
        assert_eq!(eg.opt_value(ValueId(2)), ValueId(0));
    }

    #[test]
    fn const_fold_propagates_to_downstream_operand() {
        // %2 = sub %0 %0   # SubSelf fires (via optimize.rs) →
        //                   set_const(%2, ConstI64(0))
        // %3 = add %2 %1   # elaborate must canonicalise %2's
        //                   operand slot to ConstI64(0)
        let values = vec![
            vinfo("a", Type::I64),
            vinfo("b", Type::I64),
            vinfo("r2", Type::I64),
            vinfo("r3", Type::I64),
        ];
        let blocks = vec![Block {
            id: BlockId(0),
            insts: vec![
                val_inst(
                    ValueId(2),
                    InstKind::BinOp(
                        BinOp::Sub,
                        Operand::Value(ValueId(0)),
                        Operand::Value(ValueId(0)),
                    ),
                ),
                val_inst(
                    ValueId(3),
                    InstKind::BinOp(
                        BinOp::Add,
                        Operand::Value(ValueId(2)),
                        Operand::Value(ValueId(1)),
                    ),
                ),
            ],
            term: Terminator::Ret(Some(Operand::Value(ValueId(3)))),
        }];
        let f = func(values, blocks);
        let dom = DominatorTree::compute(&f);
        let lp = LoopAnalysis::compute(&f, &dom);
        let mut eg = Egraph::new(f.values.len());
        let _ = remove_pure_and_optimize(&f, &dom, &mut eg);
        let (out, _) = elaborate(&f, &dom, &lp, &mut eg);
        // The Sub itself stays in layout (no Identity inst in the
        // input IR; const propagation only fires on use-sites).
        // The Add's LHS must canonicalise to ConstI64(0).
        let add_inst = out.blocks[0]
            .insts
            .iter()
            .find(|i| matches!(i.kind, InstKind::BinOp(BinOp::Add, _, _)))
            .expect("Add inst must survive elaborate");
        match &add_inst.kind {
            InstKind::BinOp(BinOp::Add, lhs, rhs) => {
                assert_eq!(
                    *lhs,
                    Operand::ConstI64(0),
                    "SubSelf's result must propagate as ConstI64(0) into use-sites"
                );
                assert_eq!(*rhs, Operand::Value(ValueId(1)));
            }
            other => panic!("expected BinOp::Add, got {other:?}"),
        }
    }

    #[test]
    fn const_true_cond_folds_to_unconditional_then_branch() {
        // `if (true) goto B1 else goto B2` → `goto B1`
        let values = vec![vinfo("dummy", Type::I64)];
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
                insts: vec![],
                term: Terminator::Ret(None),
            },
            Block {
                id: BlockId(2),
                insts: vec![],
                term: Terminator::Ret(None),
            },
        ];
        let f = func(values, blocks);
        let dom = DominatorTree::compute(&f);
        let lp = LoopAnalysis::compute(&f, &dom);
        let mut eg = Egraph::new(f.values.len());
        let (out, stats) = elaborate(&f, &dom, &lp, &mut eg);
        assert_eq!(stats.branches_folded, 1);
        match out.blocks[0].term {
            Terminator::Br(b) => assert_eq!(b, BlockId(1)),
            ref other => panic!("expected Br(1), got {other:?}"),
        }
    }

    #[test]
    fn const_false_cond_folds_to_unconditional_else_branch() {
        // `if (false) goto B1 else goto B2` → `goto B2`
        let values = vec![vinfo("dummy", Type::I64)];
        let blocks = vec![
            Block {
                id: BlockId(0),
                insts: vec![],
                term: Terminator::CondBr {
                    cond: Operand::ConstBool(false),
                    then_blk: BlockId(1),
                    else_blk: BlockId(2),
                },
            },
            Block {
                id: BlockId(1),
                insts: vec![],
                term: Terminator::Ret(None),
            },
            Block {
                id: BlockId(2),
                insts: vec![],
                term: Terminator::Ret(None),
            },
        ];
        let f = func(values, blocks);
        let dom = DominatorTree::compute(&f);
        let lp = LoopAnalysis::compute(&f, &dom);
        let mut eg = Egraph::new(f.values.len());
        let (out, stats) = elaborate(&f, &dom, &lp, &mut eg);
        assert_eq!(stats.branches_folded, 1);
        match out.blocks[0].term {
            Terminator::Br(b) => assert_eq!(b, BlockId(2)),
            ref other => panic!("expected Br(2), got {other:?}"),
        }
    }

    #[test]
    fn same_target_arms_fold_to_unconditional_branch() {
        // `if (cond) goto B1 else goto B1` → `goto B1` (regardless of cond)
        let values = vec![vinfo("cond", Type::Bool)];
        let blocks = vec![
            Block {
                id: BlockId(0),
                insts: vec![],
                term: Terminator::CondBr {
                    cond: Operand::Value(ValueId(0)),
                    then_blk: BlockId(1),
                    else_blk: BlockId(1),
                },
            },
            Block {
                id: BlockId(1),
                insts: vec![],
                term: Terminator::Ret(None),
            },
        ];
        let f = func(values, blocks);
        let dom = DominatorTree::compute(&f);
        let lp = LoopAnalysis::compute(&f, &dom);
        let mut eg = Egraph::new(f.values.len());
        let (out, stats) = elaborate(&f, &dom, &lp, &mut eg);
        assert_eq!(stats.branches_folded, 1);
        match out.blocks[0].term {
            Terminator::Br(b) => assert_eq!(b, BlockId(1)),
            ref other => panic!("expected Br(1), got {other:?}"),
        }
    }

    #[test]
    fn unreachable_block_detached() {
        // Block 0 → ret. Block 1 is unreachable: its contents can
        // never execute, so elaboration empties it and pins the
        // terminator to Unreachable. Un-elaborated insts (an inliner-
        // spliced Identity in particular) must never survive in a
        // dead block — liveness rejects Identity by contract.
        let values = vec![vinfo("a", Type::I64), vinfo("b", Type::I64)];
        let blocks = vec![
            Block {
                id: BlockId(0),
                insts: vec![],
                term: Terminator::Ret(None),
            },
            Block {
                id: BlockId(1),
                insts: vec![
                    val_inst(
                        ValueId(0),
                        InstKind::BinOp(BinOp::Add, Operand::ConstI64(1), Operand::ConstI64(2)),
                    ),
                    val_inst(ValueId(1), InstKind::Identity(Operand::Value(ValueId(0)))),
                ],
                term: Terminator::Br(BlockId(0)),
            },
        ];
        let f = func(values, blocks);
        let dom = DominatorTree::compute(&f);
        let lp = LoopAnalysis::compute(&f, &dom);
        let mut eg = Egraph::new(f.values.len());
        let (out, stats) = elaborate(&f, &dom, &lp, &mut eg);
        // Block 1 is unreachable from entry — detached.
        assert!(out.blocks[1].insts.is_empty());
        assert!(matches!(out.blocks[1].term, Terminator::Unreachable));
        // Only block 0 was actually walked by the DFS.
        assert_eq!(stats.blocks_visited, 1);
    }

    #[test]
    fn licm_relocates_invariant_add_to_preheader_block() {
        // End-to-end LICM round-trip:
        //
        //   params: %0 = a, %1 = b
        //   block 0 (preheader slot) → br block 1
        //   block 1 (header)         → condbr → block 2, block 3
        //   block 2 (body)           : %2 = add %0, %1; br block 1
        //   block 3 (exit)           : ret
        //
        // After remove_pure_and_optimize, %2 has its available_block
        // marked as block 0 (idom of header). Elaboration must then
        // emit the `%2 = add %0, %1` instruction into block 0 — the
        // preheader slot — and leave block 2 with no insts.
        let values = vec![
            vinfo("a", Type::I64),
            vinfo("b", Type::I64),
            vinfo("c", Type::I64),
        ];
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
                insts: vec![val_inst(
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
        let f = func_with_params(vec![ValueId(0), ValueId(1)], values, blocks);
        let dom = DominatorTree::compute(&f);
        let mut eg = Egraph::new(f.values.len());
        let opt_stats = remove_pure_and_optimize(&f, &dom, &mut eg);
        assert_eq!(
            opt_stats.licm_hoisted, 1,
            "Phase 1 marks the add for hoisting"
        );

        let lp = LoopAnalysis::compute(&f, &dom);
        let (out, stats) = elaborate(&f, &dom, &lp, &mut eg);
        // Phase 2 must report exactly one relocation.
        assert_eq!(
            stats.licm_relocated, 1,
            "elaborate relocates the hoisted inst"
        );
        // The add now lives in block 0 (preheader slot).
        assert_eq!(
            out.blocks[0].insts.len(),
            1,
            "block 0 holds the hoisted add"
        );
        assert_eq!(out.blocks[0].insts[0].result, Some(ValueId(2)));
        // Block 2 (loop body) is empty now — its only inst was hoisted.
        assert_eq!(
            out.blocks[2].insts.len(),
            0,
            "block 2 loses the hoisted inst"
        );
    }

    #[test]
    fn licm_relocated_counter_zero_in_loopless_function() {
        // Straight-line program — no hoisting possible. Verify
        // licm_relocated stays at zero so the counter does not fire
        // spuriously on non-loop functions.
        let values = vec![
            vinfo("a", Type::I64),
            vinfo("b", Type::I64),
            vinfo("c", Type::I64),
        ];
        let blocks = vec![Block {
            id: BlockId(0),
            insts: vec![val_inst(
                ValueId(2),
                InstKind::BinOp(
                    BinOp::Add,
                    Operand::Value(ValueId(0)),
                    Operand::Value(ValueId(1)),
                ),
            )],
            term: Terminator::Ret(None),
        }];
        let f = func_with_params(vec![ValueId(0), ValueId(1)], values, blocks);
        let dom = DominatorTree::compute(&f);
        let mut eg = Egraph::new(f.values.len());
        let _ = remove_pure_and_optimize(&f, &dom, &mut eg);
        let lp = LoopAnalysis::compute(&f, &dom);
        let (out, stats) = elaborate(&f, &dom, &lp, &mut eg);
        assert_eq!(stats.licm_relocated, 0);
        // The add stays in block 0 (its original source block — which
        // also happens to be the only block).
        assert_eq!(out.blocks[0].insts.len(), 1);
    }
}
