//! Mutable Linear-Scan sweep state — the four-way free pool
//! (caller / callee × GPR / FPR) and its allocation / expiry
//! machinery; eviction is in `linear_scan_spill`. Split out of
//! `linear_scan.rs` when the AAPCS64 stack-args blade pushed the file
//! past the 500-line prod limit; `allocate_linear_scan` stays there
//! and drives this.

use std::collections::VecDeque;
use torajs_core::ssa::id_hash::{HashMap, HashSet};

use crate::linear_scan_calls::CallCosts;
use crate::live_ranges::{LazyRanges, LiveRanges};
use crate::liveness::Interval;
use crate::reg::{Fpr, Gpr, Reg, aapcs64};

/// Mutable Linear-Scan sweep state. The free pool is split four ways
/// (caller / callee × GPR / FPR) so a call-crossing value can be
/// restricted to callee-saved registers — the ones AAPCS64 §6.1.1
/// guarantees survive a `BL`. Caller-saved registers are handed only
/// to values that never outlive a call.
pub(crate) struct Sweep<'a> {
    /// Final ValueId → register map being built.
    pub(crate) by_value: HashMap<u32, Reg>,
    /// (ValueId, interval, reg) of every live value that HOLDS a pool
    /// register; two values hold one register when neither is live
    /// where the other is (`share`). Vec — N ≤ pool size times
    /// `MAX_HOLDERS`, linear scan beats tree maintenance. A spilled
    /// value never enters (`activate`) and a victim leaves as it
    /// spills: it holds no register, so `expire`
    /// releases nothing for it and the victim scan skips it — keeping
    /// it only made both scans O(live values), which a 65k-element
    /// array literal (every element live to the final stores) turned
    /// into a quadratic sweep (555-01).
    pub(crate) active: Vec<(u32, Interval, Reg)>,
    /// Per register (`slot`): how many entries of `active` hold it, how
    /// many of those are saved around calls, and, once a share has
    /// asked, the union of their live ranges from `at` on (holders
    /// that expired end before it, so they are left in).
    pub(crate) holders: [u8; 64],
    pub(crate) saved_holders: [u8; 64],
    pub(crate) unions: Vec<Option<Vec<(u32, u32)>>>,
    /// Half-slot where the value being placed starts: no later arrival
    /// is live before it.
    pub(crate) at: u32,
    /// Where each value is live inside its interval (`live_ranges`).
    pub(crate) ranges: LazyRanges<'a>,
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
    /// The values of `caller_crossing`, for lookup.
    pub(crate) saved: HashSet<u32>,
    pub(crate) call_costs: CallCosts,
    /// Values that must keep a register: a spill slot holds 8 bytes,
    /// a vector 16. Never chosen as a victim; an arrival among them
    /// always takes one, and is saved whole around the calls it
    /// crosses.
    pub(crate) unspillable: HashSet<u32>,
    /// Whether a value may take a register another holds where the
    /// two are never both live; `TORAJS_SHARE_OFF=1` turns it off
    /// (bisect gate).
    pub(crate) share: bool,
    /// Whether a call-crossing value may take a caller-saved register
    /// saved around its calls; `TORAJS_CALL_SAVE_OFF=1` turns it off
    /// (bisect gate).
    pub(crate) save_calls: bool,
    /// `TORAJS_REGALLOC_DUMP`: narrate evictions and shares to stderr.
    pub(crate) log: bool,
}

/// Most values that hold one register at a time: finding a register to
/// share scans the holders, so an unbounded chain would make the sweep
/// quadratic in the values a long function keeps in holes.
pub(crate) const MAX_HOLDERS: u8 = 32;

/// `holders` index of a pool register.
pub(crate) fn slot(reg: Reg) -> Option<usize> {
    match reg {
        Reg::Gpr(g) => Some(g.idx() as usize),
        Reg::Fpr(f) => Some(32 + f.idx() as usize),
        Reg::SpillGpr(_) | Reg::SpillFpr(_) => None,
    }
}

fn pool<R: Copy>(regs: &[R], n: usize) -> VecDeque<R> {
    regs.iter().take(n).copied().collect()
}

impl<'a> Sweep<'a> {
    /// `TORAJS_REGALLOC_POOL=<n>` keeps the first `n` registers of each
    /// of the four pools, so that sharing and spilling run everywhere
    /// (a test surface for both).
    pub(crate) fn new(
        spill_base: u32,
        weights: HashMap<u32, u64>,
        call_costs: CallCosts,
        ranges: LazyRanges<'a>,
        reserved: Option<Gpr>,
    ) -> Self {
        let switches = crate::switches::get();
        let n = switches.pool;
        Sweep {
            by_value: HashMap::default(),
            active: Vec::new(),
            holders: [0; 64],
            saved_holders: [0; 64],
            unions: vec![None; 64],
            at: 0,
            ranges,
            free_caller_gpr: pool(&aapcs64::CALLER_SAVED_SCRATCH, n),
            free_callee_gpr: pool(&aapcs64::CALLEE_SAVED_SCRATCH, n)
                .into_iter()
                .filter(|&g| Some(g) != reserved)
                .collect(),
            free_caller_fpr: pool(&aapcs64::FP_CALLER_SAVED_SCRATCH, n),
            free_callee_fpr: pool(&aapcs64::FP_CALLEE_SAVED_SCRATCH, n),
            next_spill_offset: spill_base,
            used_callee_gpr_mask: 0,
            used_callee_fpr_mask: 0,
            weights,
            caller_crossing: Vec::new(),
            saved: HashSet::default(),
            call_costs,
            unspillable: HashSet::default(),
            share: !switches.share_off,
            save_calls: !switches.call_save_off,
            log: false,
        }
    }

    /// Record a live value's home; only a pool register joins the
    /// active set (doc on `active`).
    pub(crate) fn activate(&mut self, vid: u32, interval: Interval, reg: Reg) {
        let Some(k) = slot(reg) else { return };
        self.active.push((vid, interval, reg));
        self.holders[k] += 1;
        self.saved_holders[k] += u8::from(self.saved.contains(&vid));
        if let Some(u) = self.unions[k].take() {
            self.unions[k] = Some(LiveRanges::union_from(&u, self.ranges.of(vid), self.at));
        }
    }

    /// Drop `active[i]`; answers whether it was its register's last
    /// holder. A holder that leaves before it expires takes its ranges
    /// out of the union only through `forget_union`.
    pub(crate) fn deactivate(&mut self, i: usize) -> bool {
        let (vid, _, reg) = self.active.swap_remove(i);
        let k = slot(reg).expect("an active value holds a register");
        self.holders[k] -= 1;
        self.saved_holders[k] -= u8::from(self.saved.contains(&vid));
        if self.holders[k] == 0 {
            self.unions[k] = None;
        }
        self.holders[k] == 0
    }

    /// Save a call-crossing value in a caller-saved register around the
    /// calls it crosses (`linear_scan_calls::collect_call_saves`).
    pub(crate) fn save_around_calls(&mut self, vid: u32) {
        self.caller_crossing.push(vid);
        self.saved.insert(vid);
    }

    /// Release every active interval whose end is strictly before
    /// `cur_start`; its register returns to the pool it came from once
    /// no other holder of it is active.
    pub(crate) fn expire(&mut self, cur_start: u32) {
        self.at = 2 * cur_start;
        let mut i = 0;
        while i < self.active.len() {
            if self.active[i].1.end < cur_start {
                let reg = self.active[i].2;
                if self.deactivate(i) {
                    self.release(reg);
                }
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
