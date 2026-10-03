//! One arm of a diamond: which blocks it spans, and what of it may run
//! unconditionally in the header.

use torajs_core::ssa::{
    BinOp, BlockId, Function, IPred, Inst, InstKind, Operand, Terminator, Type, ValueId,
};

/// Per-arm speculation cap: at most this many non-Copy defs may hoist.
/// A csel replaces one branch (~1 cyc taken + mispredict tail); two
/// ALU ops of straight-line speculation is well under that budget,
/// fatter arms keep their branch. Bench-tunable (blade 2 validation).
const MAX_SPECULATED_INSTS: usize = 2;

/// The cap for the rest of a switch, `x === A ? a : x === B ? b : …`
/// picking constants. Which case a datum hits is what a predictor
/// cannot learn (a scanner's operator kind, an opcode), so every test
/// left as a branch mispredicts at about the rate its case occurs,
/// where a compare and a csel per case cost a fixed half cycle. Three
/// tests under the header's own: past that the csel chain's latency
/// and the work done for the cases that did not hit stop being small.
const MAX_SWITCH_SPECULATED_INSTS: usize = 6;

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
/// convertible. `switch_on` is the value the header tests against a
/// constant, when that is what it branches on. A `copy` with no other
/// def anywhere is a plain value (a `const` the arm names), hoisted with
/// the arm's defs at no cost.
pub(super) fn plan_arm(
    func: &Function,
    blks: &[usize],
    switch_on: Option<ValueId>,
    def_count: impl Fn(ValueId) -> u32,
) -> Option<ArmPlan> {
    let mut hoisted: Vec<Inst> = Vec::new();
    let mut copies: Vec<(ValueId, Type, Operand)> = Vec::new();
    let mut plain_copies = 0usize;
    for inst in blks.iter().flat_map(|&b| &func.blocks[b].insts) {
        match &inst.kind {
            InstKind::Copy(_, src)
                if inst.result.is_some_and(|r| def_count(r) == 1)
                    && !reads_join_copy(src, &copies) =>
            {
                hoisted.push(inst.clone());
                plain_copies += 1;
            }
            InstKind::Copy(ty, src) => {
                let vid = inst.result.expect("Copy always defines a value");
                if copies.iter().any(|(v, _, _)| *v == vid) {
                    return None; // duplicate def in one arm — not our shape
                }
                copies.push((vid, *ty, *src));
            }
            kind if (can_speculate(kind) || is_select(inst)) && inst.result.is_some() => {
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
    // a select is speculated only as part of a switch: one formed from
    // an inner diamond of any other kind keeps the outer branch
    let cap = if switch_on.is_some_and(|x| is_switch_tail(&hoisted, x)) {
        MAX_SWITCH_SPECULATED_INSTS
    } else if hoisted.iter().any(is_select) {
        return None;
    } else {
        MAX_SPECULATED_INSTS
    };
    (hoisted.len() - plain_copies <= cap).then_some(ArmPlan { hoisted, copies })
}

/// `src` names one of the arm's join copies made before it.
fn reads_join_copy(src: &Operand, copies: &[(ValueId, Type, Operand)]) -> bool {
    matches!(src, Operand::Value(v) if copies.iter().any(|(c, _, _)| c == v))
}

fn is_select(inst: &Inst) -> bool {
    matches!(inst.kind, InstKind::Select(..))
}

fn is_const(op: &Operand) -> bool {
    !matches!(op, Operand::Value(_))
}

/// The value block `h` branches on being equal, or not, to a constant.
pub(super) fn switch_value(func: &Function, h: usize, cond: &Operand) -> Option<ValueId> {
    let Operand::Value(c) = cond else {
        return None;
    };
    let def = func.blocks[h].insts.iter().find(|i| i.result == Some(*c))?;
    match &def.kind {
        InstKind::ICmp(IPred::Eq | IPred::Ne, Operand::Value(x), k) if is_const(k) => Some(*x),
        _ => None,
    }
}

/// `hoisted` is the rest of a switch on `x`: equality tests of `x`
/// against constants, and the selects keyed by those tests that pick
/// between constants or an earlier pick.
fn is_switch_tail(hoisted: &[Inst], x: ValueId) -> bool {
    let mut tests: Vec<ValueId> = Vec::new();
    let mut picks: Vec<ValueId> = Vec::new();
    hoisted.iter().all(|inst| {
        let Some(r) = inst.result else {
            return false;
        };
        match &inst.kind {
            InstKind::ICmp(IPred::Eq, Operand::Value(v), k) if *v == x && is_const(k) => {
                tests.push(r);
                true
            }
            InstKind::Select(_, Operand::Value(c), a, b) if tests.contains(c) => {
                let leaf = |op: &Operand| match op {
                    Operand::Value(p) => picks.contains(p),
                    _ => true,
                };
                let ok = leaf(a) && leaf(b);
                picks.push(r);
                ok
            }
            _ => false,
        }
    })
}

/// Per block position: whether control can reach it from the entry.
pub(super) fn reachable(func: &Function) -> Vec<bool> {
    let mut live = vec![false; func.blocks.len()];
    let mut work = vec![0usize];
    while let Some(b) = work.pop() {
        if b >= live.len() || std::mem::replace(&mut live[b], true) {
            continue;
        }
        match &func.blocks[b].term {
            Terminator::Br(t) => work.push(t.0 as usize),
            Terminator::CondBr {
                then_blk, else_blk, ..
            } => work.extend([then_blk.0 as usize, else_blk.0 as usize]),
            Terminator::Ret(_) | Terminator::Unreachable => {}
        }
    }
    live
}

/// Longest straight-line chains an arm may span, counting the blocks
/// that hold instructions. A growth guard that a later pass folded
/// away leaves its split behind as a block that only branches on
/// (float_demote's post-op check on collatz's `3n+1`), and a run of
/// folded guards leaves a run of them.
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
        let busy = blks.iter().filter(|&&b| !func.blocks[b].insts.is_empty());
        if busy.count() == MAX_ARM_BLOCKS || blks.contains(&(next.0 as usize)) {
            return None;
        }
        blks.push(next.0 as usize);
    }
}
