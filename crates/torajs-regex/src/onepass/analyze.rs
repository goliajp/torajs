//! One-pass determination and matcher construction.
//!
//! States are seeded by one program counter each: the start state by
//! pc 0, every other state by the successor of the consuming
//! instruction whose edge enters it. The start state is never an
//! edge target: an edge back to pc 0 (a leading `*` whose body ends
//! in a jump to it) enters a second state seeded by pc 0, so the
//! walk can begin an attempt at the start state and only there. A state's body is the ε-closure
//! of its seed walked in the Pike VM's priority order (a split's
//! first target before its second, first arrival at a pc wins), with
//! the save slots met on the way carried along. The walk stops at
//! `Op::Match`: threads behind it are lower priority than a match
//! the VM has already committed to, so they never consume.
//!
//! An assertion on the way passes or fails with the position's
//! context, so the closure is walked once per assignment of the
//! context bits the seed can reach, and the state carries a variant
//! for each (`ir::State::probes`).
//!
//! The program is one-pass when no unit is claimed by two consuming
//! instructions of the same variant unless both lead to the same
//! `(target, saves)` pair.
//!
//! Units are claimed segment by segment ([`super::units`]): every
//! instruction the lane serves gives one answer across a segment.

use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;

use super::closure::{Closure, Writes, closure, coalesce, past_jumps, probes, unpack};
use super::ir::{MatcherIr, State, UNIT_MAX_NARROW, Variant};
use super::units::{Admits, segments};
use crate::flags::unicode_mode;
use crate::parser::{RE_FLAG_I, RE_FLAG_Y};
use crate::program::{Inst, Op, Program};

/// Build the matcher for `prog` (compiled with `Program::raw_classes`
/// under `flags`) over Latin-1 code units, or over UTF-16 code units
/// when `wide`. `None` when the program is not one-pass, or uses a
/// shape this walk does not serve: lookaround, backreferences, and
/// under `i` an atom that reaches past U+00FF.
pub fn build(prog: &Program, flags: u8, wide: bool) -> Option<MatcherIr> {
    if !servable(prog) {
        return None;
    }
    let unicode = unicode_mode(flags);
    let pairs = wide && unicode;
    let segments = segments(prog, wide, pairs);
    let mut admits = Admits::new(prog, &segments, unicode);
    let mut states: Vec<State> = Vec::new();
    // edge targets by seed; the start state is not among them, so a
    // loop back to pc 0 enters a state of its own
    let mut by_seed: BTreeMap<usize, u32> = BTreeMap::new();
    let mut queue: Vec<(usize, usize)> = vec![(0, 0)];
    while let Some((seed, id)) = queue.pop() {
        if states.len() <= id {
            states.resize_with(id + 1, || State {
                probes: 0,
                variants: Vec::new(),
            });
        }
        let probes = probes(prog, seed, unicode);
        let mut variants = Vec::with_capacity(1 << probes.count_ones());
        for index in 0..1usize << probes.count_ones() {
            let ctx = unpack(probes, index);
            let Closure { consuming, accept } = closure(prog, &[seed], ctx, unicode);
            let mut table: Vec<Option<(u32, Writes)>> = vec![None; segments.len()];
            for (pc, writes) in consuming {
                let next = past_jumps(prog, pc + 1);
                let target = match by_seed.get(&next) {
                    Some(&t) => t,
                    None => {
                        let t = by_seed.len() as u32 + 1;
                        by_seed.insert(next, t);
                        queue.push((next, t as usize));
                        t
                    }
                };
                for &k in admits.of(pc) {
                    let want = (target, writes.clone());
                    match &table[k] {
                        None => table[k] = Some(want),
                        Some(have) if *have == want => {}
                        Some(_) => return None,
                    }
                }
            }
            let (accept, accept_clears) = match accept {
                Some(w) => (Some(w.saves), w.clears),
                None => (None, Vec::new()),
            };
            variants.push(Variant {
                edges: coalesce(&segments, &table),
                accept,
                accept_clears,
            });
        }
        states[id] = State { probes, variants };
    }
    Some(MatcherIr {
        states,
        stride: crate::vm::saves_arena::detect_stride(prog) as u32,
        wide,
        pairs,
        anchored: flags & RE_FLAG_Y != 0,
    })
}

pub(super) fn servable(prog: &Program) -> bool {
    if !prog.sub_progs.is_empty() {
        return false;
    }
    let n = prog.len();
    let in_range = |t: i32| t >= 0 && (t as usize) < n;
    prog.insts.iter().enumerate().all(|(i, ins)| {
        let falls_through = i + 1 < n;
        match Op::from_u8(ins.op) {
            Some(Op::Char) => falls_through && (!folds(ins) || ins.cp() <= UNIT_MAX_NARROW),
            Some(Op::AnyChar) => falls_through,
            Some(Op::Class) => {
                falls_through
                    && prog
                        .classes
                        .get(ins.a as usize)
                        .is_some_and(|c| !c.byte_only && (!folds(ins) || latin1_class(c)))
            }
            Some(Op::Save) => falls_through && ins.a >= 0,
            Some(Op::SaveReset) => falls_through && ins.a >= 0 && ins.a <= ins.b,
            Some(Op::IterStart) => falls_through,
            Some(Op::IterCheck) => falls_through && in_range(ins.a),
            Some(Op::AnchorB | Op::AnchorE | Op::WBound | Op::NWBound) => falls_through,
            Some(Op::Jmp) => in_range(ins.a),
            Some(Op::Split) => in_range(ins.a) && in_range(ins.b),
            Some(Op::Match) => true,
            _ => false,
        }
    })
}

fn folds(ins: &Inst) -> bool {
    ins.pad as u8 & RE_FLAG_I != 0
}

/// A class whose members all lie in U+0000..U+00FF — the domain
/// [`super::fold::canonicalize`] covers, so the class can be matched
/// under `i`. A property table or a range past U+00FF would need the
/// full case-folding tables.
fn latin1_class(c: &crate::charclass::CharClass) -> bool {
    c.u_prop_tables.is_empty() && c.owned_ranges.is_empty()
}

#[cfg(test)]
mod tests;
