//! Branch-driven interval refinement — the lightweight LVI/SCCP
//! analogue. A `cond_br` whose condition is a single-def `icmp v, k`
//! (k constant) narrows `v`'s interval on each outgoing edge; an
//! `fcmp oeq (frem n, 2), 0` proves `n` even on the true edge (the
//! parity-guard shape that makes `fdiv n, 2` exact).
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

use torajs_core::ssa::{BlockId, FPred, Function, IPred, InstKind, Operand, Terminator, ValueId};

use super::transfer::{NEG_INF, NumFact, POS_INF, op_const_int};
use crate::dominator::DominatorTree;

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
}

impl Constraint {
    pub fn value(&self) -> ValueId {
        match self {
            Constraint::Range(v, _) | Constraint::Even(v) | Constraint::NonZero(v) => *v,
        }
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
            }
        }
        Self { at_entry }
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
fn edge_constraints(
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
            {
                if op_const_int(two) == Some(2) {
                    return Some(vec![Constraint::Even(*n)]);
                }
            }
            None
        }
        _ => None,
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
        }
    };
    match pred {
        IPred::Eq => Some(NumFact::point(k)),
        IPred::Ne => None,
        IPred::Slt => Some(NumFact::new(NEG_INF, k - 1)),
        IPred::Sle => Some(NumFact::new(NEG_INF, k)),
        IPred::Sgt => Some(NumFact::new(k + 1, POS_INF)),
        IPred::Sge => Some(NumFact::new(k, POS_INF)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
