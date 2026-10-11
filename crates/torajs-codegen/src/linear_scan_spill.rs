//! Spilling at the sweep's active set: when no register of the asked
//! pool is free, the arrival or an active value goes to a stack slot.

use torajs_core::ssa::id_hash::HashMap;

use crate::linear_scan_sweep::Sweep;
use crate::liveness::Interval;
use crate::reg::{Reg, aapcs64};

impl Sweep {
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
            if !class_ok || (callee_only && !is_callee_reg) || self.unspillable.contains(&e.0) {
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
        assert!(
            !pinned || victim_idx.is_some(),
            "no register to evict for unspillable value %{new_vid}"
        );
        let spill_off = self.alloc_spill_slot();
        match victim_idx {
            Some(i)
                if pinned
                    || victim_key.0 < new_weight
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
