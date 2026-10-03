//! The ε-closure of a state's seed in the Pike VM's priority order,
//! the context bits it probes, and the edges a claimed segment table
//! coalesces into.

use alloc::vec;
use alloc::vec::Vec;

use super::ir::{
    Edge, INPUT_END, INPUT_START, LINE_END, LINE_START, WORD_LEFT, WORD_LEFT_FOLD, WORD_RIGHT,
    WORD_RIGHT_FOLD,
};
use crate::parser::{RE_FLAG_I, RE_FLAG_M};
use crate::program::{Inst, Op, Program};

/// Where a walk entered at `pc` starts consuming: a jump writes no slot
/// and reads no context, so the arms of an alternation share the state
/// their exits jump to instead of seeding one state per arm.
pub(super) fn past_jumps(prog: &Program, mut pc: usize) -> usize {
    for _ in 0..prog.len() {
        if Op::from_u8(prog.insts[pc].op) != Some(Op::Jmp) {
            break;
        }
        pc = prog.insts[pc].a as usize;
    }
    pc
}

/// The context bits the assertions reachable from `seed` by ε read.
pub(super) fn probes(prog: &Program, seed: usize, unicode: bool) -> u8 {
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
pub(super) fn unpack(probes: u8, index: usize) -> u8 {
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
pub(super) struct Writes {
    pub(super) saves: Vec<u32>,
    pub(super) clears: Vec<u32>,
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

pub(super) struct Closure {
    /// consuming pcs in priority order, each with the slot writes on
    /// its ε path from the seed
    pub(super) consuming: Vec<(usize, Writes)>,
    pub(super) accept: Option<Writes>,
}

pub(super) fn closure(prog: &Program, seed: usize, ctx: u8, unicode: bool) -> Closure {
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

pub(super) fn coalesce(segments: &[(u32, u32)], table: &[Option<(u32, Writes)>]) -> Vec<Edge> {
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
