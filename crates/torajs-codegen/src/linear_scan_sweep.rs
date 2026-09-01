//! Mutable Linear-Scan sweep state — the four-way free pool
//! (caller / callee × GPR / FPR) and its allocation / expiry /
//! eviction machinery. Split out of `linear_scan.rs` when the
//! AAPCS64 stack-args blade pushed the file past the 500-line prod
//! limit; `allocate_linear_scan` stays there and drives this.

use std::collections::{HashMap, VecDeque};

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
    /// (ValueId, interval, reg) of every live value that HOLDS a pool
    /// register. Vec — N ≤ pool size, linear scan beats tree
    /// maintenance. A spilled value never enters (`activate`) and a
    /// victim leaves as it spills: it holds no register, so `expire`
    /// releases nothing for it and the victim scan skips it — keeping
    /// it only made both scans O(live values), which a 65k-element
    /// array literal (every element live to the final stores) turned
    /// into a quadratic sweep (555-01).
    pub(crate) active: Vec<(u32, Interval, Reg)>,
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
    /// Round 5 popcount attack #1 — loop-aware spill weights
    /// (`spill_weight::compute_spill_weights`). The victim scan
    /// prefers the LOWEST weight so a loop-carried hot value never
    /// loses its register to a cold prelude ref with a merely-longer
    /// interval.
    pub(crate) weights: HashMap<u32, u32>,
}

impl Sweep {
    pub(crate) fn new(spill_base: u32, weights: HashMap<u32, u32>) -> Self {
        Sweep {
            by_value: HashMap::new(),
            active: Vec::new(),
            free_caller_gpr: aapcs64::CALLER_SAVED_SCRATCH.iter().copied().collect(),
            free_callee_gpr: aapcs64::CALLEE_SAVED_SCRATCH.iter().copied().collect(),
            free_caller_fpr: aapcs64::FP_CALLER_SAVED_SCRATCH.iter().copied().collect(),
            free_callee_fpr: aapcs64::FP_CALLEE_SAVED_SCRATCH.iter().copied().collect(),
            next_spill_offset: spill_base,
            used_callee_gpr_mask: 0,
            used_callee_fpr_mask: 0,
            weights,
        }
    }

    /// Record a live value's home; only a pool register joins the
    /// active set (doc on `active`).
    pub(crate) fn activate(&mut self, vid: u32, interval: Interval, reg: Reg) {
        if !matches!(reg, Reg::SpillGpr(_) | Reg::SpillFpr(_)) {
            self.active.push((vid, interval, reg));
        }
    }

    /// Release every active interval whose end is strictly before
    /// `cur_start`; its register returns to the pool it came from.
    pub(crate) fn expire(&mut self, cur_start: u32) {
        let mut i = 0;
        while i < self.active.len() {
            if self.active[i].1.end < cur_start {
                let reg = self.active[i].2;
                self.release(reg);
                self.active.swap_remove(i);
            } else {
                i += 1;
            }
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

    /// Allocate for a value whose interval crosses a call: callee-
    /// saved only (caller-saved would be clobbered by the BL). Spills
    /// when the callee pool is exhausted.
    pub(crate) fn alloc_crossing(&mut self, vid: u32, interval: Interval, is_fp: bool) -> Reg {
        let reg = if is_fp {
            self.take_callee_fpr().map(Reg::Fpr)
        } else {
            self.take_callee_gpr().map(Reg::Gpr)
        };
        reg.unwrap_or_else(|| {
            self.spill_at_active(vid, interval, is_fp, /*callee_only=*/ true)
        })
    }

    /// Allocate for a value that never crosses a call: prefer caller-
    /// saved (free to clobber), fall back to callee-saved (then it
    /// must be saved/restored), spill last.
    pub(crate) fn alloc_noncrossing(&mut self, vid: u32, interval: Interval, is_fp: bool) -> Reg {
        if is_fp {
            if let Some(f) = self.free_caller_fpr.pop_front() {
                return Reg::Fpr(f);
            }
            if let Some(f) = self.take_callee_fpr() {
                return Reg::Fpr(f);
            }
        } else {
            if let Some(g) = self.free_caller_gpr.pop_front() {
                return Reg::Gpr(g);
            }
            if let Some(g) = self.take_callee_gpr() {
                return Reg::Gpr(g);
            }
        }
        self.spill_at_active(vid, interval, is_fp, /*callee_only=*/ false)
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
        let weight_of = |w: &HashMap<u32, u32>, v: u32| w.get(&v).copied().unwrap_or(0);
        let mut victim_idx: Option<usize> = None;
        let mut victim_key: (u32, i64) = (u32::MAX, -1); // (weight asc, end desc)
        for (i, e) in self.active.iter().enumerate() {
            let (is_callee_reg, class_ok) = match e.2 {
                Reg::Gpr(g) => (aapcs64::CALLEE_SAVED_SCRATCH.contains(&g), !is_fp),
                Reg::Fpr(f) => (aapcs64::FP_CALLEE_SAVED_SCRATCH.contains(&f), is_fp),
                // Already-spilled actives can't free a register.
                Reg::SpillGpr(_) | Reg::SpillFpr(_) => continue,
            };
            if !class_ok || (callee_only && !is_callee_reg) {
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
        let spill_off = self.alloc_spill_slot();
        match victim_idx {
            Some(i)
                if victim_key.0 < new_weight
                    || (victim_key.0 == new_weight
                        && (self.active[i].1.end as i64) > new_interval.end as i64) =>
            {
                let (victim_vid, _vi, victim_reg) = self.active[i];
                let spilled = if is_fp {
                    Reg::SpillFpr(spill_off)
                } else {
                    Reg::SpillGpr(spill_off)
                };
                self.by_value.insert(victim_vid, spilled);
                self.active.swap_remove(i);
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
