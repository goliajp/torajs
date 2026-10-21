//! A value's fact as a block reads it: its state met with the
//! constraints live where it is read.

use torajs_core::ssa::Operand;

use super::refine::{self, Constraint};
use super::square;
use super::state::State;
use super::transfer::{self, AbsVal, NumFact};

pub(super) fn lookup(op: &Operand, state: &State, live: &[Constraint]) -> AbsVal {
    if let Some(k) = transfer::op_const_int(op) {
        return AbsVal::Fact(NumFact::point(k));
    }
    let Operand::Value(v) = op else {
        return AbsVal::Top;
    };
    let base = state.get(v).copied().unwrap_or(AbsVal::Bottom);
    let AbsVal::Fact(mut f) = base else {
        return base;
    };
    for c in live {
        match c {
            Constraint::Range(cv, r) if cv == v => {
                if let Some(m) = f.meet(*r) {
                    f = m;
                }
            }
            // W3 C3 — nonzero on this path: trim a zero endpoint.
            // A range with zero strictly inside keeps its bounds
            // (the exclusion is not interval-expressible — sound).
            Constraint::NonZero(cv) if cv == v => {
                if f.lo == 0 && f.hi > 0 {
                    f = NumFact::new(1, f.hi);
                } else if f.hi == 0 && f.lo < 0 {
                    f = NumFact::new(f.lo, -1);
                }
            }
            Constraint::Rel { value, .. } | Constraint::Square { value, .. } if value == v => {
                if let Some(m) = bound_range(f, c, state, live).and_then(|r| f.meet(r)) {
                    f = m;
                }
            }
            _ => {}
        }
    }
    AbsVal::Fact(f)
}

/// The range a relation to another value, or a square bound, puts on
/// a value whose fact is `f`. The comparand's own fact is the
/// flow-insensitive one: the join of everything it is ever defined as.
fn bound_range(f: NumFact, c: &Constraint, state: &State, live: &[Constraint]) -> Option<NumFact> {
    match c {
        Constraint::Rel {
            rel,
            bound,
            via_sitofp,
            ..
        } => match state.get(bound) {
            Some(AbsVal::Fact(known)) => refine::rel_range(*rel, *known, *via_sitofp),
            _ => None,
        },
        Constraint::Square {
            strict,
            bound,
            via_sitofp,
            ..
        } => square::root_range(
            f,
            square::bound_hi(*bound, *via_sitofp, state, live)?,
            *strict,
        ),
        _ => None,
    }
}
