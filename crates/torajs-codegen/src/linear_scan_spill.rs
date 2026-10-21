//! Spilling and sharing at the sweep's active set: when no register of
//! the asked pool is free, the arrival takes one whose holders are all
//! dead wherever it is live, or the holders it would meet go to stack
//! slots, or the arrival does.

use crate::linear_scan_sweep::{MAX_HOLDERS, Sweep, slot};
use crate::live_ranges::LiveRanges;
use crate::liveness::Interval;
use crate::reg::{Reg, aapcs64};

/// Whether `reg` is in the asked pool: the arrival's register class,
/// callee-saved or caller-saved.
fn in_pool(reg: Reg, is_fp: bool, callee: bool) -> bool {
    match reg {
        Reg::Gpr(g) if !is_fp && callee => aapcs64::CALLEE_SAVED_SCRATCH.contains(&g),
        Reg::Gpr(g) if !is_fp => aapcs64::CALLER_SAVED_SCRATCH.contains(&g),
        Reg::Fpr(f) if is_fp && callee => aapcs64::FP_CALLEE_SAVED_SCRATCH.contains(&f),
        Reg::Fpr(f) if is_fp => aapcs64::FP_CALLER_SAVED_SCRATCH.contains(&f),
        _ => false,
    }
}

/// Whether `reg` is a register of the arrival's class that may be
/// cleared for it: callee-saved only when `callee_only`.
fn clearable(reg: Reg, is_fp: bool, callee_only: bool) -> bool {
    match reg {
        Reg::Gpr(g) => !is_fp && (!callee_only || aapcs64::CALLEE_SAVED_SCRATCH.contains(&g)),
        Reg::Fpr(f) => is_fp && (!callee_only || aapcs64::FP_CALLEE_SAVED_SCRATCH.contains(&f)),
        Reg::SpillGpr(_) | Reg::SpillFpr(_) => false,
    }
}

impl Sweep<'_> {
    /// A register of the asked pool (callee-saved or caller-saved, of
    /// the arrival's class) every holder of which is dead wherever `vid`
    /// is live, the first such in the order the active set first names
    /// them: the arrival takes it without evicting anyone. A holder
    /// saved around the calls it crosses keeps its register to itself,
    /// since the reload after a call would land on the arrival; a
    /// register with `MAX_HOLDERS` holders takes no more.
    pub(crate) fn share(&mut self, vid: u32, is_fp: bool, callee: bool) -> Option<Reg> {
        if !self.share {
            return None;
        }
        let mine = self.ranges.of(vid);
        if mine.is_empty() {
            return None;
        }
        let mut seen = 0u64;
        for i in 0..self.active.len() {
            let reg = self.active[i].2;
            let k = slot(reg).expect("a pool register");
            if !in_pool(reg, is_fp, callee) || seen & (1 << k) != 0 {
                continue;
            }
            seen |= 1 << k;
            if self.holders[k] >= MAX_HOLDERS || self.saved_holders[k] > 0 {
                continue;
            }
            if self.unions[k].is_none() {
                let mut u: Vec<(u32, u32)> = Vec::new();
                for &(v, _, _) in self.active.iter().filter(|e| e.2 == reg) {
                    u = LiveRanges::union_from(&u, self.ranges.of(v), self.at);
                }
                self.unions[k] = Some(u);
            }
            let union = self.unions[k].as_deref().unwrap_or_default();
            if !LiveRanges::intersect(union, mine) {
                if self.log {
                    eprintln!("  share %{vid} -> {reg:?}");
                }
                return Some(reg);
            }
        }
        None
    }

    /// Poletto & Sarkar 1999 "spill at active" over registers with
    /// several holders: a register of the matching class (callee-saved
    /// only when `callee_only`, i.e. the new arrival crosses a call)
    /// frees up when every holder the arrival would meet spills; the
    /// holders dead on all of its ranges stay and share it. The cheapest
    /// register to clear is the one whose leaving holders weigh least
    /// (frequency-aware spill weights), the furthest end breaking ties;
    /// the arrival spills instead when it weighs less than them, or the
    /// same with the shorter remaining range.
    ///
    /// Returns the reg to assign to the new arrival; the holders that
    /// leave have their `by_value` rewritten to a spill slot each and
    /// drop out of `active`; the register is already in the used-mask
    /// if it was callee-saved.
    pub(crate) fn spill_at_active(
        &mut self,
        new_vid: u32,
        new_interval: Interval,
        is_fp: bool,
        callee_only: bool,
    ) -> Reg {
        let mine: Vec<(u32, u32)> = match self.share {
            true => self.ranges.of(new_vid).to_vec(),
            false => Vec::new(),
        };
        let best = self.cheapest_clear(&mine, is_fp, callee_only);
        let new_weight = self.weights.get(&new_vid).copied().unwrap_or(0);
        let pinned = self.unspillable.contains(&new_vid);
        assert!(
            !pinned || best.is_some(),
            "no register to evict for unspillable value %{new_vid}"
        );
        let spill = |off: u32| match is_fp {
            true => Reg::SpillFpr(off),
            false => Reg::SpillGpr(off),
        };
        match best {
            Some((reg, cost, end))
                if pinned
                    || cost < new_weight
                    || (cost == new_weight && end > new_interval.end as i64) =>
            {
                let mut i = 0;
                while i < self.active.len() {
                    let (vid, _, r) = self.active[i];
                    if r != reg || !self.leaves(vid, &mine) {
                        i += 1;
                        continue;
                    }
                    if self.log {
                        eprintln!("  evict %{vid} {reg:?} for %{new_vid}");
                    }
                    let off = self.spill_home(vid);
                    self.by_value.insert(vid, spill(off));
                    self.deactivate(i);
                }
                // the union still holds the leavers' ranges
                self.unions[slot(reg).expect("a pool register")] = None;
                reg
            }
            _ => {
                if self.log {
                    eprintln!("  spill %{new_vid} (best {best:?})");
                }
                spill(self.spill_home(new_vid))
            }
        }
    }

    /// The register of the asked class and pool that is cheapest to
    /// clear for an arrival live on `mine`, with the summed weight of
    /// the holders that would leave it and the furthest end among them;
    /// registers tie in the order the active set first names them.
    /// `None` when no register of the pool is held. A register one of
    /// whose leavers may not spill, or that would keep `MAX_HOLDERS`
    /// holders besides the arrival, is never chosen.
    pub(crate) fn cheapest_clear(
        &self,
        mine: &[(u32, u32)],
        is_fp: bool,
        callee_only: bool,
    ) -> Option<(Reg, u64, i64)> {
        // per register: summed leaver weight, furthest leaver end,
        // leavers, and whether a leaver may not spill
        let mut acc = [(0u64, -1i64, 0u8, false); 64];
        let (mut seen, mut order) = (0u64, Vec::new());
        for &(v, it, reg) in &self.active {
            if !clearable(reg, is_fp, callee_only) {
                continue;
            }
            let k = slot(reg).expect("a pool register");
            if seen & (1 << k) == 0 {
                seen |= 1 << k;
                order.push(reg);
            }
            if !self.leaves(v, mine) {
                continue;
            }
            let a = &mut acc[k];
            a.0 =
                a.0.saturating_add(self.weights.get(&v).copied().unwrap_or(0));
            a.1 = a.1.max(it.end as i64);
            a.2 += 1;
            a.3 |= self.unspillable.contains(&v);
        }
        let mut best: Option<(Reg, u64, i64)> = None;
        for reg in order {
            let k = slot(reg).unwrap();
            let (cost, end, leavers, pinned) = acc[k];
            if pinned || self.holders[k] - leavers >= MAX_HOLDERS {
                continue;
            }
            if best.is_none_or(|(_, c, e)| cost < c || (cost == c && end > e)) {
                best = Some((reg, cost, end));
            }
        }
        best
    }

    /// Whether holder `vid` has to leave its register for an arrival
    /// live on `mine`: it meets the arrival somewhere, or is saved
    /// around calls (its reload after a call would land on the
    /// arrival). With sharing off every holder leaves.
    fn leaves(&self, vid: u32, mine: &[(u32, u32)]) -> bool {
        !self.share || self.saved.contains(&vid) || LiveRanges::intersect(self.ranges.of(vid), mine)
    }
}
