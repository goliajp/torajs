//! Cold-arm prediction — the part of Ball & Larus (PLDI 1993,
//! "Branch Prediction for Free") this compiler can decide
//! mechanically. Feeds [`super::block_freq`].
//!
//! A conditional has a cold arm when the first matching rule says so:
//!
//! 1. **Throw probe** — `icmp ne (load (global_ref THROW_ACTIVE)), 0`:
//!    the throw arm is taken only when an exception is in flight.
//! 2. **Unreachable arm** — an arm ending in `unreachable`.
//! 3. **Header flag guard** — `icmp ne (and (load p), MASK), 0` where
//!    exactly one arm calls something: the latin1 / frozen / COW bit
//!    tests guarding a runtime slow path.
//! 4. **Length guard** — `icmp ult i, len` / `icmp uge i, len` with
//!    `len` the array length word (`load p +8`): the out-of-bounds arm
//!    (a grow on a store, `undefined` on a read). Only bounds checks
//!    compare unsigned; loop conditions use `slt`.
//!
//! Every other branch predicts nothing, and nothing splits on it: the
//! lowerer emits many conditionals no rule reads (loop versioning on a
//! combination of header bits, fused guards), and splitting those
//! evenly halves the estimate for whichever copy actually runs.
//!
//! Two classic heuristics are deliberately absent. The call heuristic
//! ("an arm that calls is rarely taken") misreads user code such as
//! `if (q.length > 16) q.shift()`, taken on almost every iteration;
//! it applies here only through rules 3–4, where the compare is one
//! the lowerer itself emitted. The opcode heuristic ("equality is
//! usually false") lost 14% on collatz's `n % 2 === 0` when block
//! layout tried it.
//!
//! A guard arm predicted cold but actually hot costs a store and a
//! reload around a call that itself costs far more.

use crate::ssa::id_hash::HashMap;

use crate::ssa::{
    BinOp, BlockId, Function, IPred, InstKind, Operand, THROW_ACTIVE_SYM, Terminator, ValueId,
};

/// Odds of an arm that runs only when an exception is in flight or
/// the program is about to trap.
pub const P_NEVER: f64 = 1.0 / 1024.0;
/// Odds of a runtime slow path whose trigger is data the program
/// controls (a non-latin1 string, a frozen array, a grow).
pub const P_SLOW_PATH: f64 = 1.0 / 64.0;

/// Byte offset of an array's length word in its heap header.
const ARR_LEN_OFFSET: u64 = 8;

/// Every predicted-cold edge: the branching block, the arm it rarely
/// takes, and the odds of taking it.
pub fn cold_arms(func: &Function) -> Vec<(BlockId, BlockId, f64)> {
    let defs = single_defs(func);
    let blocks: HashMap<BlockId, usize> = func
        .blocks
        .iter()
        .enumerate()
        .map(|(i, b)| (b.id, i))
        .collect();
    let calls = |b: BlockId| {
        func.blocks[blocks[&b]].insts.iter().any(|i| {
            matches!(
                i.kind,
                InstKind::Call(..) | InstKind::CallIndirect(..) | InstKind::RcDecAny(_)
            )
        })
    };
    let ends_unreachable =
        |b: BlockId| matches!(func.blocks[blocks[&b]].term, Terminator::Unreachable);

    let mut out = Vec::new();
    for b in &func.blocks {
        let Terminator::CondBr {
            cond,
            then_blk,
            else_blk,
        } = &b.term
        else {
            continue;
        };
        let (t, e) = (*then_blk, *else_blk);
        if t == e {
            continue;
        }
        let cond = match cond {
            Operand::Value(v) => defs.get(v).copied(),
            _ => None,
        };
        let calling_arm = || if calls(t) { t } else { e };
        let one_calls = calls(t) != calls(e);
        let arm = if cond.is_some_and(|c| is_throw_probe(c, &defs)) {
            Some((t, P_NEVER))
        } else if ends_unreachable(t) != ends_unreachable(e) {
            Some((if ends_unreachable(t) { t } else { e }, P_NEVER))
        } else if one_calls && cond.is_some_and(|c| is_flag_guard(c, &defs)) {
            Some((calling_arm(), P_SLOW_PATH))
        } else {
            cond.and_then(|c| out_of_bounds_arm(c, &defs, t, e))
                .map(|oob| (oob, P_SLOW_PATH))
        };
        if let Some((cold, p)) = arm {
            out.push((b.id, cold, p));
        }
    }
    out
}

/// Defining inst of every value written exactly once. φ destruction
/// gives a merged value one `copy` per predecessor; such values match
/// no rule.
fn single_defs(func: &Function) -> HashMap<ValueId, &InstKind> {
    let mut defs: HashMap<ValueId, Option<&InstKind>> = HashMap::default();
    for b in &func.blocks {
        for i in &b.insts {
            if let Some(r) = i.result {
                defs.entry(r)
                    .and_modify(|d| *d = None)
                    .or_insert(Some(&i.kind));
            }
        }
    }
    defs.into_iter()
        .filter_map(|(v, d)| d.map(|k| (v, k)))
        .collect()
}

fn value_def<'a>(op: &Operand, defs: &HashMap<ValueId, &'a InstKind>) -> Option<&'a InstKind> {
    match op {
        Operand::Value(v) => defs.get(v).copied(),
        _ => None,
    }
}

fn is_zero(op: &Operand) -> bool {
    matches!(op, Operand::ConstI64(0))
}

/// `icmp ne (load (global_ref THROW_ACTIVE)), 0`.
fn is_throw_probe(cond: &InstKind, defs: &HashMap<ValueId, &InstKind>) -> bool {
    let InstKind::ICmp(IPred::Ne, flag, zero) = cond else {
        return false;
    };
    if !is_zero(zero) {
        return false;
    }
    let Some(InstKind::Load(_, addr, 0)) = value_def(flag, defs) else {
        return false;
    };
    matches!(value_def(addr, defs), Some(InstKind::GlobalRef(s)) if s == THROW_ACTIVE_SYM)
}

/// `icmp ne (and (load p), MASK), 0` — a test of header bits.
fn is_flag_guard(cond: &InstKind, defs: &HashMap<ValueId, &InstKind>) -> bool {
    let InstKind::ICmp(IPred::Ne, masked, zero) = cond else {
        return false;
    };
    if !is_zero(zero) {
        return false;
    }
    let Some(InstKind::BinOp(BinOp::And, word, Operand::ConstI64(_))) = value_def(masked, defs)
    else {
        return false;
    };
    matches!(value_def(word, defs), Some(InstKind::Load(_, _, 0)))
}

/// The arm an unsigned compare against the array length word takes
/// when the index is out of bounds.
fn out_of_bounds_arm(
    cond: &InstKind,
    defs: &HashMap<ValueId, &InstKind>,
    then_blk: BlockId,
    else_blk: BlockId,
) -> Option<BlockId> {
    let InstKind::ICmp(pred @ (IPred::Ult | IPred::Uge), _, len) = cond else {
        return None;
    };
    let is_len =
        matches!(value_def(len, defs), Some(InstKind::Load(_, _, off)) if *off == ARR_LEN_OFFSET);
    is_len.then_some(if *pred == IPred::Ult {
        else_blk
    } else {
        then_blk
    })
}
