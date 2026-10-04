//! The widened path in front of a matched sum loop.
//!
//! ```text
//! entry --> E: no fixed exit, i >= 0, K <= len(arr), S passes the
//!              sum guards ----------------------------------------no--> H
//! B:  K - i < 8 ------------------------------------------------yes-> H
//!     n = min(K - i, BLOCK) & -8; lane accumulators and or-folds = 0
//! L:  per 8 elements: 4 vector loads, the body's lane arithmetic,
//!     or every checked lane value into its fold, add the addend
//! C:  S' = S + sum of lanes; any checked lane outside [0, 2^53), or S'
//!     failing a sum guard -----------------------------------------> H
//!     S = S', i = i + n, back to B
//! ```
//!
//! H is the scalar loop's header, entered with the counter and the sum
//! as they were before the block: the scalar loop redoes those elements
//! with its own guards and leaves for the f64 loop at the exact element
//! that needs it. Inside a block nothing writes the counter or the sum.
//! With every addend in `[0, 2^53)` the partial sums only grow, so S'
//! passing the guards that S passed means every partial sum does; and
//! `|S| <= 2^53` plus `BLOCK * 2^53` stays far from i64 overflow.

use std::collections::HashMap;

use torajs_core::ssa::{
    BinOp, BlockId, Function, IPred, InstKind, Operand, Terminator, Type, ValueId, VecInst, VecOp,
    VecShape, rewrite_value_operands,
};

use super::builder::{Emit, VEC};
use super::guard::{EXACT_BITS, Test};
use super::shape::{Plan, Step};
use crate::load_licm::ARR_LEN_OFF;

/// Elements per block: `BLOCK * 2^53 < 2^62`.
const BLOCK: i64 = 256;
/// Vector registers each step of the inner loop works on.
const UNROLL: usize = 4;
const LANES: i64 = 2;
const STEP: i64 = UNROLL as i64 * LANES;

/// What one block's setup hands the step and the commit.
struct BlockState {
    end: Operand,
    stop: Operand,
    cursor: ValueId,
    splats: HashMap<Operand, Operand>,
    sums: Vec<ValueId>,
    folds: Vec<ValueId>,
}

/// Install the widened path for `p`; the scalar loop is left as is.
pub(super) fn widen(func: &mut Function, p: &Plan) {
    let mut e = Emit {
        func,
        block: BlockId(0),
    };
    let entry = e.new_block();
    for &b in &p.entries {
        retarget(&mut e.func.blocks[b.0 as usize].term, p.header, entry);
    }
    e.at(entry);
    let fixed = entry_checks(&mut e, p);
    let head = e.block;
    let state = setup(&mut e, p, &fixed);
    let body = e.new_block();
    e.term(Terminator::Br(body));
    e.at(body);
    let more = step(&mut e, p, &state);
    let check = e.new_block();
    e.cond_br(more, body, check);
    e.at(check);
    commit(&mut e, p, &state, head);
}

fn retarget(term: &mut Terminator, from: BlockId, to: BlockId) {
    match term {
        Terminator::Br(t) if *t == from => *t = to,
        Terminator::CondBr {
            then_blk, else_blk, ..
        } => {
            for t in [then_blk, else_blk] {
                if *t == from {
                    *t = to;
                }
            }
        }
        _ => {}
    }
}

/// E: the conditions under which the scalar loop runs instead. Leaves
/// the builder at B, a fresh block, and returns the entry copies of the
/// body's invariant values.
fn entry_checks(e: &mut Emit<'_>, p: &Plan) -> HashMap<ValueId, ValueId> {
    let h = p.header;
    // an exit the body takes on a condition no iteration changes is
    // the scalar loop's to take
    let mut fixed: HashMap<ValueId, ValueId> = HashMap::new();
    for inst in &p.invariant {
        let mut kind = inst.kind.clone();
        rewrite_value_operands(&mut kind, |v| fixed.get(&v).copied().unwrap_or(v));
        let old = inst.result.expect("invariant body value");
        let r = e.value(e.func.value_type(old));
        e.def(r, kind);
        fixed.insert(old, r);
    }
    for &(cond, leaves_on_true) in &p.entry_exits {
        let next = e.new_block();
        let cond = Operand::Value(fixed[&cond]);
        if leaves_on_true {
            e.cond_br(cond, h, next);
        } else {
            e.cond_br(cond, next, h);
        }
        e.at(next);
    }
    // the widened path reads only `[i, K)`, inside the array; with
    // `0 <= i <= K <= len` the block sizing `K - i` cannot wrap
    let i = Operand::Value(p.counter);
    let mut bad = vec![e.icmp(IPred::Slt, i, Operand::ConstI64(0))];
    bad.push(e.icmp(IPred::Slt, p.bound, i));
    let len = e.push(InstKind::Load(Type::I64, p.arr, ARR_LEN_OFF), Type::I64);
    bad.push(e.icmp(IPred::Sgt, p.bound, len));
    for &t in &p.sum_tests {
        bad.push(e.violates(Operand::Value(p.acc), t));
    }
    let bad = e.any(bad);
    let head = e.new_block();
    e.cond_br(bad, h, head);
    e.at(head);
    fixed
}

/// B: size the block, `min(K - i, BLOCK)` rounded down to a step, or
/// leave for the scalar loop when not one step is left.
fn setup(e: &mut Emit<'_>, p: &Plan, fixed: &HashMap<ValueId, ValueId>) -> BlockState {
    let i = Operand::Value(p.counter);
    let rem = e.bin(BinOp::Sub, p.bound, i);
    let few = e.icmp(IPred::Slt, rem, Operand::ConstI64(STEP));
    let sized = e.new_block();
    e.cond_br(few, p.header, sized);
    e.at(sized);
    let big = e.icmp(IPred::Sgt, rem, Operand::ConstI64(BLOCK));
    let n = e.push(
        InstKind::Select(Type::I64, big, Operand::ConstI64(BLOCK), rem),
        Type::I64,
    );
    let n = e.bin(BinOp::And, n, Operand::ConstI64(-STEP));
    let end = e.bin(BinOp::Add, i, n);
    let off = e.bin(BinOp::Shl, i, Operand::ConstI64(3));
    let start = e.bin(BinOp::Add, p.base, off);
    let span = e.bin(BinOp::Shl, n, Operand::ConstI64(3));
    let stop = e.bin(BinOp::Add, start, span);
    let mut splats: HashMap<Operand, Operand> = HashMap::new();
    for st in &p.steps {
        let Step::Add(_, a, b) = st else { continue };
        for op in [a, b] {
            if !is_lane(p, op) && !splats.contains_key(op) {
                let scalar = match op {
                    Operand::Value(v) => Operand::Value(*fixed.get(v).unwrap_or(v)),
                    other => *other,
                };
                let v = e.vec(VecInst::Splat(VecShape::I64x2, scalar));
                splats.insert(*op, v);
            }
        }
    }
    let sums: Vec<ValueId> = (0..UNROLL).map(|_| e.value(VEC)).collect();
    let fold_count = if p.checked.is_empty() { 0 } else { UNROLL };
    let folds: Vec<ValueId> = (0..fold_count).map(|_| e.value(VEC)).collect();
    for &cell in sums.iter().chain(&folds) {
        let zero = e.vec(VecInst::Splat(VecShape::I64x2, Operand::ConstI64(0)));
        e.copy(cell, VEC, zero);
    }
    let cursor = e.value(Type::I64);
    e.copy(cursor, Type::I64, start);
    BlockState {
        end,
        stop,
        cursor,
        splats,
        sums,
        folds,
    }
}

/// L: one step of `UNROLL` vectors, the body's lane arithmetic on each.
/// Returns the condition to run another step.
fn step(e: &mut Emit<'_>, p: &Plan, b: &BlockState) -> Operand {
    let at = Operand::Value(b.cursor);
    let mut sum_next: Vec<Operand> = Vec::new();
    let mut fold_next: Vec<Operand> = Vec::new();
    for k in 0..UNROLL {
        let mut lanes: HashMap<ValueId, Operand> = HashMap::new();
        for st in &p.steps {
            let (r, v) = match st {
                Step::Elem(r) => (*r, e.push(InstKind::Load(VEC, at, 16 * k as u64), VEC)),
                Step::Alias(r, src) => (*r, lanes[src]),
                Step::Add(r, x, y) => {
                    let lane = |op: &Operand| match op {
                        Operand::Value(v) if lanes.contains_key(v) => lanes[v],
                        _ => b.splats[op],
                    };
                    let (x, y) = (lane(x), lane(y));
                    (*r, e.vbin(VecOp::Add, x, y))
                }
            };
            lanes.insert(r, v);
        }
        if let Some(&cell) = b.folds.get(k) {
            let mut fold = Operand::Value(cell);
            let mut seen: Vec<Operand> = Vec::new();
            for c in &p.checked {
                let v = lanes[c];
                if !seen.contains(&v) {
                    seen.push(v);
                    fold = e.vbin(VecOp::Or, fold, v);
                }
            }
            fold_next.push(fold);
        }
        sum_next.push(e.vbin(VecOp::Add, Operand::Value(b.sums[k]), lanes[&p.addend]));
    }
    let sums = b.sums.iter().zip(&sum_next);
    for (cell, next) in sums.chain(b.folds.iter().zip(&fold_next)) {
        e.copy(*cell, VEC, *next);
    }
    let next = e.bin(BinOp::Add, at, Operand::ConstI64(16 * UNROLL as i64));
    e.copy(b.cursor, Type::I64, next);
    e.icmp(IPred::Ult, at, b.stop)
}

/// C: commit the block, or hand it whole to the scalar loop.
fn commit(e: &mut Emit<'_>, p: &Plan, b: &BlockState, head: BlockId) {
    let mut bad: Vec<Operand> = Vec::new();
    if !b.folds.is_empty() {
        let lanes_or = e.reduce(VecOp::Or, &cells(&b.folds));
        bad.push(e.violates(lanes_or, Test::HighBits(EXACT_BITS)));
    }
    let lanes_sum = e.reduce(VecOp::Add, &cells(&b.sums));
    let total = e.bin(BinOp::Add, Operand::Value(p.acc), lanes_sum);
    for &t in &p.sum_tests {
        bad.push(e.violates(total, t));
    }
    if !bad.is_empty() {
        let bad = e.any(bad);
        let ok = e.new_block();
        e.cond_br(bad, p.header, ok);
        e.at(ok);
    }
    e.copy(p.acc, Type::I64, total);
    e.copy(p.counter, Type::I64, b.end);
    e.term(Terminator::Br(head));
}

fn cells(vs: &[ValueId]) -> Vec<Operand> {
    vs.iter().map(|v| Operand::Value(*v)).collect()
}

fn is_lane(p: &Plan, op: &Operand) -> bool {
    let Operand::Value(v) = op else { return false };
    p.steps.iter().any(|s| match s {
        Step::Elem(r) | Step::Alias(r, _) | Step::Add(r, _, _) => r == v,
    })
}
