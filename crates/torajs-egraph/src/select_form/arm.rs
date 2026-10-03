//! One arm of a diamond: which blocks it spans, and what of it may run
//! unconditionally in the header.

use torajs_core::ssa::{
    BinOp, BlockId, Function, Inst, InstKind, Operand, Terminator, Type, ValueId,
};

/// Per-arm speculation cap: at most this many non-Copy defs may hoist.
/// A csel replaces one branch (~1 cyc taken + mispredict tail); two
/// ALU ops of straight-line speculation is well under that budget,
/// fatter arms keep their branch. Bench-tunable (blade 2 validation).
const MAX_SPECULATED_INSTS: usize = 2;

/// Instructions safe to execute unconditionally: no memory access, no
/// calls, no trap. NOT `optimize::is_pure` — that is GVN purity, which
/// admits FRem (lowers to a libm `_fmod` BL, regalloc.rs
/// `inst_emits_bl`) and is the wrong question for Load (GVN-impure yet
/// also unsafe to hoist past its guard). SDiv/SRem are trap-free on
/// aarch64 but occupy the divider for ~10 cyc — outside the budget.
pub(super) fn can_speculate(kind: &InstKind) -> bool {
    match kind {
        InstKind::BinOp(op, _, _) => !matches!(op, BinOp::FRem | BinOp::SDiv | BinOp::SRem),
        InstKind::ICmp(_, _, _)
        | InstKind::FCmp(_, _, _)
        | InstKind::Neg(_)
        | InstKind::Ctpop(_)
        | InstKind::SiToFp(_)
        | InstKind::FpToSi(_)
        | InstKind::FpToInt32(_)
        | InstKind::ZExtBoolToI64(_)
        | InstKind::ZExtI32ToI64(_)
        | InstKind::BitCastF64ToI64(_)
        | InstKind::BitCastI64ToF64(_)
        | InstKind::IntToPtr(_)
        | InstKind::PtrToInt(_)
        | InstKind::TruncI64ToBool(_)
        | InstKind::StringRef(_)
        | InstKind::StaticStrRef(_)
        | InstKind::GlobalRef(_)
        | InstKind::FnAddr(_)
        | InstKind::BoxedEntryAddr(_) => true,
        _ => false,
    }
}

/// One arm's dissection: hoistable defs in order, plus the join Copys
/// in order as `(vid, ty, src)`.
pub(super) struct ArmPlan {
    pub(super) hoisted: Vec<Inst>,
    pub(super) copies: Vec<(ValueId, Type, Operand)>,
}

/// Dissect one arm (its blocks in execution order). `None` = arm not
/// convertible.
pub(super) fn plan_arm(func: &Function, blks: &[usize]) -> Option<ArmPlan> {
    let mut hoisted: Vec<Inst> = Vec::new();
    let mut copies: Vec<(ValueId, Type, Operand)> = Vec::new();
    for inst in blks.iter().flat_map(|&b| &func.blocks[b].insts) {
        match &inst.kind {
            InstKind::Copy(ty, src) => {
                let vid = inst.result.expect("Copy always defines a value");
                if copies.iter().any(|(v, _, _)| *v == vid) {
                    return None; // duplicate def in one arm — not our shape
                }
                copies.push((vid, *ty, *src));
            }
            kind if can_speculate(kind) && inst.result.is_some() => {
                // hoisted ahead of the selects, it would read the value
                // from before the diamond instead of this arm's copy
                let mut reads_copy = false;
                torajs_core::ssa::visit_value_operands(kind, |v| {
                    reads_copy |= copies.iter().any(|(c, _, _)| *c == v);
                });
                if reads_copy {
                    return None;
                }
                hoisted.push(inst.clone());
            }
            _ => return None,
        }
    }
    if hoisted.len() > MAX_SPECULATED_INSTS {
        return None;
    }
    Some(ArmPlan { hoisted, copies })
}

/// Longest straight-line chains an arm may span. A growth guard that
/// a later pass folded away leaves its split behind as a block that
/// only branches on (float_demote's post-op check on collatz's `3n+1`).
const MAX_ARM_BLOCKS: usize = 4;

/// The blocks of the arm entered at `first` (BlockId.0 == position,
/// which phi_promote destruct upholds): `first`, then each `br`
/// target with a single predecessor. The first target with more than
/// one is the join. `None` = the arm does not end in a plain `br`.
pub(super) fn arm_chain(
    func: &Function,
    first: BlockId,
    pred_count: impl Fn(BlockId) -> u32,
) -> Option<(Vec<usize>, BlockId)> {
    let mut blks = vec![first.0 as usize];
    loop {
        let Terminator::Br(next) = func.blocks[*blks.last()?].term else {
            return None;
        };
        if pred_count(next) != 1 {
            return Some((blks, next));
        }
        if blks.len() == MAX_ARM_BLOCKS || blks.contains(&(next.0 as usize)) {
            return None;
        }
        blks.push(next.0 as usize);
    }
}
