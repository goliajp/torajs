//! Phase 1.0a splice — single-block leaf SSA mutation.
//!
//! This sub-module owns the SSA-mutation half of Cluster E inlining.
//! The Phase 0 decision substrate (`InlineBudget` / `InlineDecision` /
//! `SkipReason` / `InlinerStats` / `callee_body_cost` /
//! `classify_caller_sites`) lives in the parent module
//! `crate::inliner`; this file consumes none of it and is consumed by
//! the parent's Phase 1.0b driver once it lands.
//!
//! Public surface:
//!
//! * `SpliceError` — 8 variants covering every Phase 1 narrow-scope
//!   precondition (block / site / arity / single-block / leaf / Ret
//!   terminator / Ret shape match / call inst at site).
//! * `inline_single_block_leaf(caller, blk, site, callee, args, result)`
//!   — splice routine. On success the call instruction is replaced by
//!   the rewritten callee body; on Err the caller is untouched.
//!
//! Internal helpers (`rewrite_operand`, `rewrite_inst_kind`) form an
//! *exhaustive* variant-by-variant match over `torajs-core::ssa::
//! InstKind`, so adding a new SSA opcode upstream is a compile-time
//! signal here rather than a silent miscompile.

use super::rewrite::{rewrite_inst_kind, rewrite_operand};
use std::collections::HashMap;
use torajs_core::ssa::{Function, Inst, InstKind, Operand, Terminator, ValueId};

/// Reason a `inline_single_block_leaf` call failed its Phase 1 narrow
/// preconditions. The driver (Phase 1.0b `inline_module`) is expected
/// to have pre-filtered candidates via `would_inline`; any
/// `SpliceError` returned here is therefore either a true precondition
/// violation (caller passed an inappropriate candidate) or a bug in
/// the Phase 0 classifier — both worth surfacing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpliceError {
    /// `caller_blk_idx` is past `caller.blocks.len()`.
    BlockIdxOob,
    /// `site_idx` is past `caller.blocks[caller_blk_idx].insts.len()`.
    SiteIdxOob,
    /// Inst at site is not `InstKind::Call(_, _)`. Callers must locate
    /// the exact direct call site before invoking the splice.
    NotCallSite,
    /// `args.len() != callee.params.len()`. Phase 1.0a does not coerce
    /// or truncate.
    ArityMismatch,
    /// `callee.blocks.len() != 1`. Phase 1 narrow scope; multi-block
    /// inlining (block-split + Φ-node handling) arrives in Phase 2.
    NotSingleBlock,
    /// Historical (Phase 1.0a–2.0c): callee body contained a nested
    /// `Call` / `CallIndirect`. Lifted in Phase 2.1 — nested calls
    /// clone verbatim. Kept as unreachable surface (mirrors
    /// `AllocaInBody`).
    NotLeaf,
    /// Callee terminator is not `Ret(_)`. Phase 2+ handles inlined
    /// callees whose CFG re-joins the caller via a return-block
    /// pattern; Phase 1 inlines only straight-line bodies.
    NotRetTerminator,
    /// Shape mismatch between the callee terminator and the call
    /// site result slot: `Ret(Some(_))` without `result_value` (or
    /// the reverse) means the inliner cannot bind the produced value.
    RetShapeMismatch,
    /// Historical (Phase 2.0a–2.0c-A): callee used refcounted SSA
    /// types. Lifted in Phase 2.0c-B — see `splice2::SpliceMultiError
    /// ::RefcountedTypeNotSupported` for why the guard's premise was
    /// wrong. Kept as unreachable surface (mirrors `AllocaInBody`).
    RefcountedTypeNotSupported,
    /// Callee body contains an `Alloca` / `AllocaBytes`. See the
    /// matching variant in `splice2::SpliceMultiError`.
    AllocaInBody,
}

/// Splice a single-block leaf `callee`'s body into `caller` at the
/// `Call(FuncId, _)` instruction in `caller.blocks[caller_blk_idx]
/// .insts[site_idx]`. On success the call instruction is removed and
/// the callee's body is materialised in its place with fresh
/// `ValueId`s for every non-parameter callee value; callee parameters
/// are substituted by the provided `args` directly. If the callee
/// returns a value and the call had a result slot, an
/// `InstKind::Identity` aliasing instruction is appended to bind the
/// callee's `Ret(Some(v))` operand to `result_value`.
///
/// **Phase 1 narrow scope** (each enforced by `SpliceError`):
///
/// * `callee.blocks.len() == 1` — single-block straight-line body.
/// * Callee body contains no nested call (`Call` or `CallIndirect`) —
///   a true leaf. Recursive inlining and call-chain expansion arrive
///   in Phase 2+ along with a call-graph cycle detector.
/// * Callee terminator is `Ret(_)`. Branch-terminated callees require
///   block-split + Φ-node insertion at the call site; Phase 2.
/// * Ret shape matches `result_value`: void in / out, value in / out.
/// * `args.len() == callee.params.len()`.
///
/// The call site itself is constrained too — `site_idx` must point at
/// an `InstKind::Call` inst inside an in-bounds caller block.
///
/// `BlockId` operands do not require remapping: callee's only block is
/// single, its body insts contain no `BlockId` operand fields, and the
/// terminator (`Ret(_)`) is consumed by the splice rather than copied.
/// Phase 2's multi-block path will need a real block-allocator pass.
pub fn inline_single_block_leaf(
    caller: &mut Function,
    caller_blk_idx: usize,
    site_idx: usize,
    callee: &Function,
    args: &[Operand],
    result_value: Option<ValueId>,
) -> Result<(), SpliceError> {
    // ---- 1. Validate Phase 1 narrow constraints.
    if callee.blocks.len() != 1 {
        return Err(SpliceError::NotSingleBlock);
    }
    let callee_blk = &callee.blocks[0];
    // Phase 2.1: NotLeaf guard lifted (see splice2.rs rationale) —
    // nested `Call` / `CallIndirect` insts clone verbatim with their
    // operands remapped; FuncId / SigId are module-level. Phase
    // 2.0c-A2: AllocaInBody guard lifted. Single-block splice does
    // not split the caller block, so cloned Allocas land in the
    // caller's existing block with fresh ValueIds and are picked up
    // by codegen's per-function alloca scan exactly as if the user
    // had written them inline. Both enum variants are kept as
    // unreachable surface.
    let ret_operand = match &callee_blk.term {
        Terminator::Ret(opt) => opt.clone(),
        _ => return Err(SpliceError::NotRetTerminator),
    };
    if ret_operand.is_some() != result_value.is_some() {
        return Err(SpliceError::RetShapeMismatch);
    }
    if args.len() != callee.params.len() {
        return Err(SpliceError::ArityMismatch);
    }
    if caller_blk_idx >= caller.blocks.len() {
        return Err(SpliceError::BlockIdxOob);
    }
    if site_idx >= caller.blocks[caller_blk_idx].insts.len() {
        return Err(SpliceError::SiteIdxOob);
    }
    if !matches!(
        caller.blocks[caller_blk_idx].insts[site_idx].kind,
        InstKind::Call(_, _)
    ) {
        return Err(SpliceError::NotCallSite);
    }
    // Phase 2.0c-B: RefcountedTypeNotSupported guard lifted (matches
    // splice2 — see the rationale comment there).

    // ---- 2. Build callee-ValueId → caller-Operand mapping.
    //
    // Params map directly to the supplied `args` (no fresh allocation).
    // All other callee values get a fresh `ValueId` in `caller.values`
    // whose `ValueInfo` is cloned from the callee — same type, name
    // dropped to avoid namespace collisions in error messages.
    let mut mapping: HashMap<ValueId, Operand> = HashMap::new();
    for (i, param_id) in callee.params.iter().enumerate() {
        mapping.insert(*param_id, args[i].clone());
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
        mapping.insert(ValueId(cv), Operand::Value(fresh));
    }

    // ---- 3. Clone & rewrite callee insts.
    let mut inlined: Vec<Inst> = Vec::with_capacity(callee_blk.insts.len() + 1);
    for inst in &callee_blk.insts {
        let new_kind = rewrite_inst_kind(&inst.kind, &mapping);
        let new_result = inst.result.map(|r| match mapping.get(&r) {
            Some(Operand::Value(v)) => *v,
            _ => unreachable!(
                "non-param callee Inst result must map to a fresh caller ValueId; \
                 mapping invariant broken"
            ),
        });
        inlined.push(Inst {
            result: new_result,
            kind: new_kind,
            origin: inst.origin,
        });
    }

    // ---- 4. Bind callee Ret value to caller result slot via Identity.
    if let (Some(ret_op), Some(r)) = (ret_operand, result_value) {
        let mapped = rewrite_operand(&ret_op, &mapping);
        let origin = inlined.last().and_then(|i| i.origin);
        inlined.push(Inst {
            result: Some(r),
            kind: InstKind::Identity(mapped),
            origin,
        });
    }

    // ---- 5. Splice into caller block, dropping the original call inst.
    let block = &mut caller.blocks[caller_blk_idx];
    let mut new_insts = Vec::with_capacity(block.insts.len() + inlined.len() - 1);
    new_insts.extend(block.insts[..site_idx].iter().cloned());
    new_insts.extend(inlined);
    new_insts.extend(block.insts[site_idx + 1..].iter().cloned());
    block.insts = new_insts;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{
        BinOp, Block, BlockId, FuncId, Function, Inst, InstKind, Operand, Terminator, Type,
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

    fn caller(name: &str, insts: Vec<Inst>) -> Function {
        Function {
            name: name.into(),
            params: vec![],
            ret: Type::Void,
            blocks: vec![Block {
                id: BlockId(0),
                insts,
                term: Terminator::Ret(None),
            }],
            values: vec![],
            current_origin: None,
        }
    }

    /// Construct a callee with a fixed param shape and a `Ret` of a
    /// computed value. The body is one `Add` of param[0] + param[1]
    /// whose result is returned.
    fn add_callee_2_params() -> Function {
        let values = vec![
            ValueInfo {
                ty: Type::I64,
                name: Some("a".into()),
            }, // ValueId(0) param0
            ValueInfo {
                ty: Type::I64,
                name: Some("b".into()),
            }, // ValueId(1) param1
            ValueInfo {
                ty: Type::I64,
                name: Some("sum".into()),
            }, // ValueId(2) sum
        ];
        Function {
            name: "add".into(),
            params: vec![ValueId(0), ValueId(1)],
            ret: Type::I64,
            blocks: vec![Block {
                id: BlockId(0),
                insts: vec![val_inst(
                    ValueId(2),
                    InstKind::BinOp(
                        BinOp::Add,
                        Operand::Value(ValueId(0)),
                        Operand::Value(ValueId(1)),
                    ),
                )],
                term: Terminator::Ret(Some(Operand::Value(ValueId(2)))),
            }],
            values,
            current_origin: None,
        }
    }

    #[test]
    fn splice_value_returning_leaf_into_caller() {
        let mut caller = Function {
            name: "main".into(),
            params: vec![],
            ret: Type::I64,
            blocks: vec![Block {
                id: BlockId(0),
                insts: vec![val_inst(
                    ValueId(0),
                    InstKind::Call(FuncId(1), vec![Operand::ConstI64(3), Operand::ConstI64(4)]),
                )],
                term: Terminator::Ret(Some(Operand::Value(ValueId(0)))),
            }],
            values: vec![ValueInfo {
                ty: Type::I64,
                name: Some("r".into()),
            }],
            current_origin: None,
        };
        let callee = add_callee_2_params();
        let res = inline_single_block_leaf(
            &mut caller,
            0,
            0,
            &callee,
            &[Operand::ConstI64(3), Operand::ConstI64(4)],
            Some(ValueId(0)),
        );
        assert!(res.is_ok());
        let insts = &caller.blocks[0].insts;
        assert_eq!(insts.len(), 2, "1 Add + 1 Identity, call gone");
        match &insts[0].kind {
            InstKind::BinOp(BinOp::Add, Operand::ConstI64(3), Operand::ConstI64(4)) => {}
            other => panic!("expected Add ConstI64 3, ConstI64 4; got {:?}", other),
        }
        let sum_id = insts[0].result.expect("Add must have a result");
        assert_ne!(sum_id, ValueId(0), "callee sum must remap to a fresh id");
        match &insts[1].kind {
            InstKind::Identity(Operand::Value(v)) if *v == sum_id => {}
            other => panic!("expected Identity(Value(sum_id)); got {:?}", other),
        }
        assert_eq!(insts[1].result, Some(ValueId(0)));
    }

    #[test]
    fn splice_void_leaf_into_caller() {
        let callee = Function {
            name: "noop".into(),
            params: vec![],
            ret: Type::Void,
            blocks: vec![Block {
                id: BlockId(0),
                insts: vec![],
                term: Terminator::Ret(None),
            }],
            values: vec![],
            current_origin: None,
        };
        let mut caller = caller("main", vec![void_inst(InstKind::Call(FuncId(1), vec![]))]);
        let before_inst_count = caller.blocks[0].insts.len();
        let res = inline_single_block_leaf(&mut caller, 0, 0, &callee, &[], None);
        assert!(res.is_ok());
        assert_eq!(
            caller.blocks[0].insts.len(),
            before_inst_count - 1,
            "noop callee + void result → call inst alone disappears"
        );
    }

    #[test]
    fn splice_rejects_multi_block_callee() {
        let multi = Function {
            name: "multi".into(),
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
        };
        let mut caller = caller("main", vec![void_inst(InstKind::Call(FuncId(1), vec![]))]);
        let res = inline_single_block_leaf(&mut caller, 0, 0, &multi, &[], None);
        assert_eq!(res, Err(SpliceError::NotSingleBlock));
    }

    #[test]
    fn splice_clones_nested_call_through() {
        // Phase 2.1: a non-leaf callee inlines fine — its nested call
        // is cloned verbatim into the caller body.
        let non_leaf = Function {
            name: "calls_other".into(),
            params: vec![],
            ret: Type::Void,
            blocks: vec![Block {
                id: BlockId(0),
                insts: vec![void_inst(InstKind::Call(FuncId(2), vec![]))],
                term: Terminator::Ret(None),
            }],
            values: vec![],
            current_origin: None,
        };
        let mut caller = caller("main", vec![void_inst(InstKind::Call(FuncId(1), vec![]))]);
        let res = inline_single_block_leaf(&mut caller, 0, 0, &non_leaf, &[], None);
        assert_eq!(res, Ok(()));
        assert_eq!(caller.blocks[0].insts.len(), 1);
        assert!(
            matches!(caller.blocks[0].insts[0].kind, InstKind::Call(FuncId(2), _)),
            "nested call must survive the splice with its callee FuncId intact"
        );
    }

    #[test]
    fn splice_rejects_non_ret_terminator() {
        let branchy = Function {
            name: "diverges".into(),
            params: vec![],
            ret: Type::Void,
            blocks: vec![Block {
                id: BlockId(0),
                insts: vec![],
                term: Terminator::Unreachable,
            }],
            values: vec![],
            current_origin: None,
        };
        let mut caller = caller("main", vec![void_inst(InstKind::Call(FuncId(1), vec![]))]);
        let res = inline_single_block_leaf(&mut caller, 0, 0, &branchy, &[], None);
        assert_eq!(res, Err(SpliceError::NotRetTerminator));
    }

    #[test]
    fn splice_rejects_ret_shape_mismatch() {
        let callee = add_callee_2_params();
        let mut caller = caller(
            "main",
            vec![void_inst(InstKind::Call(
                FuncId(1),
                vec![Operand::ConstI64(0), Operand::ConstI64(0)],
            ))],
        );
        let res = inline_single_block_leaf(
            &mut caller,
            0,
            0,
            &callee,
            &[Operand::ConstI64(0), Operand::ConstI64(0)],
            None,
        );
        assert_eq!(res, Err(SpliceError::RetShapeMismatch));
    }

    #[test]
    fn splice_rejects_arity_mismatch() {
        let callee = add_callee_2_params();
        let mut caller = Function {
            name: "main".into(),
            params: vec![],
            ret: Type::I64,
            blocks: vec![Block {
                id: BlockId(0),
                insts: vec![val_inst(
                    ValueId(0),
                    InstKind::Call(FuncId(1), vec![Operand::ConstI64(3)]),
                )],
                term: Terminator::Ret(Some(Operand::Value(ValueId(0)))),
            }],
            values: vec![ValueInfo {
                ty: Type::I64,
                name: None,
            }],
            current_origin: None,
        };
        let res = inline_single_block_leaf(
            &mut caller,
            0,
            0,
            &callee,
            &[Operand::ConstI64(3)],
            Some(ValueId(0)),
        );
        assert_eq!(res, Err(SpliceError::ArityMismatch));
    }

    #[test]
    fn splice_rejects_non_call_site() {
        let callee = add_callee_2_params();
        let mut caller = caller(
            "main",
            vec![val_inst(
                ValueId(0),
                InstKind::BinOp(BinOp::Add, Operand::ConstI64(1), Operand::ConstI64(2)),
            )],
        );
        caller.values.push(ValueInfo {
            ty: Type::I64,
            name: None,
        });
        let res = inline_single_block_leaf(
            &mut caller,
            0,
            0,
            &callee,
            &[Operand::ConstI64(1), Operand::ConstI64(2)],
            Some(ValueId(0)),
        );
        assert_eq!(res, Err(SpliceError::NotCallSite));
    }

    #[test]
    fn splice_preserves_surrounding_insts_in_caller_block() {
        // Caller block has: [pre = const 1; call leaf; post = const 2].
        // After splice the order is: [pre; <inlined>; post].
        let callee = Function {
            name: "leaf".into(),
            params: vec![],
            ret: Type::I64,
            blocks: vec![Block {
                id: BlockId(0),
                insts: vec![val_inst(
                    ValueId(0),
                    InstKind::BinOp(BinOp::Add, Operand::ConstI64(10), Operand::ConstI64(20)),
                )],
                term: Terminator::Ret(Some(Operand::Value(ValueId(0)))),
            }],
            values: vec![ValueInfo {
                ty: Type::I64,
                name: None,
            }],
            current_origin: None,
        };
        let mut caller = Function {
            name: "main".into(),
            params: vec![],
            ret: Type::I64,
            blocks: vec![Block {
                id: BlockId(0),
                insts: vec![
                    val_inst(
                        ValueId(0),
                        InstKind::BinOp(BinOp::Add, Operand::ConstI64(100), Operand::ConstI64(0)),
                    ),
                    val_inst(ValueId(1), InstKind::Call(FuncId(1), vec![])),
                    val_inst(
                        ValueId(2),
                        InstKind::BinOp(
                            BinOp::Sub,
                            Operand::Value(ValueId(1)),
                            Operand::ConstI64(0),
                        ),
                    ),
                ],
                term: Terminator::Ret(Some(Operand::Value(ValueId(2)))),
            }],
            values: vec![
                ValueInfo {
                    ty: Type::I64,
                    name: None,
                },
                ValueInfo {
                    ty: Type::I64,
                    name: None,
                },
                ValueInfo {
                    ty: Type::I64,
                    name: None,
                },
            ],
            current_origin: None,
        };
        let res = inline_single_block_leaf(&mut caller, 0, 1, &callee, &[], Some(ValueId(1)));
        assert!(res.is_ok());
        let insts = &caller.blocks[0].insts;
        assert_eq!(insts.len(), 4, "pre + inlined Add + Identity + post");
        assert!(
            matches!(
                insts[0].kind,
                InstKind::BinOp(BinOp::Add, Operand::ConstI64(100), _)
            ),
            "pre inst preserved at index 0"
        );
        assert!(
            matches!(
                insts[3].kind,
                InstKind::BinOp(BinOp::Sub, Operand::Value(v), _) if v == ValueId(1)
            ),
            "post inst preserved at end, still consumes ValueId(1)"
        );
    }

    /// Phase 2.0c-B — refcounted callees splice. A Str-typed
    /// passthrough was rejected with `RefcountedTypeNotSupported`
    /// before the guard lift; the splice is type-agnostic (every rc
    /// operation lives inside the callee body and transplants
    /// verbatim), so it must now inline.
    #[test]
    fn refcounted_callee_splices_after_guard_lift() {
        let callee = Function {
            name: "id_str".into(),
            params: vec![ValueId(0)],
            ret: Type::Str,
            blocks: vec![Block {
                id: BlockId(0),
                insts: vec![],
                term: Terminator::Ret(Some(Operand::Value(ValueId(0)))),
            }],
            values: vec![ValueInfo {
                ty: Type::Str,
                name: Some("s".into()),
            }],
            current_origin: None,
        };
        let mut caller = Function {
            name: "main".into(),
            params: vec![ValueId(0)],
            ret: Type::Str,
            blocks: vec![Block {
                id: BlockId(0),
                insts: vec![val_inst(
                    ValueId(1),
                    InstKind::Call(FuncId(1), vec![Operand::Value(ValueId(0))]),
                )],
                term: Terminator::Ret(Some(Operand::Value(ValueId(1)))),
            }],
            values: vec![
                ValueInfo {
                    ty: Type::Str,
                    name: Some("arg".into()),
                },
                ValueInfo {
                    ty: Type::Str,
                    name: Some("r".into()),
                },
            ],
            current_origin: None,
        };
        let res = inline_single_block_leaf(
            &mut caller,
            0,
            0,
            &callee,
            &[Operand::Value(ValueId(0))],
            Some(ValueId(1)),
        );
        assert!(
            res.is_ok(),
            "refcounted callee must splice after guard lift: {:?}",
            res
        );
        assert!(
            caller.blocks[0]
                .insts
                .iter()
                .any(|i| matches!(i.kind, InstKind::Identity(_)) && i.result == Some(ValueId(1))),
            "call must be replaced by an Identity binding the result"
        );
    }
}
