//! The window a value must land in for the fast copy to be worth
//! entering: what the loop's readers of it ask, through the copies
//! (its cells included) that carry it. A narrower window sends more
//! inputs to the slow copy; it never changes an answer.
//!
//! A divisor must not be 0, and what a `%` divides must not be
//! negative, for the integer `%` to answer what the float one does.
//! A value the loop multiplies or adds to must leave the result
//! inside 2^53, or the float arithmetic has already rounded where
//! the integer one has not.

use std::collections::HashSet;

use torajs_core::ssa::{BinOp, BlockId, Function, InstKind, Operand, ValueId};

use super::{EXACT_VALUE_MAX, INT32_MAX, INT32_MIN};
use crate::interval::transfer::op_const_int;

/// a product of two carried values stays exact below this
const SQUARE_MAX: i64 = 1 << 26;

pub(super) fn value_window(
    func: &Function,
    region: &HashSet<BlockId>,
    starts: &[ValueId],
    int32: bool,
) -> (i64, i64) {
    let (lo, hi) = if int32 {
        (INT32_MIN, INT32_MAX)
    } else {
        (-EXACT_VALUE_MAX, EXACT_VALUE_MAX)
    };
    let insts = || {
        region
            .iter()
            .flat_map(|b| func.blocks[b.0 as usize].insts.iter())
    };
    // what carries the value as it is, and what is computed from it
    let mut reach: HashSet<ValueId> = starts.iter().copied().collect();
    let mut derived = reach.clone();
    loop {
        let mut grew = false;
        for inst in insts() {
            let Some(r) = inst.result else { continue };
            let within = |set: &HashSet<ValueId>, op: &Operand| matches!(op, Operand::Value(v) if set.contains(v));
            match &inst.kind {
                InstKind::Copy(_, from) => {
                    if within(&reach, from) {
                        grew |= reach.insert(r);
                    }
                    if within(&derived, from) {
                        grew |= derived.insert(r);
                    }
                }
                InstKind::BinOp(BinOp::FMul | BinOp::FAdd | BinOp::FSub | BinOp::FRem, a, b)
                | InstKind::Select(_, _, a, b)
                    if within(&derived, a) || within(&derived, b) =>
                {
                    grew |= derived.insert(r);
                }
                _ => {}
            }
        }
        if !grew {
            break;
        }
    }
    let carried = |op: &Operand| matches!(op, Operand::Value(v) if reach.contains(v));
    let computed = |op: &Operand| matches!(op, Operand::Value(v) if derived.contains(v));
    let mut magnitude = i64::MAX;
    let mut floor = i64::MIN;
    let mut within = |bound: i64| magnitude = magnitude.min(bound);
    for inst in insts() {
        let InstKind::BinOp(op, a, b) = &inst.kind else {
            continue;
        };
        match op {
            BinOp::FRem => {
                if computed(a) {
                    floor = floor.max(0);
                }
                if carried(b) {
                    floor = floor.max(1);
                }
            }
            BinOp::FMul if carried(a) && carried(b) => within(SQUARE_MAX),
            BinOp::FMul | BinOp::FAdd | BinOp::FSub => {
                let by = match (carried(a), carried(b)) {
                    (true, false) => op_const_int(b),
                    (false, true) => op_const_int(a),
                    _ => None,
                };
                let Some(c) = by.map(|c| c.unsigned_abs()) else {
                    continue;
                };
                let max = EXACT_VALUE_MAX as u128;
                let bound = match op {
                    BinOp::FMul if c > 1 => max / c,
                    BinOp::FMul => max,
                    _ => max.saturating_sub(c),
                };
                within(bound.min(max) as i64);
            }
            _ => {}
        }
    }
    (lo.max(-magnitude).max(floor), hi.min(magnitude))
}
