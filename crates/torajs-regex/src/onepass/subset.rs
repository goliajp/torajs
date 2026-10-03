//! The matcher of a capture-free program that is not one-pass: the
//! leftmost-first DFA by subset construction, in the same
//! [`MatcherIr`] shape the one-pass walk uses.
//!
//! A state is seeded by the ordered list of pcs the Pike VM holds
//! threads at, highest priority first; the start state by pc 0 alone.
//! A variant's body is the ε-closure of the list in that order, cut at
//! `Op::Match` (threads behind it are lower priority than a match the
//! VM has committed to, so they never consume), and an edge enters
//! the state seeded by the successors of every consuming pc that
//! admits its units, in the order the closure met them. An accept
//! with edges is the fallback the walk returns to when every thread
//! still running dies; a later accept belongs to a higher-priority
//! thread and replaces it, which is what `MatcherIr::exec` does.
//!
//! Without captures the row is the whole match alone, which the walk
//! writes itself, so no edge or accept carries a slot. A program with
//! a capture group needs the one-pass analysis instead.

use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;

use super::analyze::servable;
use super::closure::{Closure, Writes, closure, coalesce, past_jumps, probes, unpack};
use super::ir::{MatcherIr, State, Variant};
use super::units::{Admits, segments};
use crate::flags::unicode_mode;
use crate::parser::RE_FLAG_Y;
use crate::program::{Op, Program};

/// Build the leftmost-first DFA of `prog` (compiled with
/// `Program::raw_classes` under `flags`) over Latin-1 code units, or
/// UTF-16 ones when `wide`. `None` when the program writes a capture
/// slot, uses a shape the walk does not serve (as [`super::build`]),
/// or needs more than `max_states` states.
pub fn build_subset(prog: &Program, flags: u8, wide: bool, max_states: usize) -> Option<MatcherIr> {
    if !servable(prog) || prog.insts.iter().any(writes_slot) {
        return None;
    }
    let unicode = unicode_mode(flags);
    let pairs = wide && unicode;
    let segments = segments(prog, wide, pairs);
    let mut admits = Admits::new(prog, &segments, unicode);
    let mut states: Vec<State> = Vec::new();
    // edge targets by seed list; the start state is not among them,
    // so an edge back to `[0]` enters a state of its own
    let mut by_seeds: BTreeMap<Vec<usize>, u32> = BTreeMap::new();
    let mut queue: Vec<(Vec<usize>, usize)> = vec![(vec![0], 0)];
    while let Some((seeds, id)) = queue.pop() {
        if states.len() <= id {
            states.resize_with(id + 1, || State {
                probes: 0,
                variants: Vec::new(),
            });
        }
        let probes = seeds.iter().fold(0, |p, &s| p | probes(prog, s, unicode));
        let mut variants = Vec::with_capacity(1 << probes.count_ones());
        for index in 0..1usize << probes.count_ones() {
            let ctx = unpack(probes, index);
            let Closure { consuming, accept } = closure(prog, &seeds, ctx, unicode);
            let mut next: Vec<Vec<usize>> = vec![Vec::new(); segments.len()];
            for (pc, _) in consuming {
                let succ = past_jumps(prog, pc + 1);
                for &k in admits.of(pc) {
                    if !next[k].contains(&succ) {
                        next[k].push(succ);
                    }
                }
            }
            let mut table: Vec<Option<(u32, Writes)>> = Vec::with_capacity(segments.len());
            for list in next {
                if list.is_empty() {
                    table.push(None);
                    continue;
                }
                let target = match by_seeds.get(&list) {
                    Some(&t) => t,
                    None => {
                        let t = by_seeds.len() as u32 + 1;
                        if t as usize >= max_states {
                            return None;
                        }
                        by_seeds.insert(list.clone(), t);
                        queue.push((list, t as usize));
                        t
                    }
                };
                table.push(Some((target, Writes::default())));
            }
            variants.push(Variant {
                edges: coalesce(&segments, &table),
                accept: accept.map(|_| Vec::new()),
                accept_clears: Vec::new(),
            });
        }
        states[id] = State { probes, variants };
    }
    Some(MatcherIr {
        states,
        stride: 2,
        wide,
        pairs,
        anchored: flags & RE_FLAG_Y != 0,
    })
}

fn writes_slot(ins: &crate::program::Inst) -> bool {
    matches!(Op::from_u8(ins.op), Some(Op::Save | Op::SaveReset))
}

#[cfg(test)]
mod tests;
