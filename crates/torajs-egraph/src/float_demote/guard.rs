//! Guard math for float demotion loop versioning (phase 1b-ii). A
//! growth site — `fmul`/`fadd`/`fsub` over an in-set value and an
//! integer constant whose result bound widened past ±(2^53-1) — can
//! still demote when the fast path checks, per iteration, that the
//! variable operand sits inside the window where the i64 result
//! magnitude stays ≤ 2^53-1. Inside the window the f64 op would have
//! been exact (and the i64 op cannot overflow); outside it the fast
//! path side-exits into the preserved f64 loop. Entry checks play the
//! same role for an `sitofp` whose operand bound is not statically
//! inside ±2^53 (the conversion itself would round).

use std::collections::HashMap;

use torajs_core::ssa::{BinOp, BlockId, IPred, InstKind, Operand, ValueId};

use crate::interval::transfer::{NumFact, op_const_int};

/// Largest integer magnitude at which every f64 op stays exact.
pub(crate) const MAX_EXACT: i64 = (1i64 << 53) - 1;
/// Largest integer magnitude an i64 → f64 conversion keeps exact
/// (2^53 itself is representable).
pub(crate) const MAX_SITOFP: i64 = 1i64 << 53;

/// One runtime bound check. The predicate fires on *violation*: a
/// true `operand PRED bound` sends the fast path to the side exit.
#[derive(Debug, Clone, Copy)]
pub(crate) struct GuardCheck {
    pub operand: Operand,
    /// `Sgt` for an upper-bound check, `Slt` for a lower-bound one.
    pub pred: IPred,
    pub bound: i64,
}

/// A growth site needing per-iteration guards. `inst_idx` indexes the
/// block's inst list at collection time — valid until mutation.
/// `post` sites check the result after the op runs (the i64 add/sub
/// cannot overflow under the in-set magnitude invariant), halving the
/// check count against per-operand windows; the side exit still
/// re-enters the slow path before the op, so only the operands — both
/// inside ±2^53 — write back.
#[derive(Debug, Clone)]
pub(crate) struct GuardSite {
    pub block: BlockId,
    pub inst_idx: usize,
    pub result: ValueId,
    pub checks: Vec<GuardCheck>,
    pub post: bool,
}

/// The checks a growth def needs, pruned by what the facts already
/// prove, plus whether they run after the op (post-result). `None` =
/// the shape is not guardable (division-shaped, double-variable
/// multiply, or the window math degenerates). `Some((vec![], _))`
/// only arises when the facts already prove the bound — the def
/// would have classified exact, so callers treat it as not-guardable
/// to stay conservative.
///
/// `fadd`/`fsub` check the *result* against ±(2^53-1) after the i64
/// op runs: with both operands at ≤ 2^53 magnitude (the in-set
/// invariant) the i64 add/sub cannot overflow, and one check covers
/// what per-operand windows needed two-to-four for. `fmul` keeps the
/// pre-op operand window (a large constant could overflow i64 before
/// a post check sees the result).
pub(crate) fn growth_checks(
    kind: &InstKind,
    result: ValueId,
    facts: &HashMap<ValueId, NumFact>,
) -> Option<(Vec<GuardCheck>, bool)> {
    let InstKind::BinOp(op @ (BinOp::FMul | BinOp::FAdd | BinOp::FSub), a, b) = kind else {
        return None;
    };
    let value_ok = |o: &Operand| match o {
        Operand::Value(_) => true,
        other => op_const_int(other).is_some(),
    };
    match op {
        BinOp::FAdd | BinOp::FSub => {
            if !value_ok(a) || !value_ok(b) {
                return None;
            }
            let f = facts.get(&result)?;
            let checks = window_checks(Operand::Value(result), f, -MAX_EXACT, MAX_EXACT);
            if checks.is_empty() {
                None
            } else {
                Some((checks, true))
            }
        }
        BinOp::FMul => {
            let (var, var_first, c) = match (a, b) {
                (Operand::Value(_), Operand::Value(_)) => return None, // no per-operand product window
                (Operand::Value(_), other) => (*a, true, op_const_int(other)?),
                (other, Operand::Value(_)) => (*b, false, op_const_int(other)?),
                _ => return None,
            };
            let (lo, hi) = operand_window(*op, var_first, c)?;
            let Operand::Value(v) = var else {
                return None;
            };
            let f = facts.get(&v)?;
            let checks = window_checks(var, f, lo, hi);
            if checks.is_empty() {
                None
            } else {
                Some((checks, false))
            }
        }
        _ => None,
    }
}

/// The checks an `sitofp` entry needs so the conversion is exact.
pub(crate) fn entry_checks(
    x: &Operand,
    facts: &HashMap<ValueId, NumFact>,
) -> Option<Vec<GuardCheck>> {
    let Operand::Value(v) = x else {
        return None;
    };
    let f = facts.get(&v)?;
    Some(window_checks(*x, f, -MAX_SITOFP, MAX_SITOFP))
}

fn window_checks(operand: Operand, f: &NumFact, lo: i64, hi: i64) -> Vec<GuardCheck> {
    let mut checks = Vec::new();
    if f.hi > hi as i128 {
        checks.push(GuardCheck {
            operand,
            pred: IPred::Sgt,
            bound: hi,
        });
    }
    if f.lo < lo as i128 {
        checks.push(GuardCheck {
            operand,
            pred: IPred::Slt,
            bound: lo,
        });
    }
    checks
}

/// The window the variable operand of a constant multiply must stay
/// inside so the product magnitude is ≤ 2^53-1. Endpoints clamp to
/// i64 (the in-set operand is itself ≤ 2^53-1 in magnitude on the
/// fast path, so a window wider than i64 means the check is vacuous
/// on that side). `var_first` is unused for multiplication but kept
/// for shape symmetry with the call site.
fn operand_window(op: BinOp, _var_first: bool, c: i128) -> Option<(i64, i64)> {
    let m = MAX_EXACT as i128;
    let (lo, hi) = match op {
        BinOp::FMul => {
            if c == 0 {
                return None;
            }
            if c > 0 {
                (div_ceil(-m, c), div_floor(m, c))
            } else {
                (div_ceil(m, c), div_floor(-m, c))
            }
        }
        _ => return None,
    };
    Some((clamp_i64(lo), clamp_i64(hi)))
}

fn clamp_i64(x: i128) -> i64 {
    x.clamp(i64::MIN as i128, i64::MAX as i128) as i64
}

fn div_floor(a: i128, b: i128) -> i128 {
    let q = a / b;
    if a % b != 0 && (a < 0) != (b < 0) {
        q - 1
    } else {
        q
    }
}

fn div_ceil(a: i128, b: i128) -> i128 {
    let q = a / b;
    if a % b != 0 && (a < 0) == (b < 0) {
        q + 1
    } else {
        q
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interval::transfer::{NEG_INF, POS_INF};

    fn unbounded_nonneg() -> NumFact {
        NumFact::new(0, POS_INF)
    }

    #[test]
    fn collatz_mul_window() {
        // 3n must stay ≤ 2^53-1 → n ≤ (2^53-1)/3 = 3002399751580330
        let (lo, hi) = operand_window(BinOp::FMul, false, 3).unwrap();
        assert_eq!(hi, 3_002_399_751_580_330);
        assert_eq!(lo, -3_002_399_751_580_330);
    }

    #[test]
    fn nonneg_fact_drops_lower_check() {
        let mut facts = HashMap::new();
        facts.insert(ValueId(1), unbounded_nonneg());
        let kind = InstKind::BinOp(
            BinOp::FMul,
            Operand::ConstF64(3.0),
            Operand::Value(ValueId(1)),
        );
        let (checks, post) = growth_checks(&kind, ValueId(9), &facts).unwrap();
        assert!(!post, "constant multiply keeps the pre-op window");
        assert_eq!(checks.len(), 1);
        assert!(matches!(checks[0].pred, IPred::Sgt));
        assert_eq!(checks[0].bound, 3_002_399_751_580_330);
    }

    #[test]
    fn add_checks_result_post_op() {
        // the accumulator add checks its result once after the i64 op
        // (no overflow under the in-set magnitude invariant), pruned
        // by the result fact: non-negative → upper check only.
        let mut facts = HashMap::new();
        facts.insert(ValueId(1), unbounded_nonneg());
        facts.insert(ValueId(2), unbounded_nonneg());
        facts.insert(ValueId(9), unbounded_nonneg());
        let kind = InstKind::BinOp(
            BinOp::FAdd,
            Operand::Value(ValueId(1)),
            Operand::Value(ValueId(2)),
        );
        let (checks, post) = growth_checks(&kind, ValueId(9), &facts).unwrap();
        assert!(post);
        assert_eq!(checks.len(), 1);
        assert!(matches!(checks[0].pred, IPred::Sgt));
        assert_eq!(checks[0].operand, Operand::Value(ValueId(9)));
        assert_eq!(checks[0].bound, MAX_EXACT);
    }

    #[test]
    fn add_result_two_sided_when_unbounded() {
        let mut facts = HashMap::new();
        facts.insert(ValueId(1), NumFact::new(NEG_INF, POS_INF));
        facts.insert(ValueId(9), NumFact::new(NEG_INF, POS_INF));
        let kind = InstKind::BinOp(
            BinOp::FAdd,
            Operand::Value(ValueId(1)),
            Operand::ConstF64(1.0),
        );
        let (checks, post) = growth_checks(&kind, ValueId(9), &facts).unwrap();
        assert!(post);
        assert_eq!(checks.len(), 2);
    }

    #[test]
    fn double_variable_mul_not_guardable() {
        let mut facts = HashMap::new();
        facts.insert(ValueId(1), unbounded_nonneg());
        facts.insert(ValueId(2), unbounded_nonneg());
        facts.insert(ValueId(9), unbounded_nonneg());
        let kind = InstKind::BinOp(
            BinOp::FMul,
            Operand::Value(ValueId(1)),
            Operand::Value(ValueId(2)),
        );
        assert!(growth_checks(&kind, ValueId(9), &facts).is_none());
    }

    #[test]
    fn entry_checks_prune_by_fact() {
        let mut facts = HashMap::new();
        facts.insert(ValueId(1), NumFact::new(0, POS_INF));
        let checks = entry_checks(&Operand::Value(ValueId(1)), &facts).unwrap();
        assert_eq!(checks.len(), 1);
        assert_eq!(checks[0].bound, MAX_SITOFP);
    }
}
