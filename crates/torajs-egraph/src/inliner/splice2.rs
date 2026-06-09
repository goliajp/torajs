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
    /// Callee uses refcounted SSA types in its signature (params or
    /// return type) or any of its non-parameter values.
    ///
    /// torajs's refcounted types (`Str` / `Substr` / `Arr` / `Obj` /
    /// `Closure` / `Promise` / `Map` / `Set` / etc.) participate in
    /// caller-side `__torajs_rc_inc` / `__torajs_rc_dec` calling-
    /// convention that ssa_lower emits around the original `Call`
    /// instruction. Splicing the callee body into the caller without
    /// rebalancing those calls causes double-drops or use-after-free
    /// at runtime (observed as SIGSEGV / SIGBUS on the
    /// parser-optional-param / restparam / spread conformance
    /// fixtures during the first Phase 2.0a ship). Until the splice
    /// becomes RC-aware (Phase 2.0c+), this guard restricts inlining
    /// to value-shaped callees (number / bool / void).
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
            if matches!(inst.kind, InstKind::Alloca(_) | InstKind::AllocaBytes(_)) {
                return Err(SpliceMultiError::AllocaInBody);
            }
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
    // ---- 1b. Refcounted-type guard. ssa_lower emits rc_inc/rc_dec
    // calls around the original call site for refcounted types
    // (Str / Arr / Obj / Closure / Promise / Map / Set / ...);
    // splicing without rebalancing those calls causes double-drops
    // or use-after-free. Until the splice becomes RC-aware, restrict
    // inlining to value-shaped callees.
    if callee.ret.is_refcounted() {
        return Err(SpliceMultiError::RefcountedTypeNotSupported);
    }
    for param_id in &callee.params {
        let ty = &callee.values[param_id.0 as usize].ty;
        if ty.is_refcounted() {
            return Err(SpliceMultiError::RefcountedTypeNotSupported);
        }
    }
    for (i, vi) in callee.values.iter().enumerate() {
        if callee.params.iter().any(|p| p.0 as usize == i) {
            continue;
        }
        if vi.ty.is_refcounted() {
            return Err(SpliceMultiError::RefcountedTypeNotSupported);
        }
    }

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

    // ---- 5. Clone each callee block into caller.blocks.
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
        caller.blocks.push(Block {
            id: new_id,
            insts: new_insts,
            term: new_term,
        });
    }

    // ---- 6. Push continuation block holding post-call insts + the
    // caller's original terminator.
    caller.blocks.push(Block {
        id: continuation_block_id,
        insts: post_insts,
        term: original_term,
    });

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

    /// Build a two-block void callee: entry → exit, exit returns void.
    fn two_block_void() -> Function {
        Function {
            name: "two_block".into(),
            params: vec![],
            ret: Type::Void,
            blocks: vec![
                Block {
                    id: BlockId(0),
                    insts: vec![],
                    term: Terminator::Br(BlockId(1)),
                },
                Block {
                    id: BlockId(1),
                    insts: vec![],
                    term: Terminator::Ret(None),
                },
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
                Block {
                    id: BlockId(0),
                    insts: vec![],
                    term: Terminator::Br(BlockId(1)),
                },
                Block {
                    id: BlockId(1),
                    insts: vec![val_inst(
                        ValueId(2),
                        InstKind::BinOp(
                            torajs_core::ssa::BinOp::Add,
                            Operand::Value(ValueId(0)),
                            Operand::Value(ValueId(1)),
                        ),
                    )],
                    term: Terminator::Br(BlockId(2)),
                },
                Block {
                    id: BlockId(2),
                    insts: vec![],
                    term: Terminator::Ret(Some(Operand::Value(ValueId(2)))),
                },
            ],
            values: vec![
                ValueInfo {
                    ty: Type::I64,
                    name: Some("a".into()),
                },
                ValueInfo {
                    ty: Type::I64,
                    name: Some("b".into()),
                },
                ValueInfo {
                    ty: Type::I64,
                    name: Some("sum".into()),
                },
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
                Block {
                    id: BlockId(0),
                    insts: vec![val_inst(
                        ValueId(2),
                        InstKind::ICmp(
                            IPred::Slt,
                            Operand::Value(ValueId(0)),
                            Operand::Value(ValueId(1)),
                        ),
                    )],
                    term: Terminator::CondBr {
                        cond: Operand::Value(ValueId(2)),
                        then_blk: BlockId(1),
                        else_blk: BlockId(2),
                    },
                },
                Block {
                    id: BlockId(1),
                    insts: vec![],
                    term: Terminator::Ret(Some(Operand::Value(ValueId(0)))),
                },
                Block {
                    id: BlockId(2),
                    insts: vec![],
                    term: Terminator::Ret(Some(Operand::Value(ValueId(1)))),
                },
            ],
            values: vec![
                ValueInfo {
                    ty: Type::I64,
                    name: Some("a".into()),
                },
                ValueInfo {
                    ty: Type::I64,
                    name: Some("b".into()),
                },
                ValueInfo {
                    ty: Type::Bool,
                    name: Some("lt".into()),
                },
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
}
