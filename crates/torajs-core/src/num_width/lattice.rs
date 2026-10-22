//! The expression-width lattice the constraint walk computes in:
//! what one expression contributes to the slots it reaches.

use super::{KeyMap, KeySet, SlotKeyId, SlotKeys};
use std::collections::VecDeque;

/// A slot dependency plus whether the value path from that slot to
/// the consuming write runs through a growth op (Mul, or Add/Sub with
/// a non-constant increment). A growth edge inside an assignment-graph
/// cycle means the slot's value can grow without bound across
/// iterations — integral, yet not i64-safe (W5): past 2^53 the f64
/// semantics round where i64 keeps exact / wraps, so the slot must
/// stay F64 and let guarded demotion (1b-ii) pull it back.
pub(super) type Dep = (SlotKeyId, bool);

/// Width of an expression as seen by the analysis.
#[derive(Clone)]
pub(super) enum W {
    /// Statically certain to be (or possibly be) a fractional /
    /// non-i64-exact f64 value.
    F64,
    /// Integral candidate whose final width depends on these slots.
    Num(Vec<Dep>),
    /// Not a number value (or a shape the analysis does not track —
    /// member/index reads keep their annotation-derived width, the
    /// container-width face is W4 scope).
    NotNum,
}

pub(super) fn join(a: W, b: W) -> W {
    match (a, b) {
        (W::F64, _) | (_, W::F64) => W::F64,
        (W::Num(mut d1), W::Num(d2)) => {
            d1.extend(d2);
            W::Num(d1)
        }
        (W::Num(d), W::NotNum) | (W::NotNum, W::Num(d)) => W::Num(d),
        (W::NotNum, W::NotNum) => W::NotNum,
    }
}

/// A number literal that cannot live in an i64 slot: genuinely
/// fractional, past i64 range (`n as i64` saturates), or `-0` (the
/// sign bit is meaningful f64 state that i64 zero erases — the miss
/// that aborted repro R5).
pub(super) fn literal_is_f64(n: f64) -> bool {
    n.fract() != 0.0 || n.abs() >= 9.223372036854776e18 || (n == 0.0 && n.is_sign_negative())
}

/// Poison flows forward along assignment edges until stable.
/// Monotone single-direction lattice — O(edges).
pub(super) fn fixpoint(seeds: Vec<SlotKeyId>, edges: &KeyMap<Vec<Dep>>) -> KeySet {
    let mut out = KeySet::default();
    let mut work: VecDeque<SlotKeyId> = seeds.into_iter().collect();
    while let Some(k) = work.pop_front() {
        if out.insert(k)
            && let Some(dsts) = edges.get(&k)
        {
            for (d, _) in dsts {
                if !out.contains(d) {
                    work.push_back(*d);
                }
            }
        }
    }
    out
}

/// `TORAJS_NUM_WIDTH_STATS=1` — dump the canonical F64 class set (one
/// line per poisoned representative), the same diagnostic shape as the
/// SSA-pass `*_STATS` switches.
pub(super) fn dump_f64_classes(keys: &SlotKeys, canon_out: &KeySet) {
    if std::env::var_os("TORAJS_NUM_WIDTH_STATS").is_some() {
        let mut lines: Vec<String> = canon_out.iter().map(|k| keys.debug(*k)).collect();
        lines.sort();
        for l in &lines {
            eprintln!("[num_width] f64 class: {l}");
        }
    }
}
