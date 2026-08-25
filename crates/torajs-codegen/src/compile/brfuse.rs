//! Branch fuse — a CondBr whose cond is the block-tail single-use
//! ICmp skips the CSET/CBNZ detour.
//!
//! The unfused shape pays four instructions and a data dependency per
//! branch (`cmp; cset; cbnz; b`): CSET materializes the predicate the
//! CMP already left in NZCV, and CBNZ re-tests it. When the compare
//! is the last instruction of the block and its result feeds only
//! this terminator, the pair fuses to `cmp; b.cond` — and an eq/ne
//! compare against constant 0 over an I64 value drops the CMP too
//! (`cbz`/`cbnz` on the value register). The block-tail restriction
//! keeps NZCV live across zero intervening instructions; the
//! single-use restriction keeps the (skipped) CSET's 0/1 result
//! unobservable.
//!
//! Collatz decomposition A3
//! (.claude/tasks/2026-07-18/perf-collatz-decomposition.md) — every
//! loop header / parity test in a division loop carries this shape.

use std::collections::HashMap;

use torajs_core::ssa::{
    BlockId, FPred, Function, IPred, InstKind, Operand, Terminator, Type, ValueId,
    visit_value_operands,
};

use crate::reg::Reg;

use super::cmp::{emit_compare_nzcv, fpred_to_cond, ipred_to_cond};
use super::operand::{materialize_operand_fpr, materialize_operand_gpr};
use super::{
    BranchFixup, BranchKind, FP_SCRATCH_LHS, FP_SCRATCH_RHS, OP_SCRATCH_LHS, OP_SCRATCH_RHS,
    write_u32,
};
use crate::enc::{b_cond_imm19, b_imm26, cbnz_x, cbz_x, fcmp_d};
use crate::regalloc::Assignment;

/// One compare picked to emit inside its block's CondBr — integer or
/// float (the mandelbrot decomposition D3: an `FCmp; CondBr` loop
/// exit paid cset+cbnz because only ICmp fused here while the Select
/// path below already took floats).
pub(crate) struct FusedCmp {
    pub pred: FusedSelPred,
    pub lhs: Operand,
    pub rhs: Operand,
}

/// Use counts over every inst operand + terminator operand — shared by
/// the CondBr and Select fuse pickers (single-use gates).
pub(super) fn count_uses(func: &Function) -> HashMap<ValueId, u32> {
    let mut uses: HashMap<ValueId, u32> = HashMap::new();
    let mut bump = |v: ValueId| *uses.entry(v).or_insert(0) += 1;
    for block in &func.blocks {
        for inst in &block.insts {
            visit_value_operands(&inst.kind, &mut bump);
        }
        match &block.term {
            Terminator::Ret(Some(Operand::Value(v))) => bump(*v),
            Terminator::CondBr {
                cond: Operand::Value(v),
                ..
            } => bump(*v),
            _ => {}
        }
    }
    uses
}

/// Map of block id → the tail ICmp its CondBr absorbs. The inst-emit
/// loop skips these compares; `emit_fused_condbr` re-emits them fused.
pub(crate) fn fusible_cmps(func: &Function) -> HashMap<u32, FusedCmp> {
    let uses = count_uses(func);
    let mut fused = HashMap::new();
    for block in &func.blocks {
        let Terminator::CondBr {
            cond: Operand::Value(v),
            ..
        } = &block.term
        else {
            continue;
        };
        let Some(tail) = block.insts.last() else {
            continue;
        };
        if tail.result != Some(*v) || uses.get(v) != Some(&1) {
            continue;
        }
        let pred = match &tail.kind {
            InstKind::ICmp(pred, _, _) => FusedSelPred::Int(*pred),
            // `FPred::One` never fuses: ordered != has no single cond
            // code (`fpred_to_cond` rejects it) — it keeps the 3-inst
            // CSET MI / CSET GT / ORR path in emit_fcmp.
            InstKind::FCmp(pred, _, _) if *pred != FPred::One => FusedSelPred::Float(*pred),
            _ => continue,
        };
        let (InstKind::ICmp(_, lhs, rhs) | InstKind::FCmp(_, lhs, rhs)) = &tail.kind else {
            unreachable!("arm above admits only compares");
        };
        fused.insert(
            block.id.0,
            FusedCmp {
                pred,
                lhs: *lhs,
                rhs: *rhs,
            },
        );
    }
    fused
}

/// A fused compare's predicate — integer or float. Shared by the
/// CondBr fuse (`FusedCmp`) and the Select fuse (`FusedSelCmp`); the
/// CSINC fuse only ever constructs the Int arm.
#[derive(Clone, Copy)]
pub(crate) enum FusedSelPred {
    Int(IPred),
    Float(FPred),
}

pub(crate) struct FusedSelCmp {
    pub pred: FusedSelPred,
    pub lhs: Operand,
    pub rhs: Operand,
}

/// Map of `(block id, cmp inst index)` → the compare absorbed by the
/// Select immediately after it. Same NZCV story as the CondBr fuse:
/// the compare re-emits right before the CSEL (zero intervening
/// instructions), and the single-use gate keeps the skipped CSET's
/// 0/1 result unobservable. Collatz's outer max-update select and
/// every blade-2 formed `icmp; select` pair carry this shape; the
/// parity selects share their cond (use count 2) and keep the
/// cset + cmp #0 path.
///
/// An FCmp cond fuses under two extra gates:
///   - `FPred::One` never fuses — ordered ≠ is a 3-inst
///     CSET MI / CSET GT / ORR sequence with no single cond code
///     (`fpred_to_cond` rejects it).
///   - When the Select's value type is F64, both compare operands
///     must sit in allocated FPRs (`Reg::Fpr`, never V16-V18 — the
///     allocator reserves those out of the pool). The FPR value arms
///     occupy FP_SCRATCH_LHS/RHS, so a compare operand that needed an
///     FPR materialize would clobber a value arm before the FCSEL
///     reads it. Non-F64 selects need no gate: their value arms ride
///     GPR scratches, a different register file.
pub(crate) fn fusible_select_cmps(
    func: &Function,
    alloc: &Assignment,
) -> HashMap<(u32, u32), FusedSelCmp> {
    let uses = count_uses(func);
    let mut fused = HashMap::new();
    for block in &func.blocks {
        for (ii, pair) in block.insts.windows(2).enumerate() {
            let InstKind::Select(sel_ty, Operand::Value(c), _, _) = &pair[1].kind else {
                continue;
            };
            if pair[0].result != Some(*c) || uses.get(c) != Some(&1) {
                continue;
            }
            let (pred, lhs, rhs) = match &pair[0].kind {
                InstKind::ICmp(pred, lhs, rhs) => (FusedSelPred::Int(*pred), lhs, rhs),
                InstKind::FCmp(pred, lhs, rhs) => {
                    if *pred == FPred::One {
                        continue;
                    }
                    if *sel_ty == Type::F64 {
                        let in_fpr = |op: &Operand| {
                            matches!(op, Operand::Value(v)
                                if matches!(alloc.of(*v), Reg::Fpr(_)))
                        };
                        if !(in_fpr(lhs) && in_fpr(rhs)) {
                            continue;
                        }
                    }
                    (FusedSelPred::Float(*pred), lhs, rhs)
                }
                _ => continue,
            };
            fused.insert(
                (block.id.0, ii as u32),
                FusedSelCmp {
                    pred,
                    lhs: *lhs,
                    rhs: *rhs,
                },
            );
        }
    }
    fused
}

/// Predicate inversion for the layout-driven branch flip: when the
/// then-target is the fall-through block, `cmp; b.<pred> then; b
/// else` becomes `cmp; b.<!pred> else` and the hot successor runs on
/// the not-taken path. Exact for integer predicates (no NaN cases).
pub(crate) fn invert_ipred(pred: IPred) -> IPred {
    match pred {
        IPred::Eq => IPred::Ne,
        IPred::Ne => IPred::Eq,
        IPred::Slt => IPred::Sge,
        IPred::Sge => IPred::Slt,
        IPred::Sgt => IPred::Sle,
        IPred::Sle => IPred::Sgt,
    }
}

/// Emit the fused form of `CondBr { cond: <tail icmp/fcmp>, .. }`.
///
/// `invert` branches on the compare's logical negation (the layout-
/// driven flip: then-target falls through, so the taken edge is the
/// else-target). Integer predicates invert in the predicate domain
/// (`invert_ipred`, exact — no NaN cases). Float predicates invert in
/// the condition-code domain: AArch64 defines cond bit 0 as the
/// logical negation of the tested condition (b.<cc^1> takes exactly
/// when b.<cc> would not), and that holds for every NZCV state FCMP
/// can produce *including unordered* — so `!(a > b)` lands on b.LE,
/// which does take on NaN, matching the SSA CondBr semantics of a
/// false compare.
///
/// `cbz`/`cbnz` fold applies only to an integer eq/ne against
/// `ConstI64(0)` with a Value on the other side — a ConstI32 zero
/// keeps the CMP path (its W-form compare masks the high half that an
/// X-reg CBZ would read, see `emit_compare_nzcv`).
///
/// `else_blk: None` = the else-target is the fall-through block; the
/// trailing unconditional B is skipped.
pub(crate) fn emit_fused_condbr(
    bytes: &mut Vec<u8>,
    fixups: &mut Vec<BranchFixup>,
    fc: &FusedCmp,
    invert: bool,
    then_blk: BlockId,
    else_blk: Option<BlockId>,
    alloc: &Assignment,
) {
    let int_pred = match fc.pred {
        FusedSelPred::Int(p) => Some(if invert { invert_ipred(p) } else { p }),
        FusedSelPred::Float(_) => None,
    };
    let zero_test = match (int_pred, &fc.lhs, &fc.rhs) {
        (Some(IPred::Eq | IPred::Ne), Operand::Value(_), Operand::ConstI64(0)) => Some(fc.lhs),
        (Some(IPred::Eq | IPred::Ne), Operand::ConstI64(0), Operand::Value(_)) => Some(fc.rhs),
        _ => None,
    };
    if let Some(val) = zero_test {
        let rt = materialize_operand_gpr(bytes, &val, OP_SCRATCH_LHS, alloc);
        let site = bytes.len() as u32;
        fixups.push(BranchFixup {
            site_byte_offset: site,
            target_block: then_blk,
            kind: BranchKind::Imm19,
        });
        // branch to then_blk when the cond holds: eq → value == 0 → CBZ
        write_u32(
            bytes,
            match int_pred {
                Some(IPred::Eq) => cbz_x(rt, 0),
                Some(IPred::Ne) => cbnz_x(rt, 0),
                _ => unreachable!("zero_test admits eq/ne only"),
            },
        );
    } else {
        let cond = match (int_pred, fc.pred) {
            (Some(p), _) => {
                emit_compare_nzcv(bytes, &fc.lhs, &fc.rhs, alloc);
                ipred_to_cond(p)
            }
            (None, FusedSelPred::Float(p)) => {
                // The terminator owns both FP scratches here — unlike
                // the Select fuse there are no value arms to clobber,
                // so a ConstF64 operand may ride its GPR carrier into
                // V16/V17 freely.
                let rn =
                    materialize_operand_fpr(bytes, &fc.lhs, FP_SCRATCH_LHS, OP_SCRATCH_LHS, alloc);
                let rm =
                    materialize_operand_fpr(bytes, &fc.rhs, FP_SCRATCH_RHS, OP_SCRATCH_RHS, alloc);
                write_u32(bytes, fcmp_d(rn, rm));
                fpred_to_cond(p) ^ (invert as u8)
            }
            (None, FusedSelPred::Int(_)) => unreachable!("int_pred is Some for Int"),
        };
        let site = bytes.len() as u32;
        fixups.push(BranchFixup {
            site_byte_offset: site,
            target_block: then_blk,
            kind: BranchKind::Imm19,
        });
        write_u32(bytes, b_cond_imm19(cond, 0));
    }
    if let Some(else_blk) = else_blk {
        let b_site = bytes.len() as u32;
        fixups.push(BranchFixup {
            site_byte_offset: b_site,
            target_block: else_blk,
            kind: BranchKind::Imm26,
        });
        write_u32(bytes, b_imm26(0));
    }
}

/// Follow chains of empty forwarder blocks (`insts: [], term: Br(t)`)
/// to the branch's real destination, so a taken branch lands on work
/// instead of another unconditional B. Bounded walk — a cycle of
/// empty blocks (degenerate infinite loop) stops forwarding and keeps
/// the last id reached.
pub(crate) fn resolve_forwarded_target(
    blocks_by_id: &HashMap<u32, &torajs_core::ssa::Block>,
    start: BlockId,
) -> BlockId {
    let mut id = start;
    for _ in 0..16 {
        match blocks_by_id.get(&id.0) {
            Some(b) if b.insts.is_empty() => match b.term {
                Terminator::Br(t) if t != id => id = t,
                _ => break,
            },
            _ => break,
        }
    }
    id
}

#[cfg(test)]
mod tests {
    use super::super::compile_function;
    use super::*;
    use torajs_core::ssa::{Block, Inst, Type, ValueInfo};

    /// b0: %0 = zext(true); %1 = icmp(pred, %0, rhs); CondBr(%1, b1, b2)
    /// b1/b2: materialize + ret. `extra_use` adds a second consumer of
    /// the icmp result in b1 (kills the fuse).
    fn icmp_condbr_func(pred: IPred, rhs: Operand, extra_use: bool) -> Function {
        let vi = |n: &str| ValueInfo {
            ty: Type::I64,
            name: Some(n.into()),
        };
        let mk = |result: u32, kind: InstKind| Inst {
            result: Some(ValueId(result)),
            kind,
            origin: None,
        };
        let b1_insts = if extra_use {
            vec![mk(2, InstKind::ZExtBoolToI64(Operand::Value(ValueId(1))))]
        } else {
            vec![mk(2, InstKind::ZExtBoolToI64(Operand::ConstBool(true)))]
        };
        Function {
            name: "f".into(),
            params: vec![],
            ret: Type::I64,
            values: vec![vi("v0"), vi("v1"), vi("v2"), vi("v3")],
            blocks: vec![
                Block {
                    id: BlockId(0),
                    insts: vec![
                        mk(0, InstKind::ZExtBoolToI64(Operand::ConstBool(true))),
                        mk(1, InstKind::ICmp(pred, Operand::Value(ValueId(0)), rhs)),
                    ],
                    term: Terminator::CondBr {
                        cond: Operand::Value(ValueId(1)),
                        then_blk: BlockId(1),
                        else_blk: BlockId(2),
                    },
                },
                Block {
                    id: BlockId(1),
                    insts: b1_insts,
                    term: Terminator::Ret(Some(Operand::Value(ValueId(2)))),
                },
                Block {
                    id: BlockId(2),
                    insts: vec![mk(3, InstKind::ZExtBoolToI64(Operand::ConstBool(false)))],
                    term: Terminator::Ret(Some(Operand::Value(ValueId(3)))),
                },
            ],
            current_origin: None,
        }
    }

    fn words_of(bytes: &[u8]) -> Vec<u32> {
        bytes
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect()
    }

    fn is_cset(w: u32) -> bool {
        (w & 0xFFFF_0FE0) == 0x9A9F_07E0
    }

    #[test]
    fn tail_single_use_icmp_fuses() {
        let func = icmp_condbr_func(IPred::Sgt, Operand::ConstI64(5), false);
        let fused = fusible_cmps(&func);
        assert_eq!(fused.len(), 1);
        assert!(fused.contains_key(&0));
        let words = words_of(&compile_function(&func).bytes);
        assert!(!words.iter().any(|w| is_cset(*w)), "cset must be gone");
        assert_eq!(
            words.iter().filter(|w| (*w >> 24) == 0x54).count(),
            1,
            "one b.cond"
        );
    }

    /// b0: %0 = sitofp(3); %1 = fcmp(pred, %0, 4.0); CondBr(%1, b1, b2).
    fn fcmp_condbr_func(pred: torajs_core::ssa::FPred) -> Function {
        let mk = |result: u32, kind: InstKind| Inst {
            result: Some(ValueId(result)),
            kind,
            origin: None,
        };
        let vi = |ty: Type, n: &str| ValueInfo {
            ty,
            name: Some(n.into()),
        };
        Function {
            name: "f".into(),
            params: vec![],
            ret: Type::I64,
            values: vec![
                vi(Type::F64, "v0"),
                vi(Type::Bool, "v1"),
                vi(Type::I64, "v2"),
                vi(Type::I64, "v3"),
            ],
            blocks: vec![
                Block {
                    id: BlockId(0),
                    insts: vec![
                        mk(0, InstKind::SiToFp(Operand::ConstI64(3))),
                        mk(
                            1,
                            InstKind::FCmp(
                                pred,
                                Operand::Value(ValueId(0)),
                                Operand::ConstF64(4.0),
                            ),
                        ),
                    ],
                    term: Terminator::CondBr {
                        cond: Operand::Value(ValueId(1)),
                        then_blk: BlockId(1),
                        else_blk: BlockId(2),
                    },
                },
                Block {
                    id: BlockId(1),
                    insts: vec![mk(2, InstKind::ZExtBoolToI64(Operand::ConstBool(true)))],
                    term: Terminator::Ret(Some(Operand::Value(ValueId(2)))),
                },
                Block {
                    id: BlockId(2),
                    insts: vec![mk(3, InstKind::ZExtBoolToI64(Operand::ConstBool(false)))],
                    term: Terminator::Ret(Some(Operand::Value(ValueId(3)))),
                },
            ],
            current_origin: None,
        }
    }

    #[test]
    fn tail_single_use_fcmp_fuses() {
        let func = fcmp_condbr_func(torajs_core::ssa::FPred::Ogt);
        let fused = fusible_cmps(&func);
        assert_eq!(fused.len(), 1, "FCmp CondBr must fuse (mandelbrot D3)");
        let words = words_of(&compile_function(&func).bytes);
        assert!(!words.iter().any(|w| is_cset(*w)), "cset must be gone");
        assert_eq!(
            words.iter().filter(|w| (*w >> 24) == 0x54).count(),
            1,
            "one b.cond"
        );
    }

    #[test]
    fn fcmp_one_keeps_the_cset_form() {
        // Ordered != has no single cond code — never fuses.
        let func = fcmp_condbr_func(torajs_core::ssa::FPred::One);
        assert!(fusible_cmps(&func).is_empty());
    }

    #[test]
    fn second_use_keeps_the_cset_form() {
        let func = icmp_condbr_func(IPred::Sgt, Operand::ConstI64(5), true);
        assert!(fusible_cmps(&func).is_empty());
        let words = words_of(&compile_function(&func).bytes);
        assert!(words.iter().any(|w| is_cset(*w)), "cset must remain");
    }

    #[test]
    fn eq_zero_fuses_to_cbz_without_cmp() {
        let func = icmp_condbr_func(IPred::Eq, Operand::ConstI64(0), false);
        assert_eq!(fusible_cmps(&func).len(), 1);
        let words = words_of(&compile_function(&func).bytes);
        assert!(!words.iter().any(|w| is_cset(*w)));
        // no CMP-immediate either — the value branches directly. The
        // then-target is the fall-through block, so the layout flip
        // inverts eq → ne and the CBZ emits as CBNZ to the else.
        assert!(words.iter().any(|w| (*w >> 24) == 0xB5), "cbnz present");
        assert!(
            !words.iter().any(|w| (*w >> 24) == 0x54),
            "no b.cond needed"
        );
    }

    #[test]
    fn br_to_next_block_elides() {
        // b0: [inst] br b1 ; b1: ret — the B is a fall-through.
        let vi = ValueInfo {
            ty: Type::I64,
            name: Some("v".into()),
        };
        let func = Function {
            name: "f".into(),
            params: vec![],
            ret: Type::I64,
            values: vec![vi],
            blocks: vec![
                Block {
                    id: BlockId(0),
                    insts: vec![Inst {
                        result: Some(ValueId(0)),
                        kind: InstKind::ZExtBoolToI64(Operand::ConstBool(true)),
                        origin: None,
                    }],
                    term: Terminator::Br(BlockId(1)),
                },
                Block {
                    id: BlockId(1),
                    insts: vec![],
                    term: Terminator::Ret(Some(Operand::Value(ValueId(0)))),
                },
            ],
            current_origin: None,
        };
        let words = words_of(&compile_function(&func).bytes);
        assert!(
            !words.iter().any(|w| (*w >> 26) == 0b000101),
            "unconditional B must be elided"
        );
    }

    #[test]
    fn forwarder_chain_resolves_and_cycle_stops() {
        let empty_br = |id: u32, t: u32| Block {
            id: BlockId(id),
            insts: vec![],
            term: Terminator::Br(BlockId(t)),
        };
        let blocks = vec![empty_br(0, 1), empty_br(1, 2), empty_br(2, 2)];
        let by_id: HashMap<u32, &Block> = blocks.iter().map(|b| (b.id.0, b)).collect();
        // 0 → 1 → 2; 2 self-loops so the walk parks there
        assert_eq!(resolve_forwarded_target(&by_id, BlockId(0)), BlockId(2));
    }
}
