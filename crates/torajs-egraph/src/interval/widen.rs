//! Widening and narrowing of the interval ascent.

use torajs_core::ssa::{Function, InstKind};

use super::transfer::{self, AbsVal, NumFact};

/// Kleene rounds before unstable cells widen to sentinel bounds.
const WIDEN_ROUND: usize = 4;

/// Widening thresholds — every integer constant compared against in
/// the function (±1 for strict/inclusive edges). Standard "widening
/// with thresholds" (Astrée/Miné): a growing bound first jumps to the
/// nearest threshold, giving branch refinement a chance to pin loop
/// counters (`i < 1e7` settles at `[0, 9999999]`) before chained
/// cells (`n = i; n = n & (n-1)`) would otherwise widen to a sentinel
/// the narrowing sweep cannot recover through self-referential joins.
pub(super) fn collect_thresholds(func: &Function) -> Vec<i128> {
    let mut t: Vec<i128> = Vec::new();
    for block in &func.blocks {
        for inst in &block.insts {
            if let InstKind::ICmp(_, a, b) | InstKind::FCmp(_, a, b) = &inst.kind {
                for op in [a, b] {
                    if let Some(k) = transfer::op_const_int(op) {
                        t.extend([k - 1, k, k + 1]);
                    }
                }
            }
        }
    }
    t.sort_unstable();
    t.dedup();
    t
}

/// Ascend step: monotone join, with threshold widening once the round
/// budget is spent — a bound still growing jumps to the next
/// threshold, then to a sentinel.
pub(super) fn widen(old: AbsVal, joined: AbsVal, round: usize, thresholds: &[i128]) -> AbsVal {
    if round < WIDEN_ROUND {
        return joined;
    }
    let (AbsVal::Fact(o), AbsVal::Fact(j)) = (old, joined) else {
        return joined;
    };
    let hi = if j.hi > o.hi {
        thresholds
            .iter()
            .copied()
            .find(|&t| t >= j.hi)
            .unwrap_or(transfer::POS_INF)
    } else {
        j.hi
    };
    let lo = if j.lo < o.lo {
        thresholds
            .iter()
            .rev()
            .copied()
            .find(|&t| t <= j.lo)
            .unwrap_or(transfer::NEG_INF)
    } else {
        j.lo
    };
    AbsVal::Fact(NumFact::new(lo, hi))
}

/// One round's update of a cell from its old fact and the join of its
/// defs: widen while ascending, re-tighten while narrowing.
pub(super) fn step(
    old: AbsVal,
    new: AbsVal,
    narrowing: bool,
    round: usize,
    thresholds: &[i128],
) -> AbsVal {
    if narrowing {
        narrow(old, new)
    } else {
        widen(old, old.join(new), round, thresholds)
    }
}

/// Narrowing step: accept a recomputed fact only when it tightens.
pub(super) fn narrow(old: AbsVal, new: AbsVal) -> AbsVal {
    match (old, new) {
        (AbsVal::Top, AbsVal::Fact(_)) => new,
        (AbsVal::Fact(o), AbsVal::Fact(n)) if o.contains(n) => new,
        _ => old,
    }
}
