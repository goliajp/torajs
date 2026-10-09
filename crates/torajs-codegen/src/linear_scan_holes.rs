//! The sweep over live ranges with holes (Traub et al. 1998, Wimmer &
//! Mössenböck 2005 without splitting): a value that holds a register
//! sits in `active` while a range of it covers the sweep position and
//! waits in `Holes::waiting` while the position falls in one of its
//! holes. A new value may take a register whose holders are all
//! waiting, when none of their ranges meets its own; a register can
//! thus have several holders at once, and returns to the pool when the
//! last range of the last of them is behind the sweep.
//!
//! Nothing is split: every value keeps one home for its whole life, so
//! two values share a register only when no slot holds both live.
//!
//! Holders are bucketed by register, so finding a register to share
//! looks at the holders of each pool register, not at every waiting
//! value; the waiting ones are ordered by where their next range
//! starts, so the sweep wakes only those whose range has come. A
//! register takes at most `MAX_HOLDERS` holders: every allocation and
//! every victim check intersects the arrival's ranges with each holder
//! of a register, so an unbounded chain (many values each live only
//! in its own exit block at the end of a function) would make the
//! sweep quadratic.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use crate::linear_scan_sweep::Sweep;
use crate::live_ranges::LiveRanges;
use crate::liveness::Interval;
use crate::reg::{Fpr, Gpr, Reg, aapcs64};

const NIL: u32 = u32::MAX;

/// The most values one register holds at a time: generator and async
/// state machines chain up to about 30, a longer chain only costs
/// compile time.
pub(crate) const MAX_HOLDERS: u8 = 32;

/// The holders of every pool register and the ones in a hole.
pub(crate) struct Holes {
    /// Per pool register (`holder_key`), the first of the values holding
    /// it, active or waiting, chained through `next`; the register
    /// returns to the pool when its chain empties.
    head: [u32; 64],
    next: Vec<u32>,
    /// Per register, how many values hold it (at most `MAX_HOLDERS`).
    count: [u8; 64],
    reg_of: [Option<Reg>; 64],
    /// Parallel to `Sweep::active`: the range that covers the sweep
    /// position, as its index among the holder's ranges and its last
    /// slot.
    at: Vec<(u32, u32)>,
    /// `(next range start, vid, its index, hull start, hull end,
    /// register)` of every holder in a hole, earliest start first.
    waiting: BinaryHeap<Reverse<(u32, u32, u32, u32, u32, usize)>>,
    /// Per register, how many of its holders wait; the registers with
    /// a waiting holder; the registers with an active holder (at most
    /// one each: active holders all cover the sweep position).
    waiting_on: [u8; 64],
    waiting_regs: u64,
    active_regs: u64,
    /// The waiting holders spilled while in their hole; their entries
    /// in `waiting` are dropped when they come up.
    gone: Vec<bool>,
}

impl Holes {
    pub(crate) fn new(values: usize) -> Self {
        Holes {
            head: [NIL; 64],
            next: vec![NIL; values],
            count: [0; 64],
            reg_of: [None; 64],
            at: Vec::new(),
            waiting: BinaryHeap::new(),
            waiting_on: [0; 64],
            waiting_regs: 0,
            active_regs: 0,
            gone: vec![false; values],
        }
    }

    pub(crate) fn add(&mut self, reg: Reg, vid: u32) {
        let k = key(reg);
        self.next[vid as usize] = self.head[k];
        self.head[k] = vid;
        self.count[k] += 1;
        self.reg_of[k] = Some(reg);
    }

    /// Unlink `vid` from `reg`'s holders; `true` when none is left.
    fn remove(&mut self, reg: Reg, vid: u32) -> bool {
        let k = key(reg);
        let after = self.next[vid as usize];
        if self.head[k] == vid {
            self.head[k] = after;
        } else {
            let mut h = self.head[k];
            while self.next[h as usize] != vid {
                h = self.next[h as usize];
            }
            self.next[h as usize] = after;
        }
        self.count[k] -= 1;
        self.head[k] == NIL
    }

    fn holders(&self, reg: Reg) -> impl Iterator<Item = u32> + '_ {
        let first = Some(self.head[key(reg)]).filter(|&h| h != NIL);
        std::iter::successors(first, |&h| {
            Some(self.next[h as usize]).filter(|&n| n != NIL)
        })
    }

    fn wait(&mut self, (i, (lo, _)): (usize, (u32, u32)), vid: u32, it: Interval, reg: Reg) {
        let k = key(reg);
        (self.waiting).push(Reverse((lo, vid, i as u32, it.start, it.end, k)));
        self.waiting_on[k] += 1;
        self.waiting_regs |= 1 << k;
    }

    fn wake(&mut self, k: usize) -> Reg {
        self.unwait(k);
        self.reg_of[k].expect("a waiting holder's register")
    }

    fn unwait(&mut self, k: usize) {
        self.waiting_on[k] -= 1;
        if self.waiting_on[k] == 0 {
            self.waiting_regs &= !(1 << k);
        }
    }

    /// Whether `reg` can take one more holder.
    fn has_room(&self, reg: Reg) -> bool {
        self.count[key(reg)] < MAX_HOLDERS
    }
}

/// A pool register's index into `Holes::head`; `None` for a slot.
pub(crate) fn holder_key(reg: Reg) -> Option<usize> {
    match reg {
        Reg::Gpr(g) => Some(g.idx() as usize),
        Reg::Fpr(f) => Some(32 + f.idx() as usize),
        Reg::SpillGpr(_) | Reg::SpillFpr(_) => None,
    }
}

fn key(reg: Reg) -> usize {
    holder_key(reg).expect("holders hold pool registers")
}

/// The `holder_key` bits of each pool.
const CALLER_GPRS: u64 = gpr_bits(&aapcs64::CALLER_SAVED_SCRATCH);
const CALLEE_GPRS: u64 = gpr_bits(&aapcs64::CALLEE_SAVED_SCRATCH);
const CALLER_FPRS: u64 = fpr_bits(&aapcs64::FP_CALLER_SAVED_SCRATCH) << 32;
const CALLEE_FPRS: u64 = fpr_bits(&aapcs64::FP_CALLEE_SAVED_SCRATCH) << 32;

const fn gpr_bits(regs: &[Gpr]) -> u64 {
    let (mut bits, mut i) = (0, 0);
    while i < regs.len() {
        bits |= 1 << regs[i].idx();
        i += 1;
    }
    bits
}

const fn fpr_bits(regs: &[Fpr]) -> u64 {
    let (mut bits, mut i) = (0, 0);
    while i < regs.len() {
        bits |= 1 << regs[i].idx();
        i += 1;
    }
    bits
}

impl Sweep {
    /// Put a holder into `active`, its range being the first of its
    /// ranges that reaches `pos`.
    pub(crate) fn push_active(&mut self, vid: u32, it: Interval, reg: Reg, pos: u32) {
        let at = match self.ranges.next_from(vid, pos, 0) {
            Some((i, (_, hi))) => (i as u32, hi),
            None => (0, it.end),
        };
        self.active.push((vid, it, reg));
        self.holes.at.push(at);
        self.holes.active_regs |= 1 << key(reg);
    }

    /// Take entry `i` out of `active`, keeping its register out of the
    /// pool: it goes to the caller, whose value replaces this one.
    pub(crate) fn take_active(&mut self, i: usize) -> (u32, Interval, Reg) {
        let e = self.active.swap_remove(i);
        self.holes.at.swap_remove(i);
        self.holes.active_regs &= !(1 << key(e.2));
        self.holes.remove(e.2, e.0);
        e
    }

    /// Advance the sweep to `pos`: an active holder whose range ended
    /// moves to its next range or into a hole, a waiting one whose next
    /// range has come back into `active`, and one with no range left
    /// lets go of its register.
    pub(crate) fn expire(&mut self, pos: u32) {
        let mut i = 0;
        while i < self.active.len() {
            let (index, until) = self.holes.at[i];
            if until >= pos {
                i += 1;
                continue;
            }
            let (vid, it, reg) = self.active[i];
            match self.ranges.next_from(vid, pos, index as usize + 1) {
                Some((j, (lo, hi))) if lo <= pos => {
                    self.holes.at[i] = (j as u32, hi);
                    i += 1;
                }
                next => {
                    self.active.swap_remove(i);
                    self.holes.at.swap_remove(i);
                    self.holes.active_regs &= !(1 << key(reg));
                    match next {
                        Some(r) => self.holes.wait(r, vid, it, reg),
                        None => self.drop_holder(vid, reg),
                    }
                }
            }
        }
        while let Some(&Reverse((next, vid, index, start, end, k))) = self.holes.waiting.peek()
            && next <= pos
        {
            self.holes.waiting.pop();
            if self.holes.gone[vid as usize] {
                continue;
            }
            let reg = self.holes.wake(k);
            let it = Interval { start, end };
            match self.ranges.next_from(vid, pos, index as usize) {
                Some((j, (lo, hi))) if lo <= pos => {
                    self.active.push((vid, it, reg));
                    self.holes.at.push((j as u32, hi));
                    self.holes.active_regs |= 1 << k;
                }
                Some(r) => self.holes.wait(r, vid, it, reg),
                None => self.drop_holder(vid, reg),
            }
        }
    }

    fn drop_holder(&mut self, vid: u32, reg: Reg) {
        if self.holes.remove(reg, vid) {
            self.release(reg);
        }
    }

    /// Whether every holder of `reg` other than `holder` misses `vid`.
    pub(crate) fn others_miss(&self, reg: Reg, holder: u32, vid: u32) -> bool {
        let mine = self.ranges.of(vid);
        (self.holes.holders(reg))
            .filter(|&h| h != holder)
            .all(|h| !LiveRanges::intersect(mine, self.ranges.of(h)))
    }

    /// A register of the caller-saved (with `callee`, callee-saved)
    /// pool whose holders all wait in a hole and miss `vid`: `vid` can
    /// share it without either clobbering the other.
    pub(crate) fn inactive_reg(&self, vid: u32, is_fp: bool, callee: bool) -> Option<Reg> {
        let pool = match (is_fp, callee) {
            (false, false) => CALLER_GPRS,
            (false, true) => CALLEE_GPRS,
            (true, false) => CALLER_FPRS,
            (true, true) => CALLEE_FPRS,
        };
        // an active holder covers the sweep position, where `vid` starts
        let held = self.holes.waiting_regs & !self.holes.active_regs & pool;
        if held == 0 {
            return None;
        }
        let mine = self.ranges.of(vid);
        let free = |r: Reg| {
            held & (1 << key(r)) != 0
                && self.holes.has_room(r)
                && (self.holes.holders(r)).all(|h| !LiveRanges::intersect(mine, self.ranges.of(h)))
        };
        match (is_fp, callee) {
            (false, false) => (aapcs64::CALLER_SAVED_SCRATCH.iter())
                .map(|&g| Reg::Gpr(g))
                .find(|&r| free(r)),
            (false, true) => (aapcs64::CALLEE_SAVED_SCRATCH.iter())
                .map(|&g| Reg::Gpr(g))
                .find(|&r| free(r)),
            (true, false) => (aapcs64::FP_CALLER_SAVED_SCRATCH.iter())
                .map(|&f| Reg::Fpr(f))
                .find(|&r| free(r)),
            (true, true) => (aapcs64::FP_CALLEE_SAVED_SCRATCH.iter())
                .map(|&f| Reg::Fpr(f))
                .find(|&r| free(r)),
        }
    }

    /// For a value that must keep a register, when every register of
    /// its class has a holder that meets it and no single spillable
    /// holder frees one: spill every holder that meets `vid` from the
    /// register where they are all spillable and weigh least, and hand
    /// that register over.
    pub(crate) fn evict_holders(&mut self, vid: u32, is_fp: bool, callee_only: bool) -> Reg {
        let mine = self.ranges.of(vid);
        let meeting = |r: Reg| -> Vec<u32> {
            (self.holes.holders(r))
                .filter(|&h| LiveRanges::intersect(mine, self.ranges.of(h)))
                .collect()
        };
        let weight = |v: u32| self.weights.get(&v).copied().unwrap_or(0);
        let pools: &[bool] = if callee_only { &[true] } else { &[false, true] };
        let mut best: Option<(u64, Reg, Vec<u32>)> = None;
        for r in pools.iter().flat_map(|&callee| pool_regs(is_fp, callee)) {
            let hit = meeting(r);
            if hit.iter().any(|h| self.unspillable.contains(h))
                || self.holes.count[key(r)] as usize - hit.len() >= MAX_HOLDERS as usize
            {
                continue;
            }
            let cost = hit.iter().map(|&h| weight(h)).fold(0, u64::saturating_add);
            if best.as_ref().is_none_or(|b| cost < b.0) {
                best = Some((cost, r, hit));
            }
        }
        let (_, reg, hit) = best.unwrap_or_else(|| {
            panic!("no register to evict for unspillable value %{vid}");
        });
        for h in hit {
            match self.active.iter().position(|e| e.0 == h) {
                Some(i) => _ = self.take_active(i),
                None => {
                    self.holes.remove(reg, h);
                    self.holes.unwait(key(reg));
                    self.holes.gone[h as usize] = true;
                }
            }
            let off = self.alloc_spill_slot();
            let slot = if is_fp {
                Reg::SpillFpr(off)
            } else {
                Reg::SpillGpr(off)
            };
            self.by_value.insert(h, slot);
        }
        reg
    }
}

/// The registers of the caller-saved (with `callee`, callee-saved)
/// pool of a class.
fn pool_regs(is_fp: bool, callee: bool) -> Vec<Reg> {
    match (is_fp, callee) {
        (false, false) => aapcs64::CALLER_SAVED_SCRATCH.map(Reg::Gpr).to_vec(),
        (false, true) => aapcs64::CALLEE_SAVED_SCRATCH.map(Reg::Gpr).to_vec(),
        (true, false) => aapcs64::FP_CALLER_SAVED_SCRATCH.map(Reg::Fpr).to_vec(),
        (true, true) => aapcs64::FP_CALLEE_SAVED_SCRATCH.map(Reg::Fpr).to_vec(),
    }
}

#[cfg(test)]
mod tests;
