//! Phase 2.0a splice2 — multi-block leaf SSA mutation.
//!
//! Extends Phase 1.0a's `inline_single_block_leaf` to multi-block
//! callees: callees may have internal `Br` / `CondBr` control flow
//! (if-else / loop bodies) and, since Phase 2.1, nested `Call` /
//! `CallIndirect` insts (cloned verbatim — general non-leaf inlining
//! plus bounded recursive inlining) as well as multiple value-bearing
//! `Ret(Some(_))` terminators (joined through a caller stack slot —
//! the no-phi analogue of LLVM's merged-return block).
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
//! Validation envelope (each enforced via `SpliceMultiError`):
//! * `callee.blocks.len() ≥ 1` (declarations are out).
//! * At most one `Ret(Some(_))` terminator (`Ret(None)` may appear
//!   multiple times; each branches to continuation without binding).
//! * Ret-shape ↔ `result_value` agree (both Some or both None).
//! * `args.len() == callee.params.len()`.
//! * Caller block + site indices in bounds + inst at site is a `Call`.
//! * Terminators restricted to `Br` / `CondBr` / `Ret`; an
//!   `Unreachable` in callee is rejected with `UnsupportedTerminator`
//!   (rare in production; Phase 2 follow-up if it surfaces).

use super::splice2_emit::splice_body;
use torajs_core::ssa::{Function, InstKind, Operand, Terminator, ValueId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpliceMultiError {
    BlockIdxOob,
    SiteIdxOob,
    NotCallSite,
    ArityMismatch,
    /// Callee declaration (no blocks).
    EmptyCallee,
    /// Historical (Phase 2.0a–2.0c): callee body contained a nested
    /// `Call` / `CallIndirect`. Lifted in Phase 2.1 — nested calls
    /// clone verbatim (operands remapped, FuncId/SigId module-level);
    /// see the rationale comment at the former guard site. Kept as
    /// unreachable surface (mirrors `AllocaInBody`).
    NotLeaf,
    /// Historical (Phase 2.0a–2.0c): callee had more than one
    /// `Ret(Some(_))` value-bearing terminator. Lifted in Phase 2.1 —
    /// multiple value rets join through a caller stack slot (store at
    /// each ret, load at the continuation head; the no-phi analogue
    /// of LLVM's merged-return block). Kept as unreachable surface
    /// (mirrors `AllocaInBody`).
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
    let mut void_ret_count = 0u32;
    for blk in &callee.blocks {
        // Phase 2.1: NotLeaf guard lifted. Nested `Call` /
        // `CallIndirect` insts clone like any other instruction —
        // `rewrite_inst_kind` remaps their argument operands and the
        // callee `FuncId` / `SigId` are module-level (no remap
        // needed). The guard was Phase 2.0a scope control, not a
        // correctness constraint; lifting it enables both general
        // non-leaf inlining and GCC-style bounded recursive inlining
        // (`InlineBudget::max_recursion_depth`). Text growth stays
        // bounded because `cost_of_kind` already prices nested calls
        // (CALL_COST) and the emit pass re-checks the callee's live
        // cost against the ceiling before splicing. The enum variant
        // is kept as unreachable surface (mirrors `AllocaInBody`).
        //
        // Phase 2.0c-A2: AllocaInBody guard lifted. The Phase
        // 2.0c-A1 (96a1a04) splice now inserts callee blocks in
        // control-flow order via `Vec::splice`, so cloned Allocas
        // in inlined callees go through the same per-function
        // alloca scan as any user-written stack slot. The
        // `AllocaInBody` enum variant is kept as unreachable
        // surface (future frame-size budget guard).
        match &blk.term {
            Terminator::Br(_) | Terminator::CondBr { .. } => {}
            Terminator::Ret(opt) => {
                if opt.is_some() {
                    value_ret_count += 1;
                } else {
                    void_ret_count += 1;
                }
            }
            Terminator::Unreachable => {
                return Err(SpliceMultiError::UnsupportedTerminator);
            }
        }
    }
    // Phase 2.1: MultipleRetWithValue guard lifted. With no phi nodes
    // in the SSA (by design — torajs SSA is alloca/load/store-formed),
    // multiple value-bearing rets join through a caller stack slot:
    // each `Ret(Some(v))` becomes `Store v, slot; Br continuation` and
    // the continuation head loads the slot into `result_value`. The
    // single-ret case keeps the zero-cost Identity binding. A callee
    // mixing value rets and void rets stays rejected (shape error).
    if (value_ret_count >= 1) != result_value.is_some()
        || (value_ret_count >= 1 && void_ret_count >= 1)
    {
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

    splice_body(
        caller,
        caller_blk_idx,
        site_idx,
        callee,
        args,
        result_value,
        value_ret_count,
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inliner::splice2_emit::fixtures::*;
    use torajs_core::ssa::{Block, BlockId, FuncId, Function, Operand, Terminator, Type, ValueId};

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
    fn mixed_value_and_void_rets_rejected() {
        // a callee mixing Ret(Some) and Ret(None) arms has no coherent
        // result shape — stays rejected
        let mut callee = min_callee_via_branches();
        callee.blocks[2].term = Terminator::Ret(None);
        let mut caller = caller_with_call(
            val_inst(
                ValueId(0),
                InstKind::Call(FuncId(1), vec![Operand::ConstI64(1), Operand::ConstI64(2)]),
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
                &[Operand::ConstI64(1), Operand::ConstI64(2)],
                Some(ValueId(0)),
            ),
            Err(SpliceMultiError::RetShapeMismatch)
        );
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
