//! Whether a compare is decided by the ranges of its two sides.

use torajs_core::ssa::IPred;

use crate::interval::transfer::NumFact;

pub(super) fn decide(pred: IPred, a: NumFact, b: NumFact) -> Option<bool> {
    match pred {
        IPred::Sgt => decide_ordered(a.lo > b.hi, a.hi <= b.lo),
        IPred::Slt => decide_ordered(a.hi < b.lo, a.lo >= b.hi),
        IPred::Sge => decide_ordered(a.lo >= b.hi, a.hi < b.lo),
        IPred::Sle => decide_ordered(a.hi <= b.lo, a.lo > b.hi),
        IPred::Eq => decide_ordered(a.lo == a.hi && b.lo == b.hi && a.lo == b.lo, disjoint(a, b)),
        IPred::Ne => decide_ordered(disjoint(a, b), a.lo == a.hi && b.lo == b.hi && a.lo == b.lo),
        // unsigned order agrees with signed only when both sides are
        // known non-negative
        IPred::Ult if a.lo >= 0 && b.lo >= 0 => decide(IPred::Slt, a, b),
        IPred::Uge if a.lo >= 0 && b.lo >= 0 => decide(IPred::Sge, a, b),
        IPred::Ult | IPred::Uge => None,
    }
}

fn decide_ordered(always: bool, never: bool) -> Option<bool> {
    match (always, never) {
        (true, _) => Some(true),
        (_, true) => Some(false),
        _ => None,
    }
}

fn disjoint(a: NumFact, b: NumFact) -> bool {
    a.hi < b.lo || a.lo > b.hi
}
