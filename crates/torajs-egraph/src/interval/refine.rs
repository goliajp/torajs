//! Branch-driven interval refinement — the lightweight LVI/SCCP
//! analogue. A `cond_br` whose condition is a single-def `icmp v, k`
//! (k constant) narrows `v`'s interval on each outgoing edge; one
//! whose comparand is another value relates the two, and narrows `v`
//! by whatever is known of the comparand where `v` is read; an
//! `fcmp oeq (frem n, 2), 0` proves `n` even on the true edge (the
//! parity-guard shape that makes `fdiv n, 2` exact). A condition
//! merged from a short-circuit `||` / `&&` (a bool cell, a `select`,
//! a copy chain) is read through its arms in `refine_logic`.
//!
//! A constraint attaches to a branch target only when that target's
//! sole predecessor is the branching block (every path into it takes
//! the edge), and is inherited by every dominated block. Loop cells
//! (multi-def Copy) are re-defined inside the region a constraint
//! covers, so inheritance walks the idom chain and drops any
//! constraint whose value is re-defined on a strictly lower chain
//! block — and block-local evaluation kills a constraint at the cell's
//! own def site (see `mod.rs`).

use std::collections::{HashMap, HashSet};

use torajs_core::ssa::{
    Block, BlockId, FPred, Function, IPred, InstKind, Operand, Terminator, ValueId,
};

use super::refine_logic::Logic;
use super::square::{SquareBound, root_in_block, square_constraint};
use super::transfer::{NEG_INF, NumFact, POS_INF, op_const_int};
use torajs_core::ssa::dominator::DominatorTree;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Constraint {
    /// Value lies inside the interval on this path.
    Range(ValueId, NumFact),
    /// Value is provably even on this path (frem-parity guard).
    Even(ValueId),
    /// Value is provably nonzero on this path (W3 C3 — the
    /// `while (b !== 0)` loop-guard shape; consumed as an endpoint
    /// trim when the base range touches zero).
    NonZero(ValueId),
    /// `value <rel> bound` holds on this path. Says something about
    /// `value` only once `bound` has a fact of its own.
    Rel {
        value: ValueId,
        rel: Rel,
        bound: ValueId,
        /// the comparison was made between f64s and `value` is the
        /// i64 under a `sitofp`: sound only for endpoints the
        /// conversion keeps exact
        via_sitofp: bool,
    },
    /// `value * value` stands below `bound` (at or below unless
    /// `strict`) on this path; see `square`.
    Square {
        value: ValueId,
        strict: bool,
        bound: SquareBound,
    },
}

/// How a value stands to the comparand it was branched on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rel {
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    /// unsigned below: inside `[0, bound)`
    Below,
}

impl Constraint {
    pub fn value(&self) -> ValueId {
        match self {
            Constraint::Range(v, _) | Constraint::Even(v) | Constraint::NonZero(v) => *v,
            Constraint::Rel { value, .. } | Constraint::Square { value, .. } => *value,
        }
    }
}

/// 2^53: past it `sitofp` rounds, and `sitofp(i) <= y` no longer
/// says `i <= y`.
const EXACT_LIMIT: i128 = 1 << 53;

/// `value`'s interval under `value <rel> bound`, `bound` lying in
/// `known`. `None` when the endpoint the relation reads is not one
/// the relation can be trusted at.
pub fn rel_range(rel: Rel, known: NumFact, via_sitofp: bool) -> Option<NumFact> {
    let usable = |e: i128| e > NEG_INF && e < POS_INF && (!via_sitofp || e.abs() < EXACT_LIMIT);
    match rel {
        Rel::Lt => usable(known.hi).then(|| NumFact::new(NEG_INF, known.hi - 1)),
        Rel::Le => usable(known.hi).then(|| NumFact::new(NEG_INF, known.hi)),
        Rel::Gt => usable(known.lo).then(|| NumFact::new(known.lo + 1, POS_INF)),
        Rel::Ge => usable(known.lo).then(|| NumFact::new(known.lo, POS_INF)),
        Rel::Eq => (usable(known.lo) && usable(known.hi)).then_some(known),
        Rel::Below => (known.lo >= 0 && known.hi >= 1 && usable(known.hi))
            .then(|| NumFact::new(0, known.hi - 1)),
    }
}

/// Per-block entry constraints, indexed by `BlockId.0` — block ids
/// are dense positions (`BlockId.0 == position`, the splice_body
/// renumber invariant), so a direct-index Vec replaces the historical
/// `HashMap<u32, _>`: `inherited` probes `at` once per idom-chain
/// level, and the per-probe SipHash was the top self-time slice of
/// the 75KB test262 unicode-ident class file's egraph stall
/// (rotation 268 profile).
pub struct Constraints {
    at_entry: Vec<Vec<Constraint>>,
}

impl Constraints {
    /// Collect edge constraints for every conditional branch in `func`.
    /// `defs` maps single-def values to their defining instruction.
    pub fn compute(func: &Function, defs: &HashMap<ValueId, InstKind>) -> Self {
        let n = func.blocks.len();
        let mut preds: Vec<u32> = vec![0; n];
        let mut bump = |b: BlockId| preds[b.0 as usize] += 1;
        for block in &func.blocks {
            match &block.term {
                Terminator::Br(t) => bump(*t),
                Terminator::CondBr {
                    then_blk, else_blk, ..
                } => {
                    bump(*then_blk);
                    bump(*else_blk);
                }
                _ => {}
            }
        }

        let mut at_entry: Vec<Vec<Constraint>> = vec![Vec::new(); n];
        for block in &func.blocks {
            let Terminator::CondBr {
                cond: Operand::Value(c),
                then_blk,
                else_blk,
            } = &block.term
            else {
                continue;
            };
            let Some(kind) = defs.get(c) else { continue };
            for (target, taken) in [(*then_blk, true), (*else_blk, false)] {
                if preds[target.0 as usize] != 1 {
                    continue;
                }
                if let Some(cs) = edge_constraints(kind, taken, defs) {
                    at_entry[target.0 as usize].extend(cs);
                }
                at_entry[target.0 as usize].extend(square_edge(block, kind, taken, defs));
            }
        }
        Logic::new(func, defs).extend(&mut at_entry);
        Self { at_entry }
    }

    /// Every square bound a path states, with its strictness.
    pub fn square_bounds(&self) -> Vec<(SquareBound, bool)> {
        let mut out: Vec<(SquareBound, bool)> = Vec::new();
        for c in self.at_entry.iter().flatten() {
            if let Constraint::Square { strict, bound, .. } = c
                && !out.contains(&(*bound, *strict))
            {
                out.push((*bound, *strict));
            }
        }
        out
    }

    pub fn at(&self, b: BlockId) -> &[Constraint] {
        &self.at_entry[b.0 as usize]
    }

    /// Constraints valid at `b`'s entry, inherited down the idom
    /// chain. A constraint from a chain block is dropped when its
    /// value is re-defined in a strictly lower chain block — `b`'s
    /// own defs do not kill (they sit after the entry; block-local
    /// evaluation kills at the cell's def site, so a tail
    /// `i = i + 1` still reads its loop bound at the increment).
    pub fn inherited(
        &self,
        b: BlockId,
        dom: &DominatorTree,
        block_defs: &[HashSet<ValueId>],
    ) -> Vec<Constraint> {
        let mut out: Vec<Constraint> = Vec::new();
        let mut killed: HashSet<ValueId> = HashSet::new();
        let mut cur = b;
        loop {
            for c in self.at(cur) {
                if !killed.contains(&c.value()) {
                    out.push(*c);
                }
            }
            // defs inside a strictly lower chain block kill
            // constraints inherited from above
            if cur != b {
                killed.extend(block_defs[cur.0 as usize].iter().copied());
            }
            match dom.immediate_dominator(cur) {
                Some(p) if p != cur => cur = p,
                _ => break,
            }
        }
        out
    }
}

/// Constraints implied by taking (`taken`) / not taking the branch
/// guarded by `kind`.
pub(super) fn edge_constraints(
    kind: &InstKind,
    taken: bool,
    defs: &HashMap<ValueId, InstKind>,
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
                if k == 0 && nonzero_edge(*pred, taken) {
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

/// What an integer compare of a square, defined in the branching
/// `block`, says of the square's root on the edge.
fn square_edge(
    block: &Block,
    kind: &InstKind,
    taken: bool,
    defs: &HashMap<ValueId, InstKind>,
) -> Option<Constraint> {
    let InstKind::ICmp(pred, a, b) = kind else {
        return None;
    };
    let rel = edge_rel(*pred, taken)?;
    [(a, b, true), (b, a, false)]
        .into_iter()
        .find_map(|(sq, other, left)| {
            let Operand::Value(m) = sq else { return None };
            let root = root_in_block(block, *m, defs)?;
            square_constraint(root, rel, left, other)
        })
}

/// The signed predicate an ordered f64 compare reads as between
/// integer-valued operands.
fn int_pred(pred: FPred) -> Option<IPred> {
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
fn under_sitofp(v: ValueId, defs: &HashMap<ValueId, InstKind>) -> Option<ValueId> {
    match defs.get(&v) {
        Some(InstKind::SiToFp(Operand::Value(i))) => Some(*i),
        _ => None,
    }
}

/// `x <pred> y` on the taken / not-taken edge, read both ways: what
/// it says of `x` given `y`, then what it says of `y` given `x`.
/// Empty for an edge that relates nothing (`ne` taken, the far side
/// of an unsigned compare).
fn rel_pair(pred: IPred, taken: bool, x: ValueId, y: ValueId, via_sitofp: bool) -> Vec<Constraint> {
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
fn edge_rel(pred: IPred, taken: bool) -> Option<Rel> {
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
fn nonzero_edge(pred: IPred, taken: bool) -> bool {
    matches!((pred, taken), (IPred::Ne, true) | (IPred::Eq, false))
}

/// fcmp counterpart of [`nonzero_edge`]. `One` and `Une` agree on the
/// nonzero verdict (NaN ≠ 0 either way); `Oeq` proves nonzero on the
/// not-taken edge only for non-NaN values — but NaN has no interval
/// fact, so the trim never applies to it.
fn fcmp_nonzero_edge(pred: FPred, taken: bool) -> bool {
    matches!(
        (pred, taken),
        (FPred::Une | FPred::One, true) | (FPred::Oeq, false)
    )
}

/// Interval implied for the value side of `value <pred> k` (or
/// `k <pred> value` when `flipped`) on the `taken`/not-taken edge.
fn icmp_range(pred: IPred, taken: bool, k: i128, flipped: bool) -> Option<NumFact> {
    // `x ult k` bounds x only on its taken edge, and only for k >= 1;
    // the other edge (x >= k or x < 0) and the mirrored form say nothing
    // an interval can hold
    match (pred, flipped, taken) {
        (IPred::Ult, false, true) | (IPred::Uge, false, false) if k >= 1 => {
            return Some(NumFact::new(0, k - 1));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icmp_range_unsigned_bounds_only_the_in_range_edge() {
        assert_eq!(
            icmp_range(IPred::Ult, true, 16, false),
            Some(NumFact::new(0, 15))
        );
        assert_eq!(
            icmp_range(IPred::Uge, false, 16, false),
            Some(NumFact::new(0, 15))
        );
        assert_eq!(icmp_range(IPred::Ult, false, 16, false), None);
        assert_eq!(icmp_range(IPred::Uge, true, 16, false), None);
        assert_eq!(icmp_range(IPred::Ult, true, 16, true), None);
        assert_eq!(icmp_range(IPred::Ult, true, 0, false), None);
    }

    #[test]
    fn a_relation_reads_the_endpoint_facing_the_value() {
        let known = NumFact::new(3, 10);
        assert_eq!(
            rel_range(Rel::Lt, known, false),
            Some(NumFact::new(NEG_INF, 9))
        );
        assert_eq!(
            rel_range(Rel::Le, known, false),
            Some(NumFact::new(NEG_INF, 10))
        );
        assert_eq!(
            rel_range(Rel::Gt, known, false),
            Some(NumFact::new(4, POS_INF))
        );
        assert_eq!(
            rel_range(Rel::Ge, known, false),
            Some(NumFact::new(3, POS_INF))
        );
        assert_eq!(rel_range(Rel::Eq, known, false), Some(known));
        assert_eq!(
            rel_range(Rel::Below, known, false),
            Some(NumFact::new(0, 9))
        );
    }

    #[test]
    fn a_relation_trusts_no_endpoint_it_cannot_read() {
        let open = NumFact::new(0, POS_INF);
        assert_eq!(rel_range(Rel::Lt, open, false), None);
        assert_eq!(
            rel_range(Rel::Ge, open, false),
            Some(NumFact::new(0, POS_INF))
        );
        // may be negative: unsigned-below says nothing signed
        assert_eq!(rel_range(Rel::Below, NumFact::new(-1, 10), false), None);
        // through sitofp, 2^53 is where the conversion starts rounding
        let edge = NumFact::new(0, 1 << 53);
        assert_eq!(rel_range(Rel::Le, edge, true), None);
        assert_eq!(
            rel_range(Rel::Le, edge, false),
            Some(NumFact::new(NEG_INF, 1 << 53))
        );
        let inside = NumFact::new(0, (1 << 53) - 1);
        assert_eq!(
            rel_range(Rel::Le, inside, true),
            Some(NumFact::new(NEG_INF, (1 << 53) - 1))
        );
    }

    #[test]
    fn each_edge_of_a_compare_relates_both_operands() {
        let (x, y) = (ValueId(1), ValueId(2));
        let rels = |pred, taken| -> Vec<(ValueId, Rel, ValueId)> {
            rel_pair(pred, taken, x, y, false)
                .into_iter()
                .map(|c| match c {
                    Constraint::Rel {
                        value, rel, bound, ..
                    } => (value, rel, bound),
                    other => panic!("not a relation: {other:?}"),
                })
                .collect()
        };
        assert_eq!(
            rels(IPred::Slt, true),
            vec![(x, Rel::Lt, y), (y, Rel::Gt, x)]
        );
        assert_eq!(
            rels(IPred::Slt, false),
            vec![(x, Rel::Ge, y), (y, Rel::Le, x)]
        );
        assert_eq!(rels(IPred::Ult, true), vec![(x, Rel::Below, y)]);
        assert!(rels(IPred::Ult, false).is_empty());
        assert!(rels(IPred::Ne, true).is_empty());
        assert_eq!(
            rels(IPred::Ne, false),
            vec![(x, Rel::Eq, y), (y, Rel::Eq, x)]
        );
    }

    #[test]
    fn icmp_range_directions() {
        assert_eq!(
            icmp_range(IPred::Slt, true, 10, false),
            Some(NumFact::new(NEG_INF, 9))
        );
        assert_eq!(
            icmp_range(IPred::Slt, false, 10, false),
            Some(NumFact::new(10, POS_INF))
        );
        assert_eq!(
            icmp_range(IPred::Sle, true, 1_000_000, false),
            Some(NumFact::new(NEG_INF, 1_000_000))
        );
        // `5 < v` mirrored: v > 5
        assert_eq!(
            icmp_range(IPred::Slt, true, 5, true),
            Some(NumFact::new(6, POS_INF))
        );
        assert_eq!(
            icmp_range(IPred::Eq, true, 7, false),
            Some(NumFact::point(7))
        );
        assert_eq!(icmp_range(IPred::Ne, true, 7, false), None);
    }
}
