//! One-pass determination and matcher construction.
//!
//! States are seeded by one program counter each: the start state by
//! pc 0, every other state by the successor of the consuming
//! instruction whose edge enters it. A state's body is the ε-closure
//! of its seed walked in the Pike VM's priority order (a split's
//! first target before its second, first arrival at a pc wins), with
//! the save slots met on the way carried along. The walk stops at
//! `Op::Match`: threads behind it are lower priority than a match
//! the VM has already committed to, so they never consume.
//!
//! The program is one-pass when no unit is claimed by two consuming
//! instructions of the same state unless both lead to the same
//! `(target, saves)` pair.
//!
//! Units are claimed segment by segment: every unit up to `0xFF` on
//! its own, and above that the ranges between the points where an
//! instruction's answer can change — the line terminators `.` skips.
//! Every instruction the lane serves is constant between them.

use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;

use super::ir::{Edge, MatcherIr, State, UNIT_MAX_NARROW, UNIT_MAX_WIDE};
use crate::flags::unicode_mode;
use crate::parser::RE_FLAG_S;
use crate::program::{Inst, Op, Program};
use crate::vm::char_eq;

/// Build the matcher for `prog` (compiled with `Program::raw_classes`
/// under `flags`) over Latin-1 code units, or over UTF-16 code units
/// when `wide`. `None` when the program is not one-pass, or uses a
/// shape this walk does not serve: assertions, lookaround,
/// backreferences, a literal above 0x7F (the parser spells it as
/// UTF-8 bytes), a property class or a range past U+00FF.
pub fn build(prog: &Program, flags: u8, wide: bool) -> Option<MatcherIr> {
    if !servable(prog) {
        return None;
    }
    let segments = segments(wide);
    let mut states: Vec<State> = Vec::new();
    let mut by_seed: BTreeMap<usize, u32> = BTreeMap::new();
    let mut queue: Vec<usize> = vec![0];
    by_seed.insert(0, 0);
    while let Some(seed) = queue.pop() {
        let id = by_seed[&seed] as usize;
        if states.len() <= id {
            states.resize_with(id + 1, || State {
                edges: Vec::new(),
                accept: None,
            });
        }
        let Closure { consuming, accept } = closure(prog, seed);
        let mut table: Vec<Option<(u32, Vec<u32>)>> = vec![None; segments.len()];
        for (pc, saves) in consuming {
            let next = pc + 1;
            let target = match by_seed.get(&next) {
                Some(&t) => t,
                None => {
                    let t = by_seed.len() as u32;
                    by_seed.insert(next, t);
                    queue.push(next);
                    t
                }
            };
            let ins = prog.insts[pc];
            for (k, &(lo, _)) in segments.iter().enumerate() {
                if !admits(prog, ins, lo) {
                    continue;
                }
                let want = (target, saves.clone());
                match &table[k] {
                    None => table[k] = Some(want),
                    Some(have) if *have == want => {}
                    Some(_) => return None,
                }
            }
        }
        states[id] = State {
            edges: coalesce(&segments, &table),
            accept,
        };
    }
    Some(MatcherIr {
        states,
        stride: crate::vm::saves_arena::detect_stride(prog) as u32,
        wide,
        pairs: wide && unicode_mode(flags),
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
            Some(Op::Char) => falls_through && ins.ch < 0x80,
            Some(Op::AnyChar) => falls_through,
            Some(Op::Class) => {
                falls_through
                    && (ins.a as usize) < prog.classes.len()
                    && single_unit_class(&prog.classes[ins.a as usize])
            }
            Some(Op::Save) => falls_through && ins.a >= 0,
            Some(Op::Jmp) => in_range(ins.a),
            Some(Op::Split) => in_range(ins.a) && in_range(ins.b),
            Some(Op::Match) => true,
            _ => false,
        }
    })
}

/// A class decided by one Latin-1 code unit: the bitmap holds
/// U+0000..U+00FF as parsed (`Program::raw_classes`), so `negate`
/// reaches every unit above 0x7F the way the spec's `[^…]` does;
/// a property table or a range past U+00FF needs the code point,
/// and an expansion leaf (`byte_only`) is a UTF-8 byte, not a unit.
fn single_unit_class(c: &crate::charclass::CharClass) -> bool {
    c.u_prop_tables.is_empty() && c.owned_ranges.is_empty() && !c.byte_only
}

/// Units above `0xFF` where a served instruction's answer can change:
/// the LineTerminators `.` skips (§12.3), each on its own.
const HIGH_POINTS: [u32; 2] = [0x2028, 0x2029];

/// `(lo, hi)` ranges covering every unit the walk can read, each
/// answered alike by every served instruction.
fn segments(wide: bool) -> Vec<(u32, u32)> {
    let mut segs: Vec<(u32, u32)> = (0..=UNIT_MAX_NARROW).map(|u| (u, u)).collect();
    if !wide {
        return segs;
    }
    let mut cuts: Vec<u32> = vec![UNIT_MAX_NARROW + 1];
    for &p in &HIGH_POINTS {
        cuts.push(p);
        cuts.push(p + 1);
    }
    cuts.sort_unstable();
    cuts.dedup();
    for (i, &lo) in cuts.iter().enumerate() {
        let hi = cuts.get(i + 1).map_or(UNIT_MAX_WIDE, |&next| next - 1);
        segs.push((lo, hi));
    }
    segs
}

/// Whether `ins` consumes the code unit `u` (`§22.2.2.7.3`: a `.` skips
/// the four LineTerminators unless `s`; a class is tested as a unit,
/// which under `u` / `v` is the code point for everything the lane
/// serves, a lone surrogate included).
fn admits(prog: &Program, ins: Inst, u: u32) -> bool {
    match Op::from_u8(ins.op) {
        Some(Op::Char) => u <= 0xFF && char_eq(ins.ch, u as u8, ins.pad as u8),
        Some(Op::AnyChar) => {
            ins.pad as u8 & RE_FLAG_S != 0 || !matches!(u, 0x0A | 0x0D | 0x2028 | 0x2029)
        }
        Some(Op::Class) => {
            let ci = ins.pad as u8 & crate::parser::RE_FLAG_I != 0;
            prog.classes[ins.a as usize].test_cp_fold(u as i32, ci)
        }
        _ => false,
    }
}

struct Closure {
    /// consuming pcs in priority order, each with the slots written on
    /// its ε path from the seed
    consuming: Vec<(usize, Vec<u32>)>,
    accept: Option<Vec<u32>>,
}

fn closure(prog: &Program, seed: usize) -> Closure {
    let mut visited = vec![false; prog.len()];
    let mut stack: Vec<(usize, Vec<u32>)> = vec![(seed, Vec::new())];
    let mut consuming = Vec::new();
    let mut accept = None;
    while let Some((pc, saves)) = stack.pop() {
        if visited[pc] {
            continue;
        }
        visited[pc] = true;
        let ins = prog.insts[pc];
        match Op::from_u8(ins.op) {
            Some(Op::Jmp) => stack.push((ins.a as usize, saves)),
            Some(Op::Split) => {
                stack.push((ins.b as usize, saves.clone()));
                stack.push((ins.a as usize, saves));
            }
            Some(Op::Save) => {
                let slot = ins.a as u32;
                let mut saves = saves;
                if let Err(at) = saves.binary_search(&slot) {
                    saves.insert(at, slot);
                }
                stack.push((pc + 1, saves));
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

fn coalesce(segments: &[(u32, u32)], table: &[Option<(u32, Vec<u32>)>]) -> Vec<Edge> {
    let mut edges: Vec<Edge> = Vec::new();
    for (&(lo, hi), cell) in segments.iter().zip(table) {
        let Some((target, saves)) = cell else {
            continue;
        };
        if let Some(last) = edges.last_mut()
            && last.hi + 1 == lo
            && last.target == *target
            && last.saves == *saves
        {
            last.hi = hi;
            continue;
        }
        edges.push(Edge {
            lo,
            hi,
            saves: saves.clone(),
            target: *target,
        });
    }
    edges
}

#[cfg(test)]
mod tests;
