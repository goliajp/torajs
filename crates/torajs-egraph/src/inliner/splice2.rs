//! Phase 2.0a splice2 — multi-block leaf SSA mutation.
//!
//! Extends Phase 1.0a's `inline_single_block_leaf` to multi-block leaf
//! callees: callees may have internal `Br` / `CondBr` control flow
//! (if-else / loop bodies) as long as they remain true leaves (no
//! nested `Call` / `CallIndirect`) and contain at most one
//! `Ret(Some(_))` value-bearing terminator. The latter constraint is
//! lifted in Phase 2.0b via Φ-node insertion at the continuation
//! block.
//!
//! Algorithm (the textbook block-split inlining used by every
//! production compiler — LLVM `Transforms/Utils/InlineFunction.cpp` +
//! Go `cmd/compile/internal/inline/inl.go::inlcalls`):
//!
//! 1. Allocate fresh `BlockId`s for every callee block plus one
//!    continuation block to hold the caller's post-call instructions.
//! 2. Build operand mapping (callee params → caller-supplied `args`,
//!    other callee values → fresh `caller.values` entries).
//! 3. Split the caller block at the call site: keep pre-call insts,
//!    redirect terminator to the (remapped) callee entry block, move
//!    post-call insts + the original terminator to the new
//!    continuation block.
//! 4. Clone each callee block into `caller.blocks` with the
//!    remapped `BlockId`. Rewrite operands via the value map; rewrite
//!    terminator BlockId edges via the block map; convert
//!    `Ret(None)` → `Br(continuation)`, and `Ret(Some(v))` →
//!    `Identity` binding `result_value` followed by `Br(continuation)`.
//!
//! Phase 2.0a narrow scope (each enforced via `SpliceMultiError`):
//! * `callee.blocks.len() ≥ 1` (declarations are out).
//! * Callee body free of `Call` / `CallIndirect`.
//! * At most one `Ret(Some(_))` terminator (`Ret(None)` may appear
//!   multiple times; each branches to continuation without binding).
//! * Ret-shape ↔ `result_value` agree (both Some or both None).
//! * `args.len() == callee.params.len()`.
//! * Caller block + site indices in bounds + inst at site is a `Call`.
//! * Terminators restricted to `Br` / `CondBr` / `Ret`; an
//!   `Unreachable` in callee is rejected with `UnsupportedTerminator`
//!   (rare in production; Phase 2 follow-up if it surfaces).

use super::rewrite::{rewrite_inst_kind, rewrite_operand};
use std::collections::HashMap;
use torajs_core::ssa::{Block, BlockId, Function, Inst, InstKind, Operand, Terminator, ValueId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpliceMultiError {
    BlockIdxOob,
    SiteIdxOob,
    NotCallSite,
    ArityMismatch,
    /// Callee declaration (no blocks).
    EmptyCallee,
    /// Callee body contains nested `Call` or `CallIndirect`.
    NotLeaf,
    /// Callee has more than one `Ret(Some(_))` value-bearing
    /// terminator. Phase 2.0a out of scope; Phase 2.0b lifts this via
    /// Φ-node insertion at the continuation block.
    MultipleRetWithValue,
    /// Callee terminator includes `Unreachable` or other non-Ret /
    /// non-Br shapes that Phase 2.0a does not yet handle.
    UnsupportedTerminator,
    /// Ret-shape mismatch (value-bearing Ret without `result_value`,
    /// or `result_value` set but only void Rets).
    RetShapeMismatch,
    /// Historical (Phase 2.0a–2.0c-A): callee used refcounted SSA
    /// types (`Str` / `Substr` / `Arr` / `Obj` / `Closure` / ...).
    /// Lifted in Phase 2.0c-B — the guard's premise (caller-side
    /// rc_inc/rc_dec emitted around the `Call` inst that splicing
    /// would unbalance) was verified false against the actual SSA;
    /// see the rationale comment at the former guard site in
    /// `inline_multi_block_leaf`. Kept as unreachable surface
    /// (mirrors `AllocaInBody`).
    RefcountedTypeNotSupported,
    /// Callee body contains an `Alloca` or `AllocaBytes` instruction.
    /// These materialise stack slots in the callee's frame; cloning
    /// them into the caller's body interacts with codegen's frame-
    /// builder in ways the Phase 2.0a splice does not yet model
    /// (slot lifetime, alignment, drop-on-frame-exit). Reject for
    /// safety until codegen-level frame-merge analysis lands.
    AllocaInBody,
}

pub fn inline_multi_block_leaf(
    caller: &mut Function,
    caller_blk_idx: usize,
    site_idx: usize,
    callee: &Function,
    args: &[Operand],
    result_value: Option<ValueId>,
) -> Result<(), SpliceMultiError> {
    // ---- 1. Validate.
    if callee.blocks.is_empty() {
        return Err(SpliceMultiError::EmptyCallee);
    }
    if args.len() != callee.params.len() {
        return Err(SpliceMultiError::ArityMismatch);
    }
    if caller_blk_idx >= caller.blocks.len() {
        return Err(SpliceMultiError::BlockIdxOob);
    }
    if site_idx >= caller.blocks[caller_blk_idx].insts.len() {
        return Err(SpliceMultiError::SiteIdxOob);
    }
    if !matches!(
        caller.blocks[caller_blk_idx].insts[site_idx].kind,
        InstKind::Call(_, _)
    ) {
        return Err(SpliceMultiError::NotCallSite);
    }
    let mut value_ret_count = 0u32;
    for blk in &callee.blocks {
        for inst in &blk.insts {
            if matches!(
                inst.kind,
                InstKind::Call(_, _) | InstKind::CallIndirect(_, _, _)
            ) {
                return Err(SpliceMultiError::NotLeaf);
            }
            // Phase 2.0c-A2: AllocaInBody guard lifted. The Phase
            // 2.0c-A1 (96a1a04) splice now inserts callee blocks in
            // control-flow order via `Vec::splice`, so cloned Allocas
            // in inlined callees go through the same per-function
            // alloca scan as any user-written stack slot. The
            // `AllocaInBody` enum variant is kept as unreachable
            // surface (future frame-size budget guard).
        }
        match &blk.term {
            Terminator::Br(_) | Terminator::CondBr { .. } => {}
            Terminator::Ret(opt) => {
                if opt.is_some() {
                    value_ret_count += 1;
                }
            }
            Terminator::Unreachable => {
                return Err(SpliceMultiError::UnsupportedTerminator);
            }
        }
    }
    if value_ret_count > 1 {
        return Err(SpliceMultiError::MultipleRetWithValue);
    }
    if (value_ret_count == 1) != result_value.is_some() {
        return Err(SpliceMultiError::RetShapeMismatch);
    }
    // Phase 2.0c-B: RefcountedTypeNotSupported guard lifted. The
    // guard's premise ("ssa_lower emits rc_inc/rc_dec around the
    // original call site; splicing unbalances them") was verified
    // FALSE against the actual SSA: ssa_lower emits NO per-call
    // rc traffic on the caller side. The ownership convention is
    // borrow-param / owned-ret — args pass at +0 (callee never
    // drops params; LocalInfo.borrowed), the returned value carries
    // +1 (fresh allocation, or a retain at the return boundary for
    // borrowed bindings since the retain-at-return fix). Every rc
    // operation lives INSIDE the callee body, so the block splice
    // transplants them verbatim and stays semantics-preserving.
    // The SIGSEGVs that motivated the guard during the first Phase
    // 2.0a ship were root-caused later to two since-fixed inliner
    // bugs: blocks appended out of control-flow order (96a1a04,
    // reversed live intervals → regalloc clobber) and the broken
    // `BlockId.0 == position` invariant (dbcc923, elaborate/domtree
    // indexing visited wrong blocks). The enum variant is kept as
    // unreachable surface (mirrors `AllocaInBody`).

    // ---- 2. Allocate BlockIds: one per callee block + one
    // continuation. Caller's existing BlockIds are untouched so any
    // internal caller CFG edges stay valid.
    let base = caller.blocks.len() as u32;
    let mut block_map: HashMap<BlockId, BlockId> = HashMap::new();
    for (i, blk) in callee.blocks.iter().enumerate() {
        block_map.insert(blk.id, BlockId(base + i as u32));
    }
    let continuation_block_id = BlockId(base + callee.blocks.len() as u32);
    let callee_entry_remapped = block_map[&callee.blocks[0].id];

    // ---- 3. Operand mapping: params → args, non-param values → fresh.
    let mut value_map: HashMap<ValueId, Operand> = HashMap::new();
    for (i, param_id) in callee.params.iter().enumerate() {
        value_map.insert(*param_id, args[i].clone());
    }
    let param_set: std::collections::HashSet<u32> = callee.params.iter().map(|v| v.0).collect();
    for cv in 0..callee.values.len() as u32 {
        if param_set.contains(&cv) {
            continue;
        }
        let mut info = callee.values[cv as usize].clone();
        info.name = None;
        caller.values.push(info);
        let fresh = ValueId((caller.values.len() - 1) as u32);
        value_map.insert(ValueId(cv), Operand::Value(fresh));
    }

    // ---- 4. Split caller block. Post-call insts + original
    // terminator move to the continuation block; the caller block
    // jumps to the callee entry.
    let caller_block = &mut caller.blocks[caller_blk_idx];
    let post_insts: Vec<Inst> = caller_block.insts.split_off(site_idx + 1);
    let _call_inst = caller_block.insts.pop(); // drop the original call
    let original_term = std::mem::replace(
        &mut caller_block.term,
        Terminator::Br(callee_entry_remapped),
    );

    // ---- 5. Clone each callee block + the continuation into a
    // staging Vec. We INSERT them immediately after `caller_blk_idx`
    // (not push to the end of `caller.blocks`) so the new blocks'
    // physical position matches control-flow order.
    //
    // Why this matters — codegen liveness invariant:
    // `liveness::compute_intervals` walks `func.blocks` in physical
    // order and numbers every inst with a monotonically increasing
    // `idx`. A value's interval `[start, end]` is derived from these
    // indices and `Sweep` uses `Interval::overlaps` (which assumes
    // `start <= end`) to decide whether two values may share a
    // register.
    //
    // If the cloned callee blocks + continuation are pushed to the
    // end of `caller.blocks`, but the caller's original successor
    // blocks (e.g. `bb1` holding refcount-dec for an object whose
    // pointer is defined in the continuation block) stay at their
    // pre-splice early physical positions, the value defined in the
    // continuation block ends up with `start > end` (interval
    // reversed). `overlaps` then misjudges its lifetime against any
    // value defined in `bb1` and the linear-scan allocator hands
    // both the same register — clobbering the object pointer with the
    // decremented refcount and SIGSEGV'ing on the next `str rc,
    // [obj_ptr]` (observed on parser-optional-param-implicit-null-001
    // when Phase 2.0b lifted `AllocaInBody` and the inliner started
    // firing on multi-block callees like `function f(x?: number)`).
    //
    // Inserting in-place keeps the relationship "def block precedes
    // use block in physical order" intact, matching LLVM
    // `Transforms/Utils/InlineFunction.cpp` and Go `cmd/compile/
    // internal/inline/inl.go` which both clone into the caller right
    // at the call site rather than at the function's end.
    let mut new_blocks: Vec<Block> = Vec::with_capacity(callee.blocks.len() + 1);
    for blk in &callee.blocks {
        let new_id = block_map[&blk.id];
        let mut new_insts: Vec<Inst> = Vec::with_capacity(blk.insts.len() + 1);
        for inst in &blk.insts {
            let new_kind = rewrite_inst_kind(&inst.kind, &value_map);
            let new_result = inst.result.map(|r| match value_map.get(&r) {
                Some(Operand::Value(v)) => *v,
                _ => unreachable!(
                    "non-param callee Inst result must map to a fresh caller ValueId; \
                     value_map invariant broken"
                ),
            });
            new_insts.push(Inst {
                result: new_result,
                kind: new_kind,
                origin: inst.origin,
            });
        }
        let new_term = match &blk.term {
            Terminator::Br(target) => Terminator::Br(block_map[target]),
            Terminator::CondBr {
                cond,
                then_blk,
                else_blk,
            } => Terminator::CondBr {
                cond: rewrite_operand(cond, &value_map),
                then_blk: block_map[then_blk],
                else_blk: block_map[else_blk],
            },
            Terminator::Ret(opt) => {
                if let (Some(ret_op), Some(r)) = (opt.as_ref(), result_value) {
                    new_insts.push(Inst {
                        result: Some(r),
                        kind: InstKind::Identity(rewrite_operand(ret_op, &value_map)),
                        origin: new_insts.last().and_then(|i| i.origin),
                    });
                }
                Terminator::Br(continuation_block_id)
            }
            Terminator::Unreachable => Terminator::Unreachable,
        };
        new_blocks.push(Block {
            id: new_id,
            insts: new_insts,
            term: new_term,
        });
    }

    // ---- 6. Continuation block holding post-call insts + the
    // caller's original terminator. Appended to the staging Vec so it
    // sits between the callee blocks and the caller's original
    // successor blocks in physical order.
    new_blocks.push(Block {
        id: continuation_block_id,
        insts: post_insts,
        term: original_term,
    });

    let insert_at = caller_blk_idx + 1;
    caller.blocks.splice(insert_at..insert_at, new_blocks);

    // ---- 7. Renumber every block's `BlockId.0` so it equals its
    // physical position in `caller.blocks` (== Vec index), then
    // rewrite every terminator's BlockId references through the
    // resulting remap. The rest of the egraph/codegen pipeline
    // (`elaborate.rs`, `dominator.rs`, `egraph.rs::available_block`,
    // `optimize.rs`) indexes `func.blocks` directly by `BlockId.0
    // as usize` — `bid.0 as usize` and `func.blocks[bid.0 as usize]`
    // are scattered across every downstream pass. The inliner
    // therefore MUST re-establish the `BlockId.0 == position`
    // invariant after splicing; otherwise the dominator tree visits
    // wrong blocks and the elaborator skips Identity nodes that
    // codegen liveness then rejects with `unreachable!()`.
    let n = caller.blocks.len();
    let mut block_remap: HashMap<BlockId, BlockId> = HashMap::with_capacity(n);
    for (pos, blk) in caller.blocks.iter().enumerate() {
        block_remap.insert(blk.id, BlockId(pos as u32));
    }
    for blk in caller.blocks.iter_mut() {
        blk.id = block_remap[&blk.id];
        blk.term = match std::mem::replace(&mut blk.term, Terminator::Unreachable) {
            Terminator::Br(target) => Terminator::Br(block_remap[&target]),
            Terminator::CondBr {
                cond,
                then_blk,
                else_blk,
            } => Terminator::CondBr {
                cond,
                then_blk: block_remap[&then_blk],
                else_blk: block_remap[&else_blk],
            },
            Terminator::Ret(opt) => Terminator::Ret(opt),
            Terminator::Unreachable => Terminator::Unreachable,
        };
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{
        Block, BlockId, FuncId, Function, IPred, Inst, InstKind, Operand, Terminator, Type,
        ValueId, ValueInfo,
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

    fn blk(id: u32, insts: Vec<Inst>, term: Terminator) -> Block {
        Block {
            id: BlockId(id),
            insts,
            term,
        }
    }

    fn val(ty: Type, name: &str) -> ValueInfo {
        ValueInfo {
            ty,
            name: Some(name.into()),
        }
    }

    /// Build a two-block void callee: entry → exit, exit returns void.
    fn two_block_void() -> Function {
        Function {
            name: "two_block".into(),
            params: vec![],
            ret: Type::Void,
            blocks: vec![
                blk(0, vec![], Terminator::Br(BlockId(1))),
                blk(1, vec![], Terminator::Ret(None)),
            ],
            values: vec![],
            current_origin: None,
        }
    }

    /// 3-block callee returning `a + b` via a trivial straight-line
    /// CFG: entry → body → exit. Multi-block but single Ret(Some) —
    /// fits Phase 2.0a narrow scope. (A real min(a,b) with two
    /// value-returning Rets is Phase 2.0b territory and exercises the
    /// `rejects_multiple_value_bearing_rets` path below.)
    fn straight_line_3block_add() -> Function {
        Function {
            name: "add3".into(),
            params: vec![ValueId(0), ValueId(1)],
            ret: Type::I64,
            blocks: vec![
                blk(0, vec![], Terminator::Br(BlockId(1))),
                blk(
                    1,
                    vec![val_inst(
                        ValueId(2),
                        InstKind::BinOp(
                            torajs_core::ssa::BinOp::Add,
                            Operand::Value(ValueId(0)),
                            Operand::Value(ValueId(1)),
                        ),
                    )],
                    Terminator::Br(BlockId(2)),
                ),
                blk(2, vec![], Terminator::Ret(Some(Operand::Value(ValueId(2))))),
            ],
            values: vec![
                val(Type::I64, "a"),
                val(Type::I64, "b"),
                val(Type::I64, "sum"),
            ],
            current_origin: None,
        }
    }

    /// Two-value-Ret min-callee — fixture for the
    /// `rejects_multiple_value_bearing_rets` test (Phase 2.0a out of
    /// scope; Phase 2.0b lifts via Φ-node insertion).
    fn min_callee_via_branches() -> Function {
        Function {
            name: "min".into(),
            params: vec![ValueId(0), ValueId(1)],
            ret: Type::I64,
            blocks: vec![
                blk(
                    0,
                    vec![val_inst(
                        ValueId(2),
                        InstKind::ICmp(
                            IPred::Slt,
                            Operand::Value(ValueId(0)),
                            Operand::Value(ValueId(1)),
                        ),
                    )],
                    Terminator::CondBr {
                        cond: Operand::Value(ValueId(2)),
                        then_blk: BlockId(1),
                        else_blk: BlockId(2),
                    },
                ),
                blk(1, vec![], Terminator::Ret(Some(Operand::Value(ValueId(0))))),
                blk(2, vec![], Terminator::Ret(Some(Operand::Value(ValueId(1))))),
            ],
            values: vec![
                val(Type::I64, "a"),
                val(Type::I64, "b"),
                val(Type::Bool, "lt"),
            ],
            current_origin: None,
        }
    }

    fn caller_with_call(call: Inst, ret_ty: Type, term: Terminator) -> Function {
        Function {
            name: "main".into(),
            params: vec![],
            ret: ret_ty.clone(),
            blocks: vec![Block {
                id: BlockId(0),
                insts: vec![call],
                term,
            }],
            values: if matches!(ret_ty, Type::I64) {
                vec![ValueInfo {
                    ty: Type::I64,
                    name: Some("r".into()),
                }]
            } else {
                vec![]
            },
            current_origin: None,
        }
    }

    #[test]
    fn multi_block_void_callee_splices_in() {
        let callee = two_block_void();
        let mut caller = caller_with_call(
            void_inst(InstKind::Call(FuncId(1), vec![])),
            Type::Void,
            Terminator::Ret(None),
        );
        let res = inline_multi_block_leaf(&mut caller, 0, 0, &callee, &[], None);
        assert!(res.is_ok(), "void multi-block splice should succeed");
        // Caller: orig block (modified: no insts, Br(entry)) +
        // 2 callee blocks (entry Br(exit), exit Br(continuation)) +
        // 1 continuation (Ret(None)). Total 4 blocks.
        assert_eq!(caller.blocks.len(), 4);
        // orig caller block's terminator must now be Br to the entry
        // of the cloned callee.
        match &caller.blocks[0].term {
            Terminator::Br(_) => {}
            other => panic!("caller block 0 must branch into callee entry: {:?}", other),
        }
        // last block is the continuation, holds the original Ret(None).
        match &caller.blocks[3].term {
            Terminator::Ret(None) => {}
            other => panic!("continuation must hold original Ret: {:?}", other),
        }
    }

    #[test]
    fn multi_block_value_returning_straight_line_callee_splices_in() {
        let callee = straight_line_3block_add();
        let mut caller = caller_with_call(
            val_inst(
                ValueId(0),
                InstKind::Call(FuncId(1), vec![Operand::ConstI64(7), Operand::ConstI64(3)]),
            ),
            Type::I64,
            Terminator::Ret(Some(Operand::Value(ValueId(0)))),
        );
        let res = inline_multi_block_leaf(
            &mut caller,
            0,
            0,
            &callee,
            &[Operand::ConstI64(7), Operand::ConstI64(3)],
            Some(ValueId(0)),
        );
        assert!(res.is_ok(), "single-value-Ret multi-block splice ok");
        // Expect blocks: original (Br to entry) + 3 callee + 1 continuation = 5.
        assert_eq!(caller.blocks.len(), 5);
        // Caller block 0 inst count: was 1 (the call), is now 0.
        assert_eq!(caller.blocks[0].insts.len(), 0);
        // Last block holds the original caller terminator.
        match &caller.blocks[4].term {
            Terminator::Ret(Some(Operand::Value(v))) if *v == ValueId(0) => {}
            other => panic!("continuation must hold original Ret(Some(0)): {:?}", other),
        }
        // The third cloned callee block (BlockId(2)) was the Ret block;
        // after splice it must Br to continuation with an Identity
        // binding the caller's result_value to the rewritten sum.
        let exit_clone = &caller.blocks[3];
        assert!(
            matches!(exit_clone.term, Terminator::Br(_)),
            "Ret-block clone must Br to continuation"
        );
        assert!(
            exit_clone
                .insts
                .iter()
                .any(|i| matches!(i.kind, InstKind::Identity(_)) && i.result == Some(ValueId(0))),
            "Ret-block clone must bind result via Identity",
        );
    }

    #[test]
    fn rejects_nested_call_in_callee() {
        let mut callee = two_block_void();
        callee.blocks[0]
            .insts
            .push(void_inst(InstKind::Call(FuncId(2), vec![])));
        let mut caller = caller_with_call(
            void_inst(InstKind::Call(FuncId(1), vec![])),
            Type::Void,
            Terminator::Ret(None),
        );
        assert_eq!(
            inline_multi_block_leaf(&mut caller, 0, 0, &callee, &[], None),
            Err(SpliceMultiError::NotLeaf)
        );
    }

    #[test]
    fn rejects_unreachable_terminator() {
        let mut callee = two_block_void();
        callee.blocks[1].term = Terminator::Unreachable;
        let mut caller = caller_with_call(
            void_inst(InstKind::Call(FuncId(1), vec![])),
            Type::Void,
            Terminator::Ret(None),
        );
        assert_eq!(
            inline_multi_block_leaf(&mut caller, 0, 0, &callee, &[], None),
            Err(SpliceMultiError::UnsupportedTerminator)
        );
    }

    #[test]
    fn rejects_multiple_value_bearing_rets() {
        // Same as min_callee_via_branches but both Rets bear values:
        // already the case there. Add an extra ret-with-value block to
        // make it >1.
        let mut callee = min_callee_via_branches();
        // min_callee_via_branches already has 2 Ret(Some) — should be rejected.
        let mut caller = caller_with_call(
            val_inst(
                ValueId(0),
                InstKind::Call(FuncId(1), vec![Operand::ConstI64(1), Operand::ConstI64(2)]),
            ),
            Type::I64,
            Terminator::Ret(Some(Operand::Value(ValueId(0)))),
        );
        // Force >1 by making sure both arms remain value-bearing.
        assert!(matches!(callee.blocks[1].term, Terminator::Ret(Some(_))));
        assert!(matches!(callee.blocks[2].term, Terminator::Ret(Some(_))));
        assert_eq!(
            inline_multi_block_leaf(
                &mut caller,
                0,
                0,
                &callee,
                &[Operand::ConstI64(1), Operand::ConstI64(2)],
                Some(ValueId(0)),
            ),
            Err(SpliceMultiError::MultipleRetWithValue)
        );
        // (Eliminate the unused `mut` warning by mutating callee once
        // — purely cosmetic to keep clippy happy when MultipleRet path
        // taken before any mutation occurs.)
        let _ = &mut callee;
    }

    #[test]
    fn rejects_ret_shape_mismatch() {
        // Value-bearing single Ret callee but caller passes
        // result_value = None.
        let callee = Function {
            name: "ret_val".into(),
            params: vec![],
            ret: Type::I64,
            blocks: vec![Block {
                id: BlockId(0),
                insts: vec![],
                term: Terminator::Ret(Some(Operand::ConstI64(42))),
            }],
            values: vec![],
            current_origin: None,
        };
        let mut caller = caller_with_call(
            void_inst(InstKind::Call(FuncId(1), vec![])),
            Type::Void,
            Terminator::Ret(None),
        );
        assert_eq!(
            inline_multi_block_leaf(&mut caller, 0, 0, &callee, &[], None),
            Err(SpliceMultiError::RetShapeMismatch)
        );
    }

    #[test]
    fn rejects_arity_mismatch() {
        let callee = min_callee_via_branches();
        let mut caller = caller_with_call(
            val_inst(
                ValueId(0),
                InstKind::Call(FuncId(1), vec![Operand::ConstI64(1)]),
            ),
            Type::I64,
            Terminator::Ret(Some(Operand::Value(ValueId(0)))),
        );
        assert_eq!(
            inline_multi_block_leaf(
                &mut caller,
                0,
                0,
                &callee,
                &[Operand::ConstI64(1)],
                Some(ValueId(0)),
            ),
            Err(SpliceMultiError::ArityMismatch)
        );
    }

    /// Physical-block-order invariant — the regression test for the
    /// SIGSEGV observed on parser-optional-param-implicit-null-001
    /// during Phase 2.0b ship: a caller with multiple blocks
    /// (modelling main bb0 holding the call site followed by bb1 /
    /// bb2 holding refcount-dec cleanup that references a value
    /// defined inside the post-call portion of bb0) must end up with
    /// the cloned callee blocks + continuation block inserted
    /// IMMEDIATELY AFTER the caller block containing the call site,
    /// NOT appended to the end of `caller.blocks`. Appending breaks
    /// `liveness::compute_intervals` because the post-call continuation
    /// would land at a later physical index than the caller's original
    /// successor blocks, producing reversed intervals (start > end)
    /// that `Interval::overlaps` misjudges. The fix splices the new
    /// blocks at `caller_blk_idx + 1`.
    #[test]
    fn cloned_blocks_insert_after_caller_block_not_at_end() {
        // Caller shape: bb0 holds the call site and an unconditional
        // br to bb1; bb1 is a no-op block returning. The original
        // bb1 must remain AFTER all cloned callee blocks and the
        // continuation in the post-splice physical order.
        let callee = straight_line_3block_add();
        let mut caller = Function {
            name: "main_with_cleanup".into(),
            params: vec![],
            ret: Type::I64,
            blocks: vec![
                Block {
                    id: BlockId(0),
                    insts: vec![val_inst(
                        ValueId(0),
                        InstKind::Call(FuncId(1), vec![Operand::ConstI64(7), Operand::ConstI64(3)]),
                    )],
                    term: Terminator::Br(BlockId(1)),
                },
                Block {
                    id: BlockId(1),
                    insts: vec![],
                    term: Terminator::Ret(Some(Operand::Value(ValueId(0)))),
                },
            ],
            values: vec![ValueInfo {
                ty: Type::I64,
                name: Some("r".into()),
            }],
            current_origin: None,
        };

        let res = inline_multi_block_leaf(
            &mut caller,
            0,
            0,
            &callee,
            &[Operand::ConstI64(7), Operand::ConstI64(3)],
            Some(ValueId(0)),
        );
        assert!(res.is_ok(), "multi-bb caller splice must succeed");

        // Layout after fix: bb0 (modified) at [0]; 3 cloned callee
        // blocks at [1..4]; continuation at [4]; original bb1
        // (cleanup) at [5]. Total = 6 blocks. Post-splice renumber
        // assigns `BlockId.0 == position` to every block so the
        // downstream egraph/codegen passes that index `func.blocks`
        // by `BlockId.0` keep working.
        assert_eq!(caller.blocks.len(), 6, "expected 6 blocks post-splice");

        // Every block's BlockId equals its physical position.
        for (pos, b) in caller.blocks.iter().enumerate() {
            assert_eq!(
                b.id,
                BlockId(pos as u32),
                "BlockId.0 == position invariant must hold at pos {pos}",
            );
        }

        // First block is still the modified original bb0 — has Br
        // terminator pointing at the (remapped) callee entry (= pos 1).
        assert!(matches!(&caller.blocks[0].term, Terminator::Br(BlockId(1))));

        // The LAST block must hold the cleanup Ret (the original
        // caller bb1's terminator). This is the regression
        // assertion: pre-fix, bb1 stayed at idx 1 and the
        // continuation got pushed to the end, reversing live
        // intervals.
        match &caller.blocks[5].term {
            Terminator::Ret(Some(Operand::Value(v))) if *v == ValueId(0) => {}
            other => panic!("expected cleanup Ret at last position, got {:?}", other),
        }

        // The continuation (block[4]) holds the caller bb0's original
        // terminator (Br to the original bb1, now at pos 5).
        assert!(matches!(&caller.blocks[4].term, Terminator::Br(BlockId(5))));
    }

    /// Phase 2.0c-B — refcounted multi-block callees splice. A
    /// Str-typed two-block passthrough was rejected with
    /// `RefcountedTypeNotSupported` before the guard lift.
    #[test]
    fn refcounted_multi_block_callee_splices_after_guard_lift() {
        let callee = Function {
            name: "id_str2".into(),
            params: vec![ValueId(0)],
            ret: Type::Str,
            blocks: vec![
                blk(0, vec![], Terminator::Br(BlockId(1))),
                blk(1, vec![], Terminator::Ret(Some(Operand::Value(ValueId(0))))),
            ],
            values: vec![val(Type::Str, "s")],
            current_origin: None,
        };
        let mut caller = Function {
            name: "main".into(),
            params: vec![ValueId(0)],
            ret: Type::Str,
            blocks: vec![blk(
                0,
                vec![val_inst(
                    ValueId(1),
                    InstKind::Call(FuncId(1), vec![Operand::Value(ValueId(0))]),
                )],
                Terminator::Ret(Some(Operand::Value(ValueId(1)))),
            )],
            values: vec![val(Type::Str, "arg"), val(Type::Str, "r")],
            current_origin: None,
        };
        let res = inline_multi_block_leaf(
            &mut caller,
            0,
            0,
            &callee,
            &[Operand::Value(ValueId(0))],
            Some(ValueId(1)),
        );
        assert!(res.is_ok(), "refcounted callee must splice: {:?}", res);
        // caller bb0 + 2 callee blocks + continuation = 4 blocks,
        // BlockId.0 == position invariant restored by the renumber.
        assert_eq!(caller.blocks.len(), 4);
        for (pos, b) in caller.blocks.iter().enumerate() {
            assert_eq!(b.id, BlockId(pos as u32));
        }
        assert!(
            caller
                .blocks
                .iter()
                .flat_map(|b| b.insts.iter())
                .any(|i| matches!(i.kind, InstKind::Identity(_)) && i.result == Some(ValueId(1))),
            "ret value must bind through an Identity"
        );
    }
}
