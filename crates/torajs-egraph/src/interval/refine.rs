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
//! covers, so a constraint is inherited only by the blocks no path
//! carrying such a def reaches before passing the constraint's block
//! again — and block-local evaluation kills a constraint at the cell's
//! own def site (see `mod.rs`).

use super::SingleDefs;

use torajs_core::ssa::{BlockId, Function, InstKind, Operand, Terminator, ValueId};

use super::edge::edge_constraints;
use super::refine_logic::Logic;
use super::square::{SquareBound, square_edge};
use super::transfer::{NEG_INF, NumFact, POS_INF};
use torajs_core::ssa::dominator::DominatorTree;

pub(super) mod scope;

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
        /// compared as f64 (`sitofp (x * x)`): the bound says something
        /// only below 2^53, where the conversion is exact
        via_sitofp: bool,
    },
    /// The cell's range where the paths into the block meet; see
    /// `merge`. Read as a `Range` once a round has given it one.
    Merge(ValueId, BlockId),
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
            Constraint::Range(v, _)
            | Constraint::Even(v)
            | Constraint::NonZero(v)
            | Constraint::Merge(v, _) => *v,
            Constraint::Rel { value, .. } | Constraint::Square { value, .. } => *value,
        }
    }

    /// Whether a def of a value `is_def` names ends the constraint:
    /// it speaks of that value, or of a square compared against the
    /// value as it was at the compare.
    pub fn killed_by(&self, is_def: impl Fn(ValueId) -> bool) -> bool {
        is_def(self.value())
            || matches!(self, Constraint::Square { bound: SquareBound::Value(b), .. } if is_def(*b))
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

/// Per block: how many edges enter it, and the block it is entered
/// from when that is one edge.
fn pred_census(func: &Function) -> (Vec<u32>, Vec<Option<usize>>) {
    let n = func.blocks.len();
    let mut preds: Vec<u32> = vec![0; n];
    let mut sole_pred: Vec<Option<usize>> = vec![None; n];
    for (bi, block) in func.blocks.iter().enumerate() {
        let mut bump = |b: BlockId| {
            preds[b.0 as usize] += 1;
            sole_pred[b.0 as usize] = Some(bi);
        };
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
    for (p, s) in preds.iter().zip(sole_pred.iter_mut()) {
        if *p != 1 {
            *s = None;
        }
    }
    (preds, sole_pred)
}

impl Constraints {
    /// Collect edge constraints for every conditional branch in `func`.
    /// `defs` maps single-def values to their defining instruction.
    pub fn compute(func: &Function, defs: &SingleDefs<'_>) -> Self {
        let n = func.blocks.len();
        let (preds, sole_pred) = pred_census(func);
        let mut at_entry: Vec<Vec<Constraint>> = vec![Vec::new(); n];
        // a branch on anything but a compare is read through `Logic`
        let mut logic = false;
        for (bi, block) in func.blocks.iter().enumerate() {
            let Terminator::CondBr {
                cond: Operand::Value(c),
                then_blk,
                else_blk,
            } = &block.term
            else {
                continue;
            };
            let kind = defs.get(c);
            logic |= !matches!(kind, Some(InstKind::ICmp(..) | InstKind::FCmp(..)));
            let Some(kind) = kind else { continue };
            for (target, taken) in [(*then_blk, true), (*else_blk, false)] {
                if preds[target.0 as usize] != 1 {
                    continue;
                }
                if let Some(cs) = edge_constraints(kind, taken, defs) {
                    at_entry[target.0 as usize].extend(cs);
                }
                at_entry[target.0 as usize]
                    .extend(square_edge(func, bi, kind, taken, defs, &sole_pred));
            }
        }
        if logic {
            Logic::new(func, defs).extend(&mut at_entry);
        }
        Self { at_entry }
    }

    /// Puts `c` at `b`'s entry.
    pub(super) fn add(&mut self, b: BlockId, c: Constraint) {
        self.at_entry[b.0 as usize].push(c);
    }

    pub fn at(&self, b: BlockId) -> &[Constraint] {
        &self.at_entry[b.0 as usize]
    }

    /// Constraints set at the entry of `b` and of every block up its
    /// idom chain, nearest first, with no regard to defs in between:
    /// sound only for constraints on single-def values.
    pub fn inherited(&self, b: BlockId, dom: &DominatorTree) -> Vec<Constraint> {
        let mut out: Vec<Constraint> = Vec::new();
        let mut cur = b;
        loop {
            out.extend_from_slice(self.at(cur));
            match dom.immediate_dominator(cur) {
                Some(p) if p != cur => cur = p,
                _ => break,
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::super::edge::{icmp_range, rel_pair};
    use super::*;
    use torajs_core::ssa::IPred;

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
    fn index_zero_out_of_bounds_pins_the_length_at_zero() {
        // `0 >=u n` taken / `0 <u n` not taken: n == 0
        assert_eq!(
            icmp_range(IPred::Uge, true, 0, true),
            Some(NumFact::point(0))
        );
        assert_eq!(
            icmp_range(IPred::Ult, false, 0, true),
            Some(NumFact::point(0))
        );
        assert_eq!(icmp_range(IPred::Uge, false, 0, true), None);
        assert_eq!(icmp_range(IPred::Uge, true, 1, true), None);
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
