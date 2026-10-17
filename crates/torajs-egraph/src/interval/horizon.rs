//! The trip horizon of a small-step counter.
//!
//! A loop counter compared against an f64 bound (`i < n` with `n` from
//! `parseInt`) gets no upper bound from the compare: `n` is opaque, so
//! the ascent widens the counter to a sentinel. Everything written from
//! it inherits that — the elements of an array filled from the counter
//! keep a 2^53 window check on every read in a later loop.
//!
//! The number-width inference already decides such a counter is an
//! i64 on a trip-count argument: a cell moved only by a small literal
//! step leaves the exact integer window after `2^53 / |k|` steps, and
//! `|k| <= 2^5` keeps that at 2^48 steps or more — about twenty hours
//! of a tight `t += 1` loop, the horizon past which the representation
//! is taken to hold. A counter's value is its last init plus at most
//! 2^48 steps of at most `|k|` each, so this module states the bound
//! that argument already relies on, and claims nothing it does not:
//! the init's own fact is carried, so a counter starting near 2^53
//! still gets a bound past it and keeps its window checks.
//!
//! A cell qualifies when every def is either `cell + k` / `cell - k`
//! with a small constant `k` (directly, or through a copy of that
//! single-def sum) or a copy of some other operand (an init). A copy of
//! `cell + x` for any other `x` is a step of unknown size, so the cell
//! is no counter: read as an init, it would raise the cell's own bound
//! by a horizon every round of the ascent.

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{BinOp, Function, InstKind, Operand, Type, ValueId};

use super::transfer::{AbsVal, NumFact};

/// Steps of one counter a program run is taken never to exceed.
const HORIZON_STEPS: i128 = 1 << 48;
/// Largest step magnitude the horizon covers (`2^53 / 2^5 = 2^48`).
const MAX_STEP: i64 = 1 << 5;

/// The counters of one function, keyed by cell.
pub(super) struct Horizon {
    counters: HashMap<ValueId, Counter>,
}

impl Horizon {
    /// The counters of `func`, and the function's widening thresholds
    /// with the counters' bounds among them (see [`thresholds`]).
    pub(super) fn new(
        func: &Function,
        single: &HashMap<ValueId, InstKind>,
        cells: &HashSet<ValueId>,
    ) -> (Self, Vec<i128>) {
        let counters = collect(func, single, cells);
        let mut t = super::widen::collect_thresholds(func);
        t.extend(thresholds(&counters));
        t.sort_unstable();
        t.dedup();
        (Self { counters }, t)
    }

    /// `v`, the next fact of `cell`, capped when `cell` is a counter.
    pub(super) fn cap(&self, cell: ValueId, v: AbsVal, state: &super::State) -> AbsVal {
        match self.counters.get(&cell) {
            Some(c) => cap(c, v, state),
            None => v,
        }
    }
}

struct Counter {
    inits: Vec<Operand>,
    /// Largest upward step, 0 when the counter never steps up.
    up: i128,
    /// Largest downward step magnitude, 0 when it never steps down.
    down: i128,
}

/// The counters among `cells`: i64 cells whose defs are all inits or
/// small constant steps of the cell itself.
fn collect(
    func: &Function,
    single: &HashMap<ValueId, InstKind>,
    cells: &HashSet<ValueId>,
) -> HashMap<ValueId, Counter> {
    let mut out: HashMap<ValueId, Counter> = HashMap::default();
    let mut refused: HashSet<ValueId> = HashSet::default();
    for block in &func.blocks {
        for inst in &block.insts {
            let Some(c) = inst.result else { continue };
            if !cells.contains(&c) || refused.contains(&c) {
                continue;
            }
            if func.values[c.0 as usize].ty != Type::I64 {
                refused.insert(c);
                continue;
            }
            let step = match &inst.kind {
                InstKind::Copy(_, Operand::Value(t)) => single.get(t).and_then(|k| step_of(k, c)),
                other => step_of(other, c),
            };
            let entry = out.entry(c).or_insert(Counter {
                inits: Vec::new(),
                up: 0,
                down: 0,
            });
            match (step, &inst.kind) {
                (Some(k), _) if k > 0 => entry.up = entry.up.max(k as i128),
                (Some(k), _) => entry.down = entry.down.max(-(k as i128)),
                (None, InstKind::Copy(_, op)) if !steps_from(op, c, single) => {
                    entry.inits.push(*op)
                }
                (None, _) => {
                    refused.insert(c);
                }
            }
        }
    }
    out.retain(|c, k| !refused.contains(c) && !k.inits.is_empty() && (k.up > 0 || k.down > 0));
    out
}

/// `op` is a single-def `cell + x`, `x + cell` or `cell - x`.
fn steps_from(op: &Operand, cell: ValueId, single: &HashMap<ValueId, InstKind>) -> bool {
    let Operand::Value(t) = op else { return false };
    let c = Operand::Value(cell);
    match single.get(t) {
        Some(InstKind::BinOp(BinOp::Add, a, b)) => *a == c || *b == c,
        Some(InstKind::BinOp(BinOp::Sub, a, _)) => *a == c,
        _ => false,
    }
}

/// `cell ± k` for a small nonzero constant `k`, as the signed step.
fn step_of(kind: &InstKind, cell: ValueId) -> Option<i64> {
    let k = match kind {
        InstKind::BinOp(BinOp::Add, Operand::Value(v), Operand::ConstI64(k))
        | InstKind::BinOp(BinOp::Add, Operand::ConstI64(k), Operand::Value(v))
            if *v == cell =>
        {
            *k
        }
        InstKind::BinOp(BinOp::Sub, Operand::Value(v), Operand::ConstI64(k)) if *v == cell => {
            k.checked_neg()?
        }
        _ => return None,
    };
    (k != 0 && k.abs() <= MAX_STEP).then_some(k)
}

/// The horizon bounds of the counters whose inits are all constants,
/// as widening thresholds. A value fed by a counter (an element the
/// counter is stored into, the counter plus one) widens while the
/// counter itself is still climbing, and without these it jumps past
/// the counter's bound to the next compared constant — often 2^53 —
/// where the narrowing rounds may run out before they bring it back.
fn thresholds(counters: &HashMap<ValueId, Counter>) -> Vec<i128> {
    let mut out = Vec::new();
    for c in counters.values() {
        let inits: Option<Vec<i128>> = c
            .inits
            .iter()
            .map(|op| match op {
                Operand::ConstI64(k) => Some(*k as i128),
                _ => None,
            })
            .collect();
        let Some(inits) = inits else { continue };
        let (Some(lo), Some(hi)) = (inits.iter().min(), inits.iter().max()) else {
            continue;
        };
        let (lo, hi) = (lo - c.down * HORIZON_STEPS, hi + c.up * HORIZON_STEPS);
        if lo >= i64::MIN as i128 && hi <= i64::MAX as i128 {
            out.extend([lo, hi]);
        }
    }
    out
}

/// `v` narrowed to what the counter can reach from its inits within
/// the horizon. Inits are read flow-insensitively: the join of every
/// value they ever take.
fn cap(counter: &Counter, v: AbsVal, state: &super::State) -> AbsVal {
    let AbsVal::Fact(f) = v else { return v };
    let init = counter.inits.iter().fold(AbsVal::Bottom, |acc, op| {
        acc.join(super::lookup(op, state, &[]))
    });
    let AbsVal::Fact(i) = init else { return v };
    let lo = i.lo - counter.down * HORIZON_STEPS;
    let hi = i.hi + counter.up * HORIZON_STEPS;
    // a counter that can reach the end of i64 can wrap to anything
    if lo < i64::MIN as i128 || hi > i64::MAX as i128 {
        return v;
    }
    f.meet(NumFact::new(lo, hi)).map_or(v, AbsVal::Fact)
}

#[cfg(test)]
mod tests {
    use super::super::analyze_function;
    use super::*;
    use torajs_core::ssa::{Block, BlockId, IPred, Inst, Terminator, ValueInfo};

    fn inst(result: u32, kind: InstKind) -> Inst {
        Inst {
            result: Some(ValueId(result)),
            kind,
            origin: None,
        }
    }

    fn v(id: u32) -> Operand {
        Operand::Value(ValueId(id))
    }

    /// `%0 = init; while (%0 <pred> %9) { %0 = <step> }` against an
    /// opaque i64 parameter `%9`, with `extra` defs of `%0` in the body.
    fn counter_fn(init: i64, pred: IPred, step: InstKind, extra: Vec<Inst>) -> Function {
        let mut body = vec![inst(5, step), inst(0, InstKind::Copy(Type::I64, v(5)))];
        body.extend(extra);
        let block = |id: u32, insts: Vec<Inst>, term: Terminator| Block {
            id: BlockId(id),
            insts,
            term,
        };
        let entry = vec![inst(0, InstKind::Copy(Type::I64, Operand::ConstI64(init)))];
        let test = Terminator::CondBr {
            cond: v(1),
            then_blk: BlockId(2),
            else_blk: BlockId(3),
        };
        Function {
            name: "f".into(),
            params: vec![ValueId(9)],
            ret: Type::Void,
            blocks: vec![
                block(0, entry, Terminator::Br(BlockId(1))),
                block(1, vec![inst(1, InstKind::ICmp(pred, v(0), v(9)))], test),
                block(2, body, Terminator::Br(BlockId(1))),
                block(3, vec![], Terminator::Ret(None)),
            ],
            values: (0..10)
                .map(|i| ValueInfo {
                    ty: if i == 1 { Type::Bool } else { Type::I64 },
                    name: None,
                })
                .collect(),
            current_origin: None,
        }
    }

    fn add(k: i64) -> InstKind {
        InstKind::BinOp(BinOp::Add, v(0), Operand::ConstI64(k))
    }

    #[test]
    fn an_upward_counter_stops_at_the_horizon() {
        let f = counter_fn(0, IPred::Slt, add(1), vec![]);
        let facts = analyze_function(&f);
        assert_eq!(facts[&ValueId(0)], NumFact::new(0, HORIZON_STEPS));
    }

    #[test]
    fn a_downward_counter_stops_at_the_horizon() {
        let step = InstKind::BinOp(BinOp::Sub, v(0), Operand::ConstI64(2));
        let f = counter_fn(7, IPred::Sgt, step, vec![]);
        let facts = analyze_function(&f);
        assert_eq!(facts[&ValueId(0)], NumFact::new(7 - 2 * HORIZON_STEPS, 7));
    }

    #[test]
    fn an_init_near_2p53_keeps_a_bound_past_it() {
        let init = (1i64 << 53) - 3;
        let f = counter_fn(init, IPred::Slt, add(1), vec![]);
        let facts = analyze_function(&f);
        assert_eq!(facts[&ValueId(0)].hi, init as i128 + HORIZON_STEPS);
        assert!(facts[&ValueId(0)].hi > 1i128 << 53);
    }

    #[test]
    fn a_large_step_is_not_a_counter() {
        let f = counter_fn(0, IPred::Slt, add(MAX_STEP + 1), vec![]);
        let facts = analyze_function(&f);
        assert!(facts[&ValueId(0)].hi > (MAX_STEP as i128 + 1) * HORIZON_STEPS);
    }

    #[test]
    fn a_def_that_is_not_a_step_or_a_copy_is_not_a_counter() {
        let mul = inst(0, InstKind::BinOp(BinOp::Mul, v(0), Operand::ConstI64(2)));
        let f = counter_fn(0, IPred::Slt, add(1), vec![mul]);
        let facts = analyze_function(&f);
        assert!(facts[&ValueId(0)].hi > HORIZON_STEPS);
    }

    #[test]
    fn a_copy_of_the_cell_plus_an_unknown_is_not_a_counter() {
        // %0 = copy (%0 + %9): the step is the parameter's size
        let jump = vec![
            inst(6, InstKind::BinOp(BinOp::Add, v(0), v(9))),
            inst(0, InstKind::Copy(Type::I64, v(6))),
        ];
        let f = counter_fn(0, IPred::Slt, add(1), jump);
        let facts = analyze_function(&f);
        assert!(facts[&ValueId(0)].hi > HORIZON_STEPS);
    }

    #[test]
    fn a_counter_that_can_wrap_is_left_alone() {
        let f = counter_fn(i64::MAX - 1, IPred::Slt, add(1), vec![]);
        let facts = analyze_function(&f);
        assert!(facts[&ValueId(0)].hi >= i64::MAX as i128 - 1);
        assert_ne!(facts[&ValueId(0)].hi, i64::MAX as i128 - 1 + HORIZON_STEPS);
    }
}
