//! What one branch edge says: the constraints taking (or not taking)
//! a conditional branch puts on the values its compare reads.

use super::SingleDefs;

use torajs_core::ssa::{FPred, IPred, InstKind, Operand, ValueId};

use super::refine::{Constraint, Rel};
use super::transfer::{NEG_INF, NumFact, POS_INF, op_const_int};

/// Constraints implied by taking (`taken`) / not taking the branch
/// guarded by `kind`.
pub(super) fn edge_constraints(
    kind: &InstKind,
    taken: bool,
    defs: &SingleDefs<'_>,
) -> Option<Vec<Constraint>> {
    match kind {
        InstKind::ICmp(pred, a, b) => {
            let mut out = Vec::new();
            if let (Operand::Value(v), Some(k)) = (a, op_const_int(b)) {
                if let Some(r) = icmp_range(*pred, taken, k, false) {
                    out.push(Constraint::Range(*v, r));
                }
                if k == 0 && nonzero_edge(*pred, taken) {
                    out.push(Constraint::NonZero(*v));
                }
            }
            if let (Some(k), Operand::Value(v)) = (op_const_int(a), b) {
                if let Some(r) = icmp_range(*pred, taken, k, true) {
                    out.push(Constraint::Range(*v, r));
                }
                if k == 0 && (nonzero_edge(*pred, taken) || index_zero_in_bounds(*pred, taken)) {
                    out.push(Constraint::NonZero(*v));
                }
            }
            if let (Operand::Value(x), Operand::Value(y)) = (a, b) {
                out.extend(rel_pair(*pred, taken, *x, *y, false));
            }
            (!out.is_empty()).then_some(out)
        }
        // an ordered compare between two values, one of them the f64
        // image of an i64: both relate, and so does the i64 itself.
        // The not-taken edge negates only for operands that are not
        // NaN, which is every operand that has a fact to read.
        InstKind::FCmp(pred, Operand::Value(x), Operand::Value(y))
            if int_pred(*pred).is_some()
                && (under_sitofp(*x, defs).is_some() || under_sitofp(*y, defs).is_some()) =>
        {
            let pred = int_pred(*pred)?;
            let mut out = rel_pair(pred, taken, *x, *y, true);
            if let Some(ix) = under_sitofp(*x, defs) {
                out.extend(rel_pair(pred, taken, ix, *y, true).into_iter().take(1));
            }
            if let Some(iy) = under_sitofp(*y, defs) {
                out.extend(rel_pair(pred, taken, *x, iy, true).into_iter().skip(1));
            }
            (!out.is_empty()).then_some(out)
        }
        // fcmp une/one (v, 0) taken — or oeq not taken — proves the
        // value nonzero (W3 C3; ±0 both compare equal to 0, so the
        // nonzero edge excludes the whole zero class).
        InstKind::FCmp(pred, a, b) if fcmp_nonzero_edge(*pred, taken) => {
            let v = match (a, b) {
                (Operand::Value(v), z) if op_const_int(z) == Some(0) => Some(*v),
                (z, Operand::Value(v)) if op_const_int(z) == Some(0) => Some(*v),
                _ => None,
            }?;
            Some(vec![Constraint::NonZero(v)])
        }
        // fcmp oeq (frem n, 2), 0 — true edge proves n even
        InstKind::FCmp(FPred::Oeq, Operand::Value(r), z) if taken && op_const_int(z) == Some(0) => {
            if let Some(InstKind::BinOp(torajs_core::ssa::BinOp::FRem, Operand::Value(n), two)) =
                defs.get(r)
                && op_const_int(two) == Some(2)
            {
                return Some(vec![Constraint::Even(*n)]);
            }
            None
        }
        _ => None,
    }
}

/// The signed predicate an ordered f64 compare reads as between
/// integer-valued operands.
pub(super) fn int_pred(pred: FPred) -> Option<IPred> {
    match pred {
        FPred::Olt => Some(IPred::Slt),
        FPred::Ole => Some(IPred::Sle),
        FPred::Ogt => Some(IPred::Sgt),
        FPred::Oge => Some(IPred::Sge),
        FPred::Oeq => Some(IPred::Eq),
        FPred::One | FPred::Une => None,
    }
}

/// The i64 a single-def `sitofp` converts.
pub(super) fn under_sitofp(v: ValueId, defs: &SingleDefs<'_>) -> Option<ValueId> {
    match defs.get(&v) {
        Some(InstKind::SiToFp(Operand::Value(i))) => Some(*i),
        _ => None,
    }
}

/// `x <pred> y` on the taken / not-taken edge, read both ways: what
/// it says of `x` given `y`, then what it says of `y` given `x`.
/// Empty for an edge that relates nothing (`ne` taken, the far side
/// of an unsigned compare).
pub(super) fn rel_pair(
    pred: IPred,
    taken: bool,
    x: ValueId,
    y: ValueId,
    via_sitofp: bool,
) -> Vec<Constraint> {
    let Some(of_x) = edge_rel(pred, taken) else {
        return Vec::new();
    };
    let of_y = match of_x {
        Rel::Lt => Some(Rel::Gt),
        Rel::Le => Some(Rel::Ge),
        Rel::Gt => Some(Rel::Lt),
        Rel::Ge => Some(Rel::Le),
        Rel::Eq => Some(Rel::Eq),
        // `x` below `y` leaves `y` above a value of unknown sign
        Rel::Below => None,
    };
    let mut out = vec![Constraint::Rel {
        value: x,
        rel: of_x,
        bound: y,
        via_sitofp,
    }];
    out.extend(of_y.map(|rel| Constraint::Rel {
        value: y,
        rel,
        bound: x,
        via_sitofp,
    }));
    out
}

/// What `value <pred> bound` says of `value` on the edge.
pub(super) fn edge_rel(pred: IPred, taken: bool) -> Option<Rel> {
    match (pred, taken) {
        (IPred::Ult, true) | (IPred::Uge, false) => Some(Rel::Below),
        (IPred::Ult | IPred::Uge, _) => None,
        (IPred::Eq, true) | (IPred::Ne, false) => Some(Rel::Eq),
        (IPred::Eq | IPred::Ne, _) => None,
        (IPred::Slt, true) | (IPred::Sge, false) => Some(Rel::Lt),
        (IPred::Sle, true) | (IPred::Sgt, false) => Some(Rel::Le),
        (IPred::Sgt, true) | (IPred::Sle, false) => Some(Rel::Gt),
        (IPred::Sge, true) | (IPred::Slt, false) => Some(Rel::Ge),
    }
}

/// The edge (taken / not taken) on which `value <pred> 0` proves the
/// value nonzero. Eq/Ne only — ordering predicates already carry
/// their information through `icmp_range`.
pub(super) fn nonzero_edge(pred: IPred, taken: bool) -> bool {
    matches!((pred, taken), (IPred::Ne, true) | (IPred::Eq, false))
}

/// The edge on which `0 <pred> value`, an unsigned bounds check of
/// index 0 (the pop / shift empty guard), proves the value nonzero:
/// `0 <u n` taken, `0 >=u n` not taken.
pub(super) fn index_zero_in_bounds(pred: IPred, taken: bool) -> bool {
    matches!((pred, taken), (IPred::Ult, true) | (IPred::Uge, false))
}

/// fcmp counterpart of [`nonzero_edge`]. `One` and `Une` agree on the
/// nonzero verdict (NaN ≠ 0 either way); `Oeq` proves nonzero on the
/// not-taken edge only for non-NaN values — but NaN has no interval
/// fact, so the trim never applies to it.
pub(super) fn fcmp_nonzero_edge(pred: FPred, taken: bool) -> bool {
    matches!(
        (pred, taken),
        (FPred::Une | FPred::One, true) | (FPred::Oeq, false)
    )
}

/// Interval implied for the value side of `value <pred> k` (or
/// `k <pred> value` when `flipped`) on the `taken`/not-taken edge.
pub(super) fn icmp_range(pred: IPred, taken: bool, k: i128, flipped: bool) -> Option<NumFact> {
    // `x ult k` bounds x only on its taken edge, and only for k >= 1;
    // the other edge (x >= k or x < 0) says nothing an interval can
    // hold, nor does the mirrored form past k == 0
    match (pred, flipped, taken) {
        (IPred::Ult, false, true) | (IPred::Uge, false, false) if k >= 1 => {
            return Some(NumFact::new(0, k - 1));
        }
        // `0 >=u x` holds for x == 0 alone
        (IPred::Uge, true, true) | (IPred::Ult, true, false) if k == 0 => {
            return Some(NumFact::point(0));
        }
        (IPred::Ult | IPred::Uge, ..) => return None,
        _ => {}
    }
    // normalize to value-on-the-left by mirroring the predicate
    let pred = if flipped {
        match pred {
            IPred::Slt => IPred::Sgt,
            IPred::Sgt => IPred::Slt,
            IPred::Sle => IPred::Sge,
            IPred::Sge => IPred::Sle,
            p => p,
        }
    } else {
        pred
    };
    // not-taken edge = negated predicate
    let pred = if taken {
        pred
    } else {
        match pred {
            IPred::Eq => IPred::Ne,
            IPred::Ne => IPred::Eq,
            IPred::Slt => IPred::Sge,
            IPred::Sge => IPred::Slt,
            IPred::Sgt => IPred::Sle,
            IPred::Sle => IPred::Sgt,
            IPred::Ult | IPred::Uge => unreachable!("returned above"),
        }
    };
    match pred {
        IPred::Eq => Some(NumFact::point(k)),
        IPred::Ne => None,
        IPred::Slt => Some(NumFact::new(NEG_INF, k - 1)),
        IPred::Sle => Some(NumFact::new(NEG_INF, k)),
        IPred::Sgt => Some(NumFact::new(k + 1, POS_INF)),
        IPred::Sge => Some(NumFact::new(k, POS_INF)),
        IPred::Ult | IPred::Uge => unreachable!("returned above"),
    }
}
