//! Mutable Linear-Scan sweep state — the four-way free pool
//! (caller / callee × GPR / FPR) and its allocation / expiry /
//! eviction machinery. Split out of `linear_scan.rs` when the
//! AAPCS64 stack-args blade pushed the file past the 500-line prod
//! limit; `allocate_linear_scan` stays there and drives this.

use std::collections::VecDeque;
use torajs_core::ssa::id_hash::{HashMap, HashSet};

use crate::linear_scan_calls::CallCosts;
use crate::linear_scan_holes::{Holes, holder_key};
use crate::live_ranges::LiveRanges;
use crate::liveness::Interval;
use crate::reg::{Fpr, Gpr, Reg, aapcs64};

/// Mutable Linear-Scan sweep state. The free pool is split four ways
/// (caller / callee × GPR / FPR) so a call-crossing value can be
/// restricted to callee-saved registers — the ones AAPCS64 §6.1.1
/// guarantees survive a `BL`. Caller-saved registers are handed only
/// to values that never outlive a call.
pub(crate) struct Sweep {
    /// Final ValueId → register map being built.
    pub(crate) by_value: HashMap<u32, Reg>,
    /// (ValueId, interval, reg) of every value that HOLDS a pool
    /// register and has a live range covering the sweep position; the
    /// holders in a hole of theirs wait in `holes`
    /// (`linear_scan_holes`). Vec — N ≤ pool size, linear scan beats tree
    /// maintenance. A spilled value never enters (`activate`) and a
    /// victim leaves as it spills: it holds no register, so `expire`
    /// releases nothing for it and the victim scan skips it — keeping
    /// it only made both scans O(live values), which a 65k-element
    /// array literal (every element live to the final stores) turned
    /// into a quadratic sweep (555-01).
    pub(crate) active: Vec<(u32, Interval, Reg)>,
    pub(crate) holes: Holes,
    pub(crate) ranges: LiveRanges,
    pub(crate) free_caller_gpr: VecDeque<Gpr>,
    pub(crate) free_callee_gpr: VecDeque<Gpr>,
    pub(crate) free_caller_fpr: VecDeque<Fpr>,
    pub(crate) free_callee_fpr: VecDeque<Fpr>,
    /// sp-relative offset of the next spill slot (grows by 8; all
    /// spilled values are 64-bit GPR or D-form FPR).
    pub(crate) next_spill_offset: u32,
    /// `Gpr::idx` / `Fpr::idx` bitmasks of the callee-saved registers
    /// actually handed out — the frame saves/restores exactly these.
    pub(crate) used_callee_gpr_mask: u32,
    pub(crate) used_callee_fpr_mask: u32,
    /// Frequency-aware spill weights
    /// (`spill_weight::compute_spill_weights`). The victim scan
    /// prefers the LOWEST weight so a loop-carried hot value never
    /// loses its register to a cold prelude ref with a merely-longer
    /// interval.
    pub(crate) weights: HashMap<u32, u64>,
    /// Call-crossing values handed a caller-saved register because the
    /// callee-saved pool was full; each is saved around the calls it
    /// crosses (`linear_scan_calls::collect_call_saves`).
    pub(crate) caller_crossing: Vec<u32>,
    pub(crate) call_costs: CallCosts,
    /// Values that must keep a register: a spill slot holds 8 bytes,
    /// a vector 16. Never chosen as a victim; an arrival among them
    /// always takes one, and is saved whole around the calls it
    /// crosses.
    pub(crate) unspillable: HashSet<u32>,
}

impl Sweep {
    pub(crate) fn new(
        spill_base: u32,
        weights: HashMap<u32, u64>,
        call_costs: CallCosts,
        ranges: LiveRanges,
    ) -> Self {
        Sweep {
            by_value: HashMap::default(),
            active: Vec::new(),
            holes: Holes::new(ranges.values()),
            ranges,
            free_caller_gpr: aapcs64::CALLER_SAVED_SCRATCH.iter().copied().collect(),
            free_callee_gpr: aapcs64::CALLEE_SAVED_SCRATCH.iter().copied().collect(),
            free_caller_fpr: aapcs64::FP_CALLER_SAVED_SCRATCH.iter().copied().collect(),
            free_callee_fpr: aapcs64::FP_CALLEE_SAVED_SCRATCH.iter().copied().collect(),
            next_spill_offset: spill_base,
            used_callee_gpr_mask: 0,
            used_callee_fpr_mask: 0,
            weights,
            caller_crossing: Vec::new(),
            call_costs,
            unspillable: HashSet::default(),
        }
    }

    /// Record a live value's home; only a pool register joins the
    /// active set (doc on `active`).
    pub(crate) fn activate(&mut self, vid: u32, interval: Interval, reg: Reg) {
        if holder_key(reg).is_some() {
            self.holes.add(reg, vid);
            self.push_active(vid, interval, reg, interval.start);
        }
    }

    /// Return a freed register to its originating pool (LIFO — keeps
    /// register pressure tight). Spilled slots release nothing (their
    /// stack offset is owned for the whole function lifetime).
    pub(crate) fn release(&mut self, reg: Reg) {
        match reg {
            Reg::Gpr(g) => {
                if aapcs64::CALLER_SAVED_SCRATCH.contains(&g) {
                    self.free_caller_gpr.push_front(g);
                } else if aapcs64::CALLEE_SAVED_SCRATCH.contains(&g) {
                    self.free_callee_gpr.push_front(g);
                }
            }
            Reg::Fpr(f) => {
                if aapcs64::FP_CALLER_SAVED_SCRATCH.contains(&f) {
                    self.free_caller_fpr.push_front(f);
                } else if aapcs64::FP_CALLEE_SAVED_SCRATCH.contains(&f) {
                    self.free_callee_fpr.push_front(f);
                }
            }
            Reg::SpillGpr(_) | Reg::SpillFpr(_) => {}
        }
    }

    /// Take `reg` out of the free pool for a value that must sit in
    /// that exact register (a parameter kept in its argument lane).
    /// `false` when `reg` is not a pool register.
    pub(crate) fn claim(&mut self, reg: Reg) -> bool {
        let Reg::Gpr(g) = reg else { return false };
        let before = self.free_caller_gpr.len();
        self.free_caller_gpr.retain(|&f| f != g);
        self.free_caller_gpr.len() < before
    }

    /// Pop a callee-saved GPR and record it in the used-mask.
    pub(crate) fn take_callee_gpr(&mut self) -> Option<Gpr> {
        let g = self.free_callee_gpr.pop_front()?;
        self.used_callee_gpr_mask |= 1 << g.idx();
        Some(g)
    }

    /// Pop a callee-saved FPR and record it in the used-mask.
    pub(crate) fn take_callee_fpr(&mut self) -> Option<Fpr> {
        let f = self.free_callee_fpr.pop_front()?;
        self.used_callee_fpr_mask |= 1 << f.idx();
        Some(f)
    }

    pub(crate) fn alloc_spill_slot(&mut self) -> u32 {
        let off = self.next_spill_offset;
        self.next_spill_offset += 8;
        off
    }

    /// Poletto & Sarkar 1999 "spill at active" with pool-class
    /// awareness: pick the furthest-end active value of the matching
    /// class (restricted to callee-saved holders when `callee_only`,
    /// i.e. the new arrival crosses a call) and spill whichever of
    /// {victim, new} lives longer — the longest remaining range
    /// maximizes the freed-register window.
    ///
    /// Returns the reg to assign to the new arrival; if a victim was
    /// chosen, its `by_value` + `active` entry are rewritten to the
    /// spill slot in place and the new arrival inherits its register
    /// (already in the used-mask if it was callee-saved).
    pub(crate) fn spill_at_active(
        &mut self,
        new_vid: u32,
        new_interval: Interval,
        is_fp: bool,
        callee_only: bool,
    ) -> Reg {
        // Round 5 popcount attack #1 — victim ranking is (lowest
        // spill weight, then furthest end). Weight-flat functions
        // degrade to the original Poletto & Sarkar furthest-end
        // choice; a loop-carried accumulator (weight ≫ cold refs)
        // keeps its register even when its interval end reaches
        // function exit.
        let weight_of = |w: &HashMap<u32, u64>, v: u32| w.get(&v).copied().unwrap_or(0);
        let mut victim_idx: Option<usize> = None;
        let mut victim_key: (u64, i64) = (u64::MAX, -1); // (weight asc, end desc)
        for (i, e) in self.active.iter().enumerate() {
            let (is_callee_reg, class_ok) = match e.2 {
                Reg::Gpr(g) => (aapcs64::CALLEE_SAVED_SCRATCH.contains(&g), !is_fp),
                Reg::Fpr(f) => (aapcs64::FP_CALLEE_SAVED_SCRATCH.contains(&f), is_fp),
                // Already-spilled actives can't free a register.
                Reg::SpillGpr(_) | Reg::SpillFpr(_) => continue,
            };
            // the register also has to be free of the victim's
            // co-holders wherever the arrival is live
            if !class_ok
                || (callee_only && !is_callee_reg)
                || self.unspillable.contains(&e.0)
                || !self.others_miss(e.2, e.0, new_vid)
            {
                continue;
            }
            let key = (weight_of(&self.weights, e.0), e.1.end as i64);
            let better = key.0 < victim_key.0 || (key.0 == victim_key.0 && key.1 > victim_key.1);
            if better {
                victim_key = key;
                victim_idx = Some(i);
            }
        }

        // Spill the victim only when the new arrival outranks it:
        // strictly heavier, or equal weight with the shorter
        // remaining range (the original furthest-end rule).
        let new_weight = weight_of(&self.weights, new_vid);
        let pinned = self.unspillable.contains(&new_vid);
        if pinned && victim_idx.is_none() {
            return self.evict_holders(new_vid, is_fp, callee_only);
        }
        let spill_off = self.alloc_spill_slot();
        match victim_idx {
            Some(i)
                if pinned
                    || victim_key.0 < new_weight
                    || (victim_key.0 == new_weight && victim_key.1 > new_interval.end as i64) =>
            {
                let (victim_vid, _vi, victim_reg) = self.take_active(i);
                let spilled = if is_fp {
                    Reg::SpillFpr(spill_off)
                } else {
                    Reg::SpillGpr(spill_off)
                };
                self.by_value.insert(victim_vid, spilled);
                victim_reg
            }
            _ => {
                if is_fp {
                    Reg::SpillFpr(spill_off)
                } else {
                    Reg::SpillGpr(spill_off)
                }
            }
        }
    }
}
