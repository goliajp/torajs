//! Where the sweep homes one value: a call-crossing value in a
//! callee-saved register (or a caller-saved one saved around its
//! calls), anything else in a caller-saved register first.

use crate::linear_scan_sweep::Sweep;
use crate::liveness::Interval;
use crate::reg::{Reg, aapcs64};

impl Sweep {
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

    /// Allocate for a value that crosses a call and may be saved around
    /// it: callee-saved first. With that pool full, take a caller-saved
    /// register and save it around each crossed call when that is
    /// cheaper than spilling this value or the lightest callee-saved
    /// holder; otherwise spill as `alloc_crossing` does.
    pub(crate) fn alloc_crossing_saved(
        &mut self,
        vid: u32,
        interval: Interval,
        is_fp: bool,
    ) -> Reg {
        let callee = if is_fp {
            self.take_callee_fpr().map(Reg::Fpr)
        } else {
            self.take_callee_gpr().map(Reg::Gpr)
        };
        if let Some(r) = callee {
            return r;
        }
        let weight = |v: u32| self.weights.get(&v).copied().unwrap_or(0);
        let lightest_victim = self
            .active
            .iter()
            .filter(|e| match e.2 {
                Reg::Gpr(g) => !is_fp && aapcs64::CALLEE_SAVED_SCRATCH.contains(&g),
                Reg::Fpr(f) => is_fp && aapcs64::FP_CALLEE_SAVED_SCRATCH.contains(&f),
                _ => false,
            })
            .map(|e| weight(e.0))
            .min()
            .unwrap_or(u64::MAX);
        if self.call_costs.save_cost(interval) < weight(vid).min(lightest_victim) {
            let caller = if is_fp {
                self.free_caller_fpr.pop_front().map(Reg::Fpr)
            } else {
                self.free_caller_gpr.pop_front().map(Reg::Gpr)
            };
            if let Some(r) = caller {
                self.caller_crossing.push(vid);
                return r;
            }
        }
        self.spill_at_active(vid, interval, is_fp, /*callee_only=*/ true)
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
}
