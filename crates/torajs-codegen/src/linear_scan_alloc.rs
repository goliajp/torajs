//! Where the sweep homes one value: a call-crossing value in a
//! callee-saved register (or a caller-saved one saved around its
//! calls), anything else in a caller-saved register first. Within a
//! pool, a register whose holders all sit in a hole and miss the
//! value's live ranges comes before a free one (`linear_scan_holes`):
//! a callee-saved one is then already saved by the frame.

use torajs_core::ssa::block_freq::ENTRY_FREQ;

use crate::linear_scan_sweep::Sweep;
use crate::liveness::Interval;
use crate::reg::{Reg, aapcs64};

impl Sweep {
    /// Allocate for a value whose interval crosses a call: callee-
    /// saved only (caller-saved would be clobbered by the BL). Spills
    /// when the callee pool is exhausted.
    pub(crate) fn alloc_crossing(&mut self, vid: u32, interval: Interval, is_fp: bool) -> Reg {
        if let Some(r) = self.inactive_reg(vid, is_fp, true) {
            return r;
        }
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
    /// it: callee-saved first, unless that means saving one more
    /// register on entry for calls that rarely run. With that pool
    /// full, take a caller-saved register and save it around each
    /// crossed call when that is cheaper than spilling this value or the
    /// lightest callee-saved holder; otherwise spill as `alloc_crossing`
    /// does.
    pub(crate) fn alloc_crossing_saved(
        &mut self,
        vid: u32,
        interval: Interval,
        is_fp: bool,
    ) -> Reg {
        // A callee-saved register no value has used yet costs a save and
        // a restore on every entry; when this value's calls run less
        // often than that, save it around them instead (RegAllocGreedy's
        // CSRFirstTimeCost)
        if let Some(r) = self.inactive_reg(vid, is_fp, true) {
            return r;
        }
        if self.next_callee_is_fresh(is_fp)
            && self.call_costs.save_cost(self.ranges.of(vid)) < ENTRY_FREQ
            && let Some(r) = self.take_caller(is_fp)
        {
            self.caller_crossing.push(vid);
            return r;
        }
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
        if self.call_costs.save_cost(self.ranges.of(vid)) < weight(vid).min(lightest_victim)
            && let Some(r) = self.take_caller(is_fp)
        {
            self.caller_crossing.push(vid);
            return r;
        }
        self.spill_at_active(vid, interval, is_fp, /*callee_only=*/ true)
    }

    /// Allocate for a vector whose interval crosses a call: any
    /// register, stored whole around each crossed call (a callee keeps
    /// only the low half of V8-V15, and a slot would hold half of it).
    pub(crate) fn alloc_crossing_vector(&mut self, vid: u32, interval: Interval) -> Reg {
        let reg = self
            .inactive_reg(vid, true, false)
            .or_else(|| self.take_caller(true))
            .or_else(|| self.inactive_reg(vid, true, true))
            .or_else(|| self.take_callee_fpr().map(Reg::Fpr))
            .unwrap_or_else(|| self.spill_at_active(vid, interval, true, false));
        self.caller_crossing.push(vid);
        reg
    }

    fn take_caller(&mut self, is_fp: bool) -> Option<Reg> {
        if is_fp {
            self.free_caller_fpr.pop_front().map(Reg::Fpr)
        } else {
            self.free_caller_gpr.pop_front().map(Reg::Gpr)
        }
    }

    /// Whether the callee-saved register handed out next is one the
    /// frame does not save yet.
    fn next_callee_is_fresh(&self, is_fp: bool) -> bool {
        if is_fp {
            self.free_callee_fpr
                .front()
                .is_some_and(|f| self.used_callee_fpr_mask & (1 << f.idx()) == 0)
        } else {
            self.free_callee_gpr
                .front()
                .is_some_and(|g| self.used_callee_gpr_mask & (1 << g.idx()) == 0)
        }
    }

    /// Allocate for a value that never crosses a call: prefer caller-
    /// saved (free to clobber), fall back to callee-saved (then it
    /// must be saved/restored), spill last.
    pub(crate) fn alloc_noncrossing(&mut self, vid: u32, interval: Interval, is_fp: bool) -> Reg {
        let take_callee = |s: &mut Self| match is_fp {
            true => s.take_callee_fpr().map(Reg::Fpr),
            false => s.take_callee_gpr().map(Reg::Gpr),
        };
        self.inactive_reg(vid, is_fp, false)
            .or_else(|| self.take_caller(is_fp))
            .or_else(|| self.inactive_reg(vid, is_fp, true))
            .or_else(|| take_callee(self))
            .unwrap_or_else(|| {
                self.spill_at_active(vid, interval, is_fp, /*callee_only=*/ false)
            })
    }
}
