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
    BlockId, Function, IPred, InstKind, Operand, Terminator, ValueId, visit_value_operands,
};

use super::cmp::{emit_compare_nzcv, ipred_to_cond};
use super::operand::materialize_operand_gpr;
use super::{BranchFixup, BranchKind, OP_SCRATCH_LHS, write_u32};
use crate::enc::{b_cond_imm19, b_imm26, cbnz_x, cbz_x};
use crate::regalloc::Assignment;

/// One compare picked to emit inside its block's CondBr.
pub(crate) struct FusedCmp {
    pub pred: IPred,
    pub lhs: Operand,
    pub rhs: Operand,
}

/// Map of block id → the tail ICmp its CondBr absorbs. The inst-emit
/// loop skips these compares; `emit_fused_condbr` re-emits them fused.
pub(crate) fn fusible_cmps(func: &Function) -> HashMap<u32, FusedCmp> {
    // use counts over every inst operand + terminator operand
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
        if let InstKind::ICmp(pred, lhs, rhs) = &tail.kind {
            fused.insert(
                block.id.0,
                FusedCmp {
                    pred: *pred,
                    lhs: *lhs,
                    rhs: *rhs,
                },
            );
        }
    }
    fused
}

/// Emit the fused form of `CondBr { cond: <tail icmp>, .. }`.
///
/// `cbz`/`cbnz` fold applies only to an eq/ne against `ConstI64(0)`
/// with a Value on the other side — a ConstI32 zero keeps the CMP
/// path (its W-form compare masks the high half that an X-reg CBZ
/// would read, see `emit_compare_nzcv`).
pub(crate) fn emit_fused_condbr(
    bytes: &mut Vec<u8>,
    fixups: &mut Vec<BranchFixup>,
    fc: &FusedCmp,
    then_blk: BlockId,
    else_blk: BlockId,
    alloc: &Assignment,
) {
    let zero_test = match (fc.pred, &fc.lhs, &fc.rhs) {
        (IPred::Eq | IPred::Ne, Operand::Value(_), Operand::ConstI64(0)) => Some(fc.lhs),
        (IPred::Eq | IPred::Ne, Operand::ConstI64(0), Operand::Value(_)) => Some(fc.rhs),
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
            match fc.pred {
                IPred::Eq => cbz_x(rt, 0),
                IPred::Ne => cbnz_x(rt, 0),
                _ => unreachable!("zero_test admits eq/ne only"),
            },
        );
    } else {
        emit_compare_nzcv(bytes, &fc.lhs, &fc.rhs, alloc);
        let site = bytes.len() as u32;
        fixups.push(BranchFixup {
            site_byte_offset: site,
            target_block: then_blk,
            kind: BranchKind::Imm19,
        });
        write_u32(bytes, b_cond_imm19(ipred_to_cond(fc.pred), 0));
    }
    let b_site = bytes.len() as u32;
    fixups.push(BranchFixup {
        site_byte_offset: b_site,
        target_block: else_blk,
        kind: BranchKind::Imm26,
    });
    write_u32(bytes, b_imm26(0));
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
        // no CMP-immediate either — the value branches directly
        assert!(words.iter().any(|w| (*w >> 24) == 0xB4), "cbz present");
        assert!(
            !words.iter().any(|w| (*w >> 24) == 0x54),
            "no b.cond needed"
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
