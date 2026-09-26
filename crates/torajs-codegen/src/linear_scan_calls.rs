//! Call-site facts the Linear-Scan sweep consults: which intervals
//! cross a `BL` and how much outgoing stack-arg area the frame needs.

use std::collections::HashMap;

use torajs_core::ssa::{Function, InstKind};

use crate::linear_scan_sweep::Sweep;
use crate::liveness::Interval;
use crate::reg::Reg;
use crate::regalloc::{alloca_slot_size, inst_emits_bl};

/// `true` iff some call site falls strictly inside `[start, end]` —
/// the value is defined before a `BL` and still used after it, so a
/// caller-saved register would be clobbered. `p == start` means the
/// value IS the call's result (written after the call returns, safe);
/// `p == end` means it's consumed as that call's argument and dies
/// immediately (safe). The strict `<` on both ends excludes those.
///
/// `call_sites` ascends (pushed in inst-slot order), so the first site
/// past `start` is the only candidate for `(start, end)`. A linear
/// `any` here cost O(calls) per interval — one call and one interval
/// per element of a 65k-element array literal made that a 72s compile
/// (555-01); the sweep is O(n log n) with the bisect.
pub(crate) fn crosses_call(interval: Interval, call_sites: &[u32]) -> bool {
    debug_assert!(call_sites.is_sorted());
    let i = call_sites.partition_point(|&p| p <= interval.start);
    call_sites.get(i).is_some_and(|&p| p < interval.end)
}

/// The param variant of [`crosses_call`]: a param defs at function
/// ENTRY, before inst slot 0, so a call AT `start` clobbers its arg
/// register too (the `p == start` exemption is sound only for a
/// call's own result).
pub(crate) fn crosses_call_from_entry(interval: Interval, call_sites: &[u32]) -> bool {
    debug_assert!(call_sites.is_sorted());
    let i = call_sites.partition_point(|&p| p < interval.start);
    call_sites.get(i).is_some_and(|&p| p < interval.end)
}

/// Outgoing stack-arg area size — when any call site passes more than
/// 8 args of a register class, the overflow goes to stack slots based
/// at the call-moment SP. The area lives at the BOTTOM of the frame
/// (sp+0), so every alloca / spill offset is biased past it;
/// `args.len() - 8` upper-bounds the per-class overflow count (int +
/// fp lanes together absorb at least 8 args), trading at most a few
/// slack slots for not needing callee sigs here.
fn outgoing_arg_bytes(func: &Function) -> u32 {
    let mut outgoing_slots: u32 = 0;
    for block in &func.blocks {
        for inst in &block.insts {
            if let InstKind::Call(_, args) | InstKind::CallIndirect(_, _, args) = &inst.kind {
                outgoing_slots = outgoing_slots.max(args.len().saturating_sub(8) as u32);
            }
        }
    }
    (outgoing_slots * 8 + 15) & !15
}

pub(crate) struct FrameScan {
    pub(crate) alloca_offsets: HashMap<u32, u32>,
    /// First byte past the alloca region (spill slots start here).
    pub(crate) next_alloca_offset: u32,
    pub(crate) has_calls: bool,
    /// Global inst-slot index of every call site, ascending.
    pub(crate) call_sites: Vec<u32>,
}

/// Alloca offsets (biased past the outgoing-arg area), `has_calls`, and
/// the slot index of every call site. The numbering MUST match
/// `compute_intervals` (one index per inst plus one for the
/// terminator) so a call site `p` compares directly against interval
/// bounds.
pub(crate) fn scan_frame(func: &Function) -> FrameScan {
    let mut scan = FrameScan {
        alloca_offsets: HashMap::new(),
        next_alloca_offset: outgoing_arg_bytes(func),
        has_calls: false,
        call_sites: Vec::new(),
    };
    let mut idx: u32 = 0;
    for block in &func.blocks {
        for inst in &block.insts {
            if let Some(slot_size) = alloca_slot_size(&inst.kind) {
                let result = inst.result.expect("Alloca must have result");
                scan.alloca_offsets
                    .insert(result.0, scan.next_alloca_offset);
                scan.next_alloca_offset += slot_size;
            }
            if inst_emits_bl(&inst.kind) {
                scan.has_calls = true;
                scan.call_sites.push(idx);
            }
            idx += 1;
        }
        idx += 1; // terminator slot
    }
    scan
}

/// For every call-crossing value that ended in a caller-saved register,
/// one stack slot and the call sites strictly inside its interval: the
/// value is stored before each such call and reloaded after it. Keyed
/// by call-site slot index.
pub(crate) fn collect_call_saves(
    sweep: &mut Sweep,
    intervals: &HashMap<u32, Interval>,
    call_sites: &[u32],
) -> HashMap<u32, Vec<(Reg, u32)>> {
    let mut out: HashMap<u32, Vec<(Reg, u32)>> = HashMap::new();
    for vid in std::mem::take(&mut sweep.caller_crossing) {
        let reg = sweep.by_value[&vid];
        if matches!(reg, Reg::SpillGpr(_) | Reg::SpillFpr(_)) {
            continue; // evicted after the sweep handed it the register
        }
        let it = intervals[&vid];
        let slot = sweep.alloc_spill_slot();
        let first = call_sites.partition_point(|&p| p <= it.start);
        for &p in call_sites[first..].iter().take_while(|&&p| p < it.end) {
            out.entry(p).or_default().push((reg, slot));
        }
    }
    out
}

/// Estimated frequency of the call sites, as prefix sums: the cost of
/// keeping a value in a caller-saved register is a store and a reload
/// around every call strictly inside its interval.
pub(crate) struct CallCosts {
    sites: Vec<u32>,
    prefix: Vec<u64>,
}

impl CallCosts {
    pub(crate) fn new(call_sites: &[u32], slot_units: &[u64]) -> Self {
        let mut prefix = Vec::with_capacity(call_sites.len() + 1);
        prefix.push(0u64);
        for &p in call_sites {
            prefix.push(prefix.last().unwrap() + slot_units[p as usize]);
        }
        CallCosts {
            sites: call_sites.to_vec(),
            prefix,
        }
    }

    pub(crate) fn save_cost(&self, it: Interval) -> u64 {
        let lo = self.sites.partition_point(|&p| p <= it.start);
        let hi = self.sites.partition_point(|&p| p < it.end).max(lo);
        2 * (self.prefix[hi] - self.prefix[lo])
    }
}
