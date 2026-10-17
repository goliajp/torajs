//! One block of a round of the ascent: evaluate what has to be
//! evaluated, take the rest from the memo, and hand each cell def's
//! value to the round's join.

use std::cell::Cell;

use crate::id_hash::HashMap;

use torajs_core::ssa::{BlockId, Function, InstKind, Operand, Type, ValueId};

use super::elem::ElemPoints;
use super::live::Live;
use super::memo::Memo;
use super::state::State;
use super::transfer::{AbsVal, NumFact, eval_inst};
use super::{lookup, opaque_for, square};

pub(super) struct Walk<'r, 'a> {
    pub(super) func: &'r Function,
    /// per value, its cell's slot in `contrib`, `u32::MAX` for none
    pub(super) slot: &'r [u32],
    /// per block, whether a square bound is live in it
    pub(super) squares: &'r [bool],
    pub(super) elems: &'r ElemPoints,
    pub(super) elem_state: &'r HashMap<ValueId, AbsVal>,
    /// the merge ranges this round reads
    pub(super) merged: &'r [Option<NumFact>],
    pub(super) state: &'r mut State,
    /// per cell, the join of the defs this round evaluated
    pub(super) contrib: &'r mut [Option<AbsVal>],
    pub(super) live: &'r mut Live<'a>,
    pub(super) memo: &'r mut Memo,
    /// the roots of the square bounds this round reads
    pub(super) roots: &'r mut Vec<i128>,
}

impl Walk<'_, '_> {
    /// Walk block `bid`; whether the state of a single def moved.
    pub(super) fn block(&mut self, bid: BlockId) -> bool {
        let func = self.func;
        let slot = self.slot;
        let bi = bid.0 as usize;
        let block = &func.blocks[bi];
        if !self.squares[bi] && self.memo.unmoved(bi) {
            for (j, ev) in self.memo.cell_defs(bi) {
                let r = block.insts[j].result.expect("a cell def has a result");
                let e = self.contrib[slot[r.0 as usize] as usize].get_or_insert(AbsVal::Bottom);
                *e = e.join(ev);
            }
            return false;
        }
        let mut changed = false;
        // the block's constraints are entered once an instruction
        // has to be evaluated, with the cell defs before it killed
        let mut entered = self.squares[bi];
        if entered {
            self.live.enter(bid, self.merged);
            square::live_roots(self.live.all(), self.state, self.roots);
        }
        for (j, inst) in block.insts.iter().enumerate() {
            let Some(r) = inst.result else { continue };
            let elem_root = match self.elems.reads.is_empty() {
                true => None,
                false => self.elems.reads.get(&r),
            };
            let k = slot[r.0 as usize];
            let ev = match self.memo.reuse(bi, j, &inst.kind, elem_root.is_some()) {
                // a single def's state is the value it was last given
                Some(_) if k == u32::MAX => continue,
                Some(ev) => ev,
                None => {
                    if !entered {
                        self.live.enter(bid, self.merged);
                        for d in block.insts[..j].iter().filter_map(|i| i.result) {
                            if slot[d.0 as usize] != u32::MAX {
                                self.live.kill(d);
                            }
                        }
                        entered = true;
                    }
                    let ty = func.values[r.0 as usize].ty;
                    let (ev, read) = eval_at(&inst.kind, ty, self.state, self.live);
                    // The load of a tracked element answers with the
                    // element point rather than the opaque default.
                    // Only for numeric results — a pointer element
                    // carries no interval and no consumer wants one.
                    let ev = match elem_root {
                        Some(root) if matches!(ty, Type::I64 | Type::F64) => {
                            self.elem_state.get(root).copied().unwrap_or(AbsVal::Bottom)
                        }
                        _ => ev,
                    };
                    self.memo.record(bi, j, ev, read);
                    ev
                }
            };
            if k != u32::MAX {
                let e = self.contrib[k as usize].get_or_insert(AbsVal::Bottom);
                *e = e.join(ev);
                if entered {
                    self.live.kill(r);
                }
            } else if self.state.get(&r) != Some(&ev) {
                self.state.insert(r, ev);
                self.memo.moved(r);
                changed = true;
            }
        }
        self.memo.walked(bi);
        changed
    }
}

/// What `kind` yields at the current point of the block `live` holds,
/// and whether it read an operand's fact to say so.
fn eval_at(kind: &InstKind, ty: Type, state: &State, live: &Live) -> (AbsVal, bool) {
    let read = Cell::new(false);
    let env = |op: &Operand| {
        read.set(true);
        lookup(op, state, live.for_operand(op))
    };
    let is_even = |op: &Operand| live.is_even(op);
    let ev = eval_inst(kind, ty, &env, &is_even);
    let ev = if ev == AbsVal::Bottom
        && !matches!(
            kind,
            InstKind::Copy(..)
                | InstKind::Identity(_)
                | InstKind::BinOp(..)
                | InstKind::SiToFp(_)
                | InstKind::FpToSi(_)
                | InstKind::FpToInt32(_)
        ) {
        opaque_for(ty)
    } else {
        ev
    };
    (ev, read.get())
}
