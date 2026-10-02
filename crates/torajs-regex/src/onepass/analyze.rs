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

use super::ir::{
    Edge, INPUT_END, INPUT_START, LINE_END, LINE_START, MatcherIr, State, UNIT_MAX_NARROW, Variant,
    WORD_LEFT, WORD_LEFT_FOLD, WORD_RIGHT, WORD_RIGHT_FOLD,
};
use super::units::{admits, segments};
use crate::flags::unicode_mode;
use crate::parser::{RE_FLAG_I, RE_FLAG_M, RE_FLAG_Y};
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
            let Closure { consuming, accept } = closure(prog, seed, ctx, unicode);
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
                let ins = prog.insts[pc];
                for (k, &(lo, _)) in segments.iter().enumerate() {
                    if !admits(prog, ins, lo, unicode) {
                        continue;
                    }
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

fn servable(prog: &Program) -> bool {
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

/// Where a walk entered at `pc` starts consuming: a jump writes no slot
/// and reads no context, so the arms of an alternation share the state
/// their exits jump to instead of seeding one state per arm.
fn past_jumps(prog: &Program, mut pc: usize) -> usize {
    for _ in 0..prog.len() {
        if Op::from_u8(prog.insts[pc].op) != Some(Op::Jmp) {
            break;
        }
        pc = prog.insts[pc].a as usize;
    }
    pc
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

/// The context bits the assertions reachable from `seed` by ε read.
fn probes(prog: &Program, seed: usize, unicode: bool) -> u8 {
    let mut visited = vec![false; prog.len()];
    let mut stack = vec![seed];
    let mut probes = 0;
    while let Some(pc) = stack.pop() {
        if visited[pc] {
            continue;
        }
        visited[pc] = true;
        let ins = prog.insts[pc];
        let multiline = ins.pad as u8 & RE_FLAG_M != 0;
        let fold = unicode && ins.pad as u8 & RE_FLAG_I != 0;
        match Op::from_u8(ins.op) {
            Some(Op::Jmp) => stack.push(ins.a as usize),
            Some(Op::Split) => {
                stack.push(ins.a as usize);
                stack.push(ins.b as usize);
            }
            Some(Op::Save | Op::SaveReset | Op::IterStart | Op::IterCheck) => stack.push(pc + 1),
            Some(Op::AnchorB) => {
                probes |= if multiline { LINE_START } else { INPUT_START };
                stack.push(pc + 1);
            }
            Some(Op::AnchorE) => {
                probes |= if multiline { LINE_END } else { INPUT_END };
                stack.push(pc + 1);
            }
            Some(Op::WBound | Op::NWBound) => {
                probes |= if fold {
                    WORD_LEFT_FOLD | WORD_RIGHT_FOLD
                } else {
                    WORD_LEFT | WORD_RIGHT
                };
                stack.push(pc + 1);
            }
            _ => {}
        }
    }
    probes
}

/// The context whose variant index is `index` (the inverse of
/// [`super::ir::pack`]).
fn unpack(probes: u8, index: usize) -> u8 {
    let mut ctx = 0;
    let mut k = 0;
    for bit in 0..8 {
        if probes & (1 << bit) != 0 {
            if index & (1 << k) != 0 {
                ctx |= 1 << bit;
            }
            k += 1;
        }
    }
    ctx
}

/// Whether the assertion `ins` holds under `ctx`.
fn holds(ins: Inst, ctx: u8, unicode: bool) -> bool {
    let multiline = ins.pad as u8 & RE_FLAG_M != 0;
    let fold = unicode && ins.pad as u8 & RE_FLAG_I != 0;
    let (left, right) = if fold {
        (WORD_LEFT_FOLD, WORD_RIGHT_FOLD)
    } else {
        (WORD_LEFT, WORD_RIGHT)
    };
    match Op::from_u8(ins.op) {
        Some(Op::AnchorB) => ctx & if multiline { LINE_START } else { INPUT_START } != 0,
        Some(Op::AnchorE) => ctx & if multiline { LINE_END } else { INPUT_END } != 0,
        Some(Op::WBound) => (ctx & left != 0) != (ctx & right != 0),
        Some(Op::NWBound) => (ctx & left != 0) == (ctx & right != 0),
        _ => false,
    }
}

/// The slot writes of an ε path: `saves` take the position, `clears`
/// unset; the last write to a slot decides which list holds it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Writes {
    saves: Vec<u32>,
    clears: Vec<u32>,
}

impl Writes {
    fn save(&mut self, slot: u32) {
        move_slot(&mut self.clears, &mut self.saves, slot);
    }

    fn clear(&mut self, slot: u32) {
        move_slot(&mut self.saves, &mut self.clears, slot);
    }
}

fn move_slot(from: &mut Vec<u32>, to: &mut Vec<u32>, slot: u32) {
    if let Ok(at) = from.binary_search(&slot) {
        from.remove(at);
    }
    if let Err(at) = to.binary_search(&slot) {
        to.insert(at, slot);
    }
}

struct Closure {
    /// consuming pcs in priority order, each with the slot writes on
    /// its ε path from the seed
    consuming: Vec<(usize, Writes)>,
    accept: Option<Writes>,
}

fn closure(prog: &Program, seed: usize, ctx: u8, unicode: bool) -> Closure {
    let mut visited = vec![false; prog.len()];
    // each path carries its slot writes and the `IterStart` pcs on it
    let mut stack: Vec<(usize, Writes, Vec<usize>)> = vec![(seed, Writes::default(), Vec::new())];
    let mut consuming = Vec::new();
    let mut accept = None;
    while let Some((pc, saves, iters)) = stack.pop() {
        if visited[pc] {
            continue;
        }
        visited[pc] = true;
        let ins = prog.insts[pc];
        match Op::from_u8(ins.op) {
            Some(Op::Jmp) => stack.push((ins.a as usize, saves, iters)),
            Some(Op::Split) => {
                stack.push((ins.b as usize, saves.clone(), iters.clone()));
                stack.push((ins.a as usize, saves, iters));
            }
            Some(Op::Save) => {
                let mut saves = saves;
                saves.save(ins.a as u32);
                stack.push((pc + 1, saves, iters));
            }
            Some(Op::SaveReset) => {
                let mut saves = saves;
                for slot in ins.a..ins.b {
                    saves.clear(slot as u32);
                }
                stack.push((pc + 1, saves, iters));
            }
            Some(Op::IterStart) => {
                let mut iters = iters;
                iters.push(pc);
                stack.push((pc + 1, saves, iters));
            }
            Some(Op::IterCheck) => {
                if !iters.contains(&(ins.a as usize)) {
                    stack.push((pc + 1, saves, iters));
                }
            }
            Some(Op::AnchorB | Op::AnchorE | Op::WBound | Op::NWBound) => {
                if holds(ins, ctx, unicode) {
                    stack.push((pc + 1, saves, iters));
                }
            }
            Some(Op::Match) => {
                accept = Some(saves);
                break;
            }
            _ => consuming.push((pc, saves)),
        }
    }
    Closure { consuming, accept }
}

fn coalesce(segments: &[(u32, u32)], table: &[Option<(u32, Writes)>]) -> Vec<Edge> {
    let mut edges: Vec<Edge> = Vec::new();
    for (&(lo, hi), cell) in segments.iter().zip(table) {
        let Some((target, writes)) = cell else {
            continue;
        };
        if let Some(last) = edges.last_mut()
            && last.hi + 1 == lo
            && last.target == *target
            && last.saves == writes.saves
            && last.clears == writes.clears
        {
            last.hi = hi;
            continue;
        }
        edges.push(Edge {
            lo,
            hi,
            saves: writes.saves.clone(),
            clears: writes.clears.clone(),
            target: *target,
        });
    }
    edges
}

#[cfg(test)]
mod tests;
