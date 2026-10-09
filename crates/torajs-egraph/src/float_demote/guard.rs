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

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{BinOp, BlockId, Function, InstKind, Operand, ValueId};

use crate::interval::transfer::{NEG_INF, NumFact, POS_INF, op_const_int};

use super::fit;

/// Largest integer magnitude at which every f64 op stays exact.
pub(crate) const MAX_EXACT: i64 = (1i64 << 53) - 1;
/// Largest integer magnitude an i64 → f64 conversion keeps exact
/// (2^53 itself is representable).
pub(crate) const MAX_SITOFP: i64 = 1i64 << 53;

/// One runtime bound check. The test fires on *violation*: a true
/// test sends the fast path to the side exit.
#[derive(Debug, Clone, Copy)]
pub(crate) struct GuardCheck {
    pub operand: Operand,
    pub test: Test,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Test {
    /// `operand > bound`
    Above(i64),
    /// `operand < bound`
    Below(i64),
    /// `operand >> k != 0`: the operand left `[0, 2^k)`. A window
    /// whose lower side the facts prove rounds its upper bound down to
    /// a power of two — a shift and a compare-with-zero, so no wide
    /// constant has to live in a register across the loop (the
    /// hoisted bound of a `cmp` form costs a callee-saved register
    /// for the whole function, and spills what it evicts). The
    /// values between `2^k` and the exact bound take the f64 path,
    /// which is the same arithmetic.
    HighBits(u32),
}

/// A growth site needing per-iteration guards. `inst_idx` indexes the
/// block's inst list at collection time — valid until mutation.
/// `post` sites (see `growth_checks`) check the result after the op
/// runs; their side exit enters the slow path after the op and writes
/// the i64 result back through `sitofp`, so an accumulator `t = t + d`
/// updates in place instead of holding the old `t` past the check.
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
/// op runs, one check where per-operand windows needed two-to-four. A
/// constant operand must be ≤ 2^53-1 in magnitude: with a value operand
/// ≤ 2^53 (the in-set invariant) the i64 add/sub cannot wrap, and the
/// side exit writes back the exact sum for `sitofp` to round once, as
/// the f64 op would. `fmul` keeps the pre-op window (a large constant
/// could overflow i64 before a post check sees the result).
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
        other => op_const_int(other).is_some_and(|c| c.abs() <= MAX_EXACT as i128),
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
    let f = facts.get(v)?;
    Some(window_checks(*x, f, -MAX_SITOFP, MAX_SITOFP))
}

/// The checks keeping `operand` inside `[lo, hi]`, pruned by its
/// fact `f`. Every window here has `lo <= 0 <= hi`, so a non-negative
/// fact needs the upper side only, and that one takes the shift form.
fn window_checks(operand: Operand, f: &NumFact, lo: i64, hi: i64) -> Vec<GuardCheck> {
    let mut checks = Vec::new();
    if f.hi > hi as i128 {
        let test = if f.lo >= 0 && hi >= 1 {
            Test::HighBits(63 - (hi as u64 + 1).leading_zeros())
        } else {
            Test::Above(hi)
        };
        checks.push(GuardCheck { operand, test });
    }
    if f.lo < lo as i128 {
        checks.push(GuardCheck {
            operand,
            test: Test::Below(lo),
        });
    }
    checks
}

/// The non-exact (guarded) def sites of `set`, split into growth
/// sites and `sitofp` entries. Entry sites carry their inst position
/// too: one whose operand is defined inside the hosting region
/// re-classifies as a per-iteration growth site during planning (the
/// `sum += xs[j]` loop-body sitofp shape).
pub(crate) fn collect_guard_sites(
    func: &Function,
    set: &HashSet<ValueId>,
    facts: &HashMap<ValueId, NumFact>,
) -> (Vec<GuardSite>, Vec<GuardSite>) {
    let mut growth: Vec<GuardSite> = Vec::new();
    let mut entries: Vec<GuardSite> = Vec::new();
    for block in &func.blocks {
        for (i, inst) in block.insts.iter().enumerate() {
            let Some(r) = inst.result else { continue };
            if !set.contains(&r) || fit::def_exact(&inst.kind, &r, facts, set) {
                continue;
            }
            let site = |checks, post| GuardSite {
                block: block.id,
                inst_idx: i,
                result: r,
                checks,
                post,
            };
            match &inst.kind {
                InstKind::SiToFp(x) => {
                    if let Some(checks) = entry_checks(x, facts) {
                        entries.push(site(checks, false));
                    }
                }
                _ => {
                    if let Some((checks, post)) = growth_checks(&inst.kind, r, facts) {
                        growth.push(site(checks, post));
                    }
                }
            }
        }
    }
    absorb_chains(&mut growth, func, facts);
    (growth, entries)
}

/// One window for a `v * k + d` chain. The post check of an add or
/// sub whose one operand is a constant-multiply growth site and whose
/// other operand `d` is bounded folds onto the multiply's pre-op
/// window on `v`: the product must land in the band that keeps
/// `product ± d` inside ±(2^53-1) for every `d`, and `k` divides
/// that band into the window. The add then needs no check of its
/// own — the digit-parse accumulator `v * 10 + (d - 48)` pays one
/// test per iteration instead of two.
fn absorb_chains(sites: &mut Vec<GuardSite>, func: &Function, facts: &HashMap<ValueId, NumFact>) {
    let kind_of = |s: &GuardSite| &func.blocks[s.block.0 as usize].insts[s.inst_idx].kind;
    let m = MAX_EXACT as i128;
    let mut absorbed: Vec<usize> = Vec::new();
    for t in 0..sites.len() {
        if !sites[t].post {
            continue;
        }
        let InstKind::BinOp(op @ (BinOp::FAdd | BinOp::FSub), a, b) = kind_of(&sites[t]) else {
            continue;
        };
        let sub = matches!(op, BinOp::FSub);
        let by_result = |o: &Operand| match o {
            Operand::Value(r) => sites.iter().position(|s| !s.post && s.result == *r),
            _ => None,
        };
        // the product side and the bounded side of the chain
        let (src, prod_first, d) = match (by_result(a), by_result(b)) {
            (Some(s), _) => (s, true, b),
            (_, Some(s)) => (s, false, a),
            _ => continue,
        };
        let InstKind::BinOp(BinOp::FMul, ma, mb) = kind_of(&sites[src]) else {
            continue;
        };
        let (v, k) = match (ma, mb) {
            (Operand::Value(v), c) | (c, Operand::Value(v)) => match op_const_int(c) {
                Some(k) if k != 0 => (*v, k),
                _ => continue,
            },
            _ => continue,
        };
        let Some(d) = bounded(d, facts) else { continue };
        // product band: `p + d`, `p - d`, `d - p` each inside ±M
        let (plo, phi) = match (sub, prod_first) {
            (false, _) => (-m - d.lo, m - d.hi),
            (true, true) => (-m + d.hi, m + d.lo),
            (true, false) => (d.hi - m, d.lo + m),
        };
        let (plo, phi) = (plo.max(-m), phi.min(m));
        let (wlo, whi) = if k > 0 {
            (div_ceil(plo, k), div_floor(phi, k))
        } else {
            (div_ceil(phi, k), div_floor(plo, k))
        };
        if wlo > whi {
            continue;
        }
        let Some(vf) = facts.get(&v) else { continue };
        let Some((mlo, mhi)) = operand_window(BinOp::FMul, prod_first, k) else {
            continue;
        };
        let lo = mlo.max(clamp_i64(wlo));
        let hi = mhi.min(clamp_i64(whi));
        let checks = window_checks(Operand::Value(v), vf, lo, hi);
        if checks.is_empty() {
            continue;
        }
        sites[src].checks = checks;
        absorbed.push(t);
    }
    for t in absorbed.into_iter().rev() {
        sites.remove(t);
    }
}

/// An operand's finite bounds: a constant's point, or a value's fact
/// when neither endpoint is a sentinel.
fn bounded(op: &Operand, facts: &HashMap<ValueId, NumFact>) -> Option<NumFact> {
    match op {
        Operand::Value(v) => facts
            .get(v)
            .copied()
            .filter(|f| f.lo > NEG_INF && f.hi < POS_INF),
        other => op_const_int(other).map(NumFact::point),
    }
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
        // 3n ≤ 2^53-1 → n ≤ 3002399751580330, rounded down to 2^51
        // for the shift form
        let mut facts = HashMap::default();
        facts.insert(ValueId(1), unbounded_nonneg());
        let kind = InstKind::BinOp(
            BinOp::FMul,
            Operand::ConstF64(3.0),
            Operand::Value(ValueId(1)),
        );
        let (checks, post) = growth_checks(&kind, ValueId(9), &facts).unwrap();
        assert!(!post, "constant multiply keeps the pre-op window");
        assert_eq!(checks.len(), 1);
        assert_eq!(checks[0].test, Test::HighBits(51));
    }

    #[test]
    fn a_two_sided_window_keeps_the_compare_form() {
        let mut facts = HashMap::default();
        facts.insert(ValueId(1), NumFact::new(NEG_INF, POS_INF));
        let kind = InstKind::BinOp(
            BinOp::FMul,
            Operand::Value(ValueId(1)),
            Operand::ConstF64(3.0),
        );
        let (checks, _) = growth_checks(&kind, ValueId(9), &facts).unwrap();
        assert_eq!(checks.len(), 2);
        assert_eq!(checks[0].test, Test::Above(3_002_399_751_580_330));
        assert_eq!(checks[1].test, Test::Below(-3_002_399_751_580_330));
    }

    #[test]
    fn add_checks_result_post_op() {
        // the accumulator add checks its result once after the i64 op
        // (no overflow under the in-set magnitude invariant), pruned
        // by the result fact: non-negative → upper check only, and
        // 2^53-1 is one below a power of two, so the shift form loses
        // nothing
        let mut facts = HashMap::default();
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
        assert_eq!(checks[0].operand, Operand::Value(ValueId(9)));
        assert_eq!(checks[0].test, Test::HighBits(53));
    }

    #[test]
    fn a_post_add_takes_only_a_constant_inside_2_pow_53() {
        // `t + c` with |c| past 2^53-1 can wrap the i64 add before the
        // check runs, and the side exit would write the wrapped sum back
        let mut facts = HashMap::default();
        facts.insert(ValueId(1), unbounded_nonneg());
        facts.insert(ValueId(9), unbounded_nonneg());
        let guarded = |k: &InstKind| growth_checks(k, ValueId(9), &facts).is_some();
        for (c, guardable) in [
            (9_007_199_254_740_991.0, true),
            (9_007_199_254_740_992.0, false),
            (9_223_372_036_854_774_784.0, false),
            (-9_223_372_036_854_774_784.0, false),
        ] {
            for op in [BinOp::FAdd, BinOp::FSub] {
                let kind = InstKind::BinOp(op, Operand::Value(ValueId(1)), Operand::ConstF64(c));
                let swapped = InstKind::BinOp(op, Operand::ConstF64(c), Operand::Value(ValueId(1)));
                assert_eq!(guarded(&kind), guardable);
                assert_eq!(guarded(&swapped), guardable);
            }
        }
    }

    /// `%2 = fmul %1, 10; %3 = fadd %2, %d` in one block, `%1`
    /// non-negative and unbounded, `%d` inside `[0, 9]`.
    fn digit_chain(d: Operand) -> (Function, HashMap<ValueId, NumFact>) {
        use torajs_core::ssa::{Block, BlockId, Inst, Terminator, Type, ValueInfo};
        let inst = |result: u32, kind: InstKind| Inst {
            result: Some(ValueId(result)),
            kind,
            origin: None,
        };
        let func = Function {
            name: "f".into(),
            params: vec![ValueId(1), ValueId(4)],
            ret: Type::Void,
            blocks: vec![Block {
                id: BlockId(0),
                insts: vec![
                    inst(
                        2,
                        InstKind::BinOp(
                            BinOp::FMul,
                            Operand::Value(ValueId(1)),
                            Operand::ConstF64(10.0),
                        ),
                    ),
                    inst(
                        3,
                        InstKind::BinOp(BinOp::FAdd, Operand::Value(ValueId(2)), d),
                    ),
                ],
                term: Terminator::Ret(None),
            }],
            values: (0..5)
                .map(|_| ValueInfo {
                    ty: Type::F64,
                    name: None,
                })
                .collect(),
            current_origin: None,
        };
        let mut facts = HashMap::default();
        facts.insert(ValueId(1), unbounded_nonneg());
        facts.insert(ValueId(2), unbounded_nonneg());
        facts.insert(ValueId(3), unbounded_nonneg());
        facts.insert(ValueId(4), NumFact::new(0, 9));
        (func, facts)
    }

    #[test]
    fn a_bounded_add_folds_onto_the_multiply_window() {
        let (func, facts) = digit_chain(Operand::Value(ValueId(4)));
        let set = HashSet::from_iter([ValueId(1), ValueId(2), ValueId(3), ValueId(4)]);
        let (growth, entries) = collect_guard_sites(&func, &set, &facts);
        assert!(entries.is_empty());
        assert_eq!(growth.len(), 1, "the add's post check is absorbed");
        assert_eq!(growth[0].result, ValueId(2));
        assert!(!growth[0].post);
        // v ≤ (2^53 - 1 - 9) / 10 = 900719925474098, rounded to 2^49
        assert_eq!(growth[0].checks.len(), 1);
        assert_eq!(growth[0].checks[0].operand, Operand::Value(ValueId(1)));
        assert_eq!(growth[0].checks[0].test, Test::HighBits(49));
    }

    /// A signed cell keeps the compare form, and the window is the
    /// exact `(M - d.hi) / 10` from above and `(-M - d.lo) / 10` from
    /// below: the sum must stay inside ±M for every `d`.
    #[test]
    fn an_absorbed_chain_window_is_exact_for_the_whole_d_range() {
        let (func, mut facts) = digit_chain(Operand::Value(ValueId(4)));
        facts.insert(ValueId(1), NumFact::new(NEG_INF, POS_INF));
        let set: HashSet<ValueId> = [ValueId(2), ValueId(3)].into_iter().collect();
        let (growth, _) = collect_guard_sites(&func, &set, &facts);
        assert_eq!(growth.len(), 1);
        assert_eq!(growth[0].checks[0].test, Test::Above(900_719_925_474_098));
        assert_eq!(growth[0].checks[1].test, Test::Below(-900_719_925_474_099));
    }

    #[test]
    fn an_unbounded_add_keeps_its_own_check() {
        let (func, mut facts) = digit_chain(Operand::Value(ValueId(4)));
        facts.insert(ValueId(4), NumFact::new(0, POS_INF));
        let set = HashSet::from_iter([ValueId(1), ValueId(2), ValueId(3), ValueId(4)]);
        let (growth, _) = collect_guard_sites(&func, &set, &facts);
        assert_eq!(growth.len(), 2);
    }

    #[test]
    fn add_result_two_sided_when_unbounded() {
        let mut facts = HashMap::default();
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
        let mut facts = HashMap::default();
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
        let mut facts = HashMap::default();
        facts.insert(ValueId(1), NumFact::new(0, POS_INF));
        let checks = entry_checks(&Operand::Value(ValueId(1)), &facts).unwrap();
        assert_eq!(checks.len(), 1);
        assert_eq!(checks[0].test, Test::HighBits(53));
    }
}
