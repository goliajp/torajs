//! `TORAJS_REGALLOC_DUMP=<substring>`: print every value's interval,
//! spill weight, call-crossing verdict and home for each function
//! whose name contains the substring (`*`: every function), and narrate
//! the sweep's shares and evictions (debug surface, mirrors
//! `TORAJS_SSA_DUMP`).

use torajs_core::ssa::id_hash::HashMap;
use torajs_core::ssa::{Function, ValueId};

use crate::linear_scan_calls::{crosses_call, crosses_call_from_entry};
use crate::linear_scan_sweep::Sweep;
use crate::liveness::Interval;

pub(crate) fn wanted(func: &Function) -> bool {
    (crate::switches::get().dump.as_deref()).is_some_and(|s| s == "*" || func.name.contains(s))
}

pub(crate) fn dump(
    func: &Function,
    intervals: &HashMap<u32, Interval>,
    sweep: &Sweep<'_>,
    call_sites: &[u32],
) {
    let mut order: Vec<u32> = intervals.keys().copied().collect();
    order.sort_by_key(|v| (intervals[v].start, *v));
    eprintln!("regalloc {}: calls at {call_sites:?}", func.name);
    for v in order {
        let it = intervals[&v];
        let crossing = if func.params.contains(&ValueId(v)) {
            crosses_call_from_entry(it, call_sites)
        } else {
            crosses_call(it, call_sites)
        };
        eprintln!(
            "  %{v} {:?} [{}, {}] w={} cross={crossing} -> {:?}",
            func.values[v as usize].ty,
            it.start,
            it.end,
            sweep.weights.get(&v).copied().unwrap_or(0),
            sweep.by_value.get(&v),
        );
    }
}
