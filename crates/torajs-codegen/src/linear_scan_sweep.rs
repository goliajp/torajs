//! Mutable Linear-Scan sweep state — the four-way free pool
//! (caller / callee × GPR / FPR) and its allocation / expiry
//! machinery; eviction is in `linear_scan_spill`. Split out of
//! `linear_scan.rs` when the AAPCS64 stack-args blade pushed the file
//! past the 500-line prod limit; `allocate_linear_scan` stays there
//! and drives this.

use std::collections::VecDeque;
use torajs_core::ssa::id_hash::{HashMap, HashSet};

use crate::linear_scan_calls::CallCosts;
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
    pub(crate) fn new(spill_base: u32, weights: HashMap<u32, u64>, call_costs: CallCosts) -> Self {
        Sweep {
            by_value: HashMap::default(),
            active: Vec::new(),
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
}
