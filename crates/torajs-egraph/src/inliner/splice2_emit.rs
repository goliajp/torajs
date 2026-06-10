//! splice2 emit half — the mutation steps of `inline_multi_block_leaf`
//! after validation passes: BlockId allocation, operand mapping,
//! caller-block split, multi-ret join slot, callee block cloning,
//! continuation assembly, and the `BlockId.0 == position` renumber.
//! Split out of `splice2.rs` to keep both files inside the project
//! file/function size hard limits; see `splice2.rs` for the
//! validation envelope and the error surface.

use super::rewrite::{rewrite_inst_kind, rewrite_operand};
use std::collections::HashMap;
use torajs_core::ssa::{
    Block, BlockId, Function, Inst, InstKind, Operand, Terminator, Type, ValueId, ValueInfo,
};

/// Perform the splice proper. Pre-validated by
/// `splice2::inline_multi_block_leaf` — `value_ret_count` is the
/// number of value-bearing `Ret` terminators in `callee` (≥ 2 routes
/// the result through a stack-slot join, 1 uses the zero-cost
/// Identity binding).
#[allow(clippy::too_many_arguments)]
pub(super) fn splice_body(
    caller: &mut Function,
    caller_blk_idx: usize,
    site_idx: usize,
    callee: &Function,
    args: &[Operand],
    result_value: Option<ValueId>,
    value_ret_count: u32,
) {
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

    // ---- 4b. Multi-ret join slot. With ≥ 2 value-bearing rets and no
    // phi nodes in the SSA, the returned value joins through a stack
    // slot: alloca'd at the end of the (split) caller block, stored by
    // every cloned `Ret(Some(_))`, loaded once at the continuation
    // head. codegen's per-function alloca scan assigns it a frame slot
    // like any user-written local, so a call site inside a loop reuses
    // the same slot every iteration.
    let ret_slot: Option<ValueId> = if value_ret_count > 1 {
        caller.values.push(ValueInfo {
            ty: Type::Ptr,
            name: None,
        });
        let slot = ValueId((caller.values.len() - 1) as u32);
        caller.blocks[caller_blk_idx].insts.push(Inst {
            result: Some(slot),
            kind: InstKind::Alloca(callee.ret.clone()),
            origin: None,
        });
        Some(slot)
    } else {
        None
    };

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
                    match ret_slot {
                        // multi-ret: store into the join slot; the
                        // continuation head loads it into `r`
                        Some(slot) => new_insts.push(Inst {
                            result: None,
                            kind: InstKind::Store(
                                rewrite_operand(ret_op, &value_map),
                                Operand::Value(slot),
                                0,
                            ),
                            origin: new_insts.last().and_then(|i| i.origin),
                        }),
                        // single ret: zero-cost Identity binding
                        None => new_insts.push(Inst {
                            result: Some(r),
                            kind: InstKind::Identity(rewrite_operand(ret_op, &value_map)),
                            origin: new_insts.last().and_then(|i| i.origin),
                        }),
                    }
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
    // successor blocks in physical order. In the multi-ret case the
    // continuation head loads the join slot into the call-result
    // ValueId before any post-call inst reads it.
    let mut cont_insts = post_insts;
    if let (Some(slot), Some(r)) = (ret_slot, result_value) {
        cont_insts.insert(
            0,
            Inst {
                result: Some(r),
                kind: InstKind::Load(callee.ret.clone(), Operand::Value(slot), 0),
                origin: None,
            },
        );
    }
    new_blocks.push(Block {
        id: continuation_block_id,
        insts: cont_insts,
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
}

#[cfg(test)]
pub(crate) mod fixtures {
    //! Shared test fixtures for the splice2 validation tests
    //! (`splice2.rs`) and the emit tests below.
    use torajs_core::ssa::{
        Block, BlockId, Function, IPred, Inst, InstKind, Operand, Terminator, Type, ValueId,
        ValueInfo,
    };

    pub(crate) fn val_inst(result: ValueId, kind: InstKind) -> Inst {
        Inst {
            result: Some(result),
            kind,
            origin: None,
        }
    }

    pub(crate) fn void_inst(kind: InstKind) -> Inst {
        Inst {
            result: None,
            kind,
            origin: None,
        }
    }

    pub(crate) fn blk(id: u32, insts: Vec<Inst>, term: Terminator) -> Block {
        Block {
            id: BlockId(id),
            insts,
            term,
        }
    }

    pub(crate) fn val(ty: Type, name: &str) -> ValueInfo {
        ValueInfo {
            ty,
            name: Some(name.into()),
        }
    }

    /// Build a two-block void callee: entry → exit, exit returns void.
    pub(crate) fn two_block_void() -> Function {
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
    pub(crate) fn straight_line_3block_add() -> Function {
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
    pub(crate) fn min_callee_via_branches() -> Function {
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

    pub(crate) fn caller_with_call(call: Inst, ret_ty: Type, term: Terminator) -> Function {
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
}

#[cfg(test)]
mod tests {
    use super::super::splice2::inline_multi_block_leaf;
    use super::fixtures::*;
    use torajs_core::ssa::{
        Block, BlockId, FuncId, Function, Inst, InstKind, Operand, Terminator, Type, ValueId,
        ValueInfo,
    };

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
    fn nested_call_in_callee_clones_through() {
        // Phase 2.1: a non-leaf multi-block callee inlines fine — the
        // nested call clones verbatim into the spliced blocks.
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
            Ok(())
        );
        let nested: usize = caller
            .blocks
            .iter()
            .flat_map(|b| &b.insts)
            .filter(|i| matches!(i.kind, InstKind::Call(FuncId(2), _)))
            .count();
        assert_eq!(nested, 1, "nested call must survive the splice");
    }

    #[test]
    fn multi_value_rets_join_through_stack_slot() {
        // Phase 2.1: min(a,b) with two value-bearing Rets inlines via
        // the stack-slot join — alloca at the split caller block tail,
        // a Store per cloned Ret arm, one Load at the continuation
        // head binding the call-result ValueId.
        let callee = min_callee_via_branches();
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
            Ok(())
        );
        // split caller block ends with the join-slot alloca
        let slot = match caller.blocks[0].insts.last() {
            Some(Inst {
                result: Some(slot),
                kind: InstKind::Alloca(Type::I64),
                ..
            }) => *slot,
            other => panic!("expected join-slot alloca, got {other:?}"),
        };
        // each cloned value-Ret arm stores into the slot then branches
        let stores: usize = caller
            .blocks
            .iter()
            .flat_map(|b| &b.insts)
            .filter(|i| matches!(&i.kind, InstKind::Store(_, Operand::Value(p), 0) if *p == slot))
            .count();
        assert_eq!(stores, 2, "one store per value-bearing ret arm");
        // continuation head loads the slot into the call-result vid
        let loads: Vec<&Inst> = caller
            .blocks
            .iter()
            .flat_map(|b| &b.insts)
            .filter(|i| matches!(&i.kind, InstKind::Load(_, Operand::Value(p), 0) if *p == slot))
            .collect();
        assert_eq!(loads.len(), 1);
        assert_eq!(loads[0].result, Some(ValueId(0)));
        // and no call inst survives anywhere
        assert!(
            !caller
                .blocks
                .iter()
                .flat_map(|b| &b.insts)
                .any(|i| matches!(i.kind, InstKind::Call(..)))
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
