//! Where the sweep homes one value: a call-crossing value in a
//! callee-saved register (or a caller-saved one saved around its
//! calls), anything else in a caller-saved register first.

use torajs_core::ssa::block_freq::ENTRY_FREQ;

use crate::linear_scan_sweep::Sweep;
use crate::liveness::Interval;
use crate::reg::Reg;

impl Sweep<'_> {
    /// Home an instruction or alloca result that is not pinned to a
    /// lane, and record it as active.
    pub(crate) fn alloc_value(
        &mut self,
        vid: u32,
        interval: Interval,
        is_fp: bool,
        crossing: bool,
    ) {
        let reg = if crossing && self.unspillable.contains(&vid) {
            self.alloc_crossing_vector(vid, interval)
        } else if crossing && self.save_calls {
            self.alloc_crossing_saved(vid, interval, is_fp)
        } else if crossing {
            self.alloc_crossing(vid, interval, is_fp)
        } else {
            self.alloc_noncrossing(vid, interval, is_fp)
        };
        self.by_value.insert(vid, reg);
        self.activate(vid, interval, reg);
    }

    /// Allocate for a value whose interval crosses a call: callee-
    /// saved only (caller-saved would be clobbered by the BL). Spills
    /// when the callee pool is exhausted.
    pub(crate) fn alloc_crossing(&mut self, vid: u32, interval: Interval, is_fp: bool) -> Reg {
        if let Some(r) = self.share_before_fresh(vid, is_fp) {
            return r;
        }
        let reg = if is_fp {
            self.take_callee_fpr().map(Reg::Fpr)
        } else {
            self.take_callee_gpr().map(Reg::Gpr)
        };
        reg.or_else(|| self.share(vid, is_fp, true))
            .unwrap_or_else(|| {
                self.spill_at_active(vid, interval, is_fp, /*callee_only=*/ true)
            })
    }

    /// Allocate for a value that crosses a call and may be saved around
    /// it: callee-saved first, unless that means saving one more
    /// register on entry for calls that rarely run. With that pool
    /// full, take a caller-saved register and save it around each
    /// crossed call when that is cheaper than spilling this value or
    /// clearing the cheapest callee-saved register; otherwise spill as
    /// `alloc_crossing` does.
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
        if let Some(r) = self.share_before_fresh(vid, is_fp) {
            return r;
        }
        if self.next_callee_is_fresh(is_fp)
            && self.call_costs.save_cost(interval) < ENTRY_FREQ
            && let Some(r) = self.take_caller(is_fp)
        {
            self.save_around_calls(vid);
            return r;
        }
        let callee = if is_fp {
            self.take_callee_fpr().map(Reg::Fpr)
        } else {
            self.take_callee_gpr().map(Reg::Gpr)
        };
        if let Some(r) = callee.or_else(|| self.share(vid, is_fp, true)) {
            return r;
        }
        let weight = |v: u32| self.weights.get(&v).copied().unwrap_or(0);
        let cheapest_clear = self
            .cheapest_clear(self.ranges.of(vid), is_fp, true)
            .map_or(u64::MAX, |(_, cost, _)| cost);
        if self.call_costs.save_cost(interval) < weight(vid).min(cheapest_clear)
            && let Some(r) = self.take_caller(is_fp)
        {
            self.save_around_calls(vid);
            return r;
        }
        self.spill_at_active(vid, interval, is_fp, /*callee_only=*/ true)
    }

    /// Allocate for a vector whose interval crosses a call: any
    /// register, stored whole around each crossed call (a callee keeps
    /// only the low half of V8-V15, and a slot would hold half of it).
    pub(crate) fn alloc_crossing_vector(&mut self, vid: u32, interval: Interval) -> Reg {
        let reg = self
            .take_caller(true)
            .or_else(|| self.take_callee_fpr().map(Reg::Fpr))
            .or_else(|| self.share(vid, true, false))
            .or_else(|| self.share(vid, true, true))
            .unwrap_or_else(|| self.spill_at_active(vid, interval, true, false));
        self.save_around_calls(vid);
        reg
    }

    /// A callee-saved register the frame saves already whose holders
    /// are dead wherever `vid` is live, asked for before one the frame
    /// does not save yet: a function whose cold arms sit after the path
    /// holds many values live across calls at once, each read again
    /// only in its own arm, and a fresh register for each saved eight
    /// more on every entry of a ten-statement `main`. General-purpose
    /// registers only: sharing d8-d15 the same way made the frames
    /// smaller but measurably slower.
    fn share_before_fresh(&mut self, vid: u32, is_fp: bool) -> Option<Reg> {
        if is_fp || !self.next_callee_is_fresh(is_fp) {
            return None;
        }
        self.share(vid, is_fp, true)
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
    /// must be saved/restored), then a register whose holders are dead
    /// wherever this value is live, spill last.
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
        if let Some(r) = self
            .share(vid, is_fp, false)
            .or_else(|| self.share(vid, is_fp, true))
        {
            return r;
        }
        self.spill_at_active(vid, interval, is_fp, /*callee_only=*/ false)
    }
}
