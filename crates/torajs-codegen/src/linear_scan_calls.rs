//! Call-site facts the Linear-Scan sweep consults: which intervals
//! cross a `BL`, which values' intervals do not bound their life, and
//! how much outgoing stack-arg area the frame needs.

use std::collections::{HashMap, HashSet};

use torajs_core::ssa::{Function, InstKind};

use crate::liveness::{Interval, visit_inst_operands, visit_terminator_operands};

/// ValueIds whose linear interval does not bound their life: a value
/// read at some slot and written at a LATER one.
///
/// The SSA the lowerer produces is single-assignment and never does
/// this, but φ destruction (`phi_promote::destruct`) is not: it gives
/// one ValueId a `copy` in every predecessor of a join, and a join can
/// be numbered before its predecessors. The interval then runs from
/// the use to the last def, so a call sitting in the join AHEAD of the
/// use falls outside `start < p < end` by construction — no linear
/// range describes a life that runs backwards through block order.
///
/// Only that shape qualifies. A value written more than once with
/// every write ahead of every read still has a sound interval and is
/// left alone — asking for callee-saved there costs registers and buys
/// nothing (measured: a 5% loss on one bench case when this set was
/// merely "multi-def").
///
/// The slot numbering must match [`compute_intervals`]: one index per
/// inst, then one for the terminator.
pub(crate) fn collect_unbounded_values(func: &Function) -> HashSet<u32> {
    let mut first_use: HashMap<u32, u32> = HashMap::new();
    let mut last_def: HashMap<u32, u32> = HashMap::new();
    let mut idx: u32 = 0;
    for block in &func.blocks {
        for inst in &block.insts {
            visit_inst_operands(&inst.kind, |v| {
                first_use.entry(v.0).or_insert(idx);
            });
            if let Some(r) = inst.result {
                last_def.insert(r.0, idx);
            }
            idx += 1;
        }
        visit_terminator_operands(&block.term, |v| {
            first_use.entry(v.0).or_insert(idx);
        });
        idx += 1;
    }
    last_def
        .iter()
        .filter(|(v, d)| first_use.get(*v).is_some_and(|u| u < *d))
        .map(|(v, _)| *v)
        .collect()
}

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
pub(crate) fn outgoing_arg_bytes(func: &Function) -> u32 {
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
