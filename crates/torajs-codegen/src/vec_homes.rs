//! The register contract of `Type::Vec` values.
//!
//! Spill slots, call-site saves and the callee-saved FPR convention
//! (AAPCS64 keeps only the low 64 bits of V8-V15) all move 8 bytes,
//! so a vector that is spilled or live across a call would lose its
//! upper lane. The sweep never spills one (`Sweep::unspillable`,
//! seeded here); the only producer, loop widening, emits no call
//! inside a widened loop. Should a vector still end up live across a
//! call, compilation stops rather than corrupt it.

use std::collections::{HashMap, HashSet};

use torajs_core::ssa::{Function, Type};

use crate::linear_scan_calls::crosses_call;
use crate::liveness::Interval;
use crate::reg::Reg;
use crate::regalloc::Assignment;

pub(crate) fn vector_values(func: &Function) -> HashSet<u32> {
    (0..func.values.len() as u32)
        .filter(|&v| matches!(func.values[v as usize].ty, Type::Vec(_)))
        .collect()
}

pub(crate) fn assert_vec_homes(
    func: &Function,
    intervals: &HashMap<u32, Interval>,
    call_sites: &[u32],
    alloc: &Assignment,
) {
    for (vid, info) in func.values.iter().enumerate() {
        if !matches!(info.ty, Type::Vec(_)) {
            continue;
        }
        let Some(&interval) = intervals.get(&(vid as u32)) else {
            continue;
        };
        assert!(
            !crosses_call(interval, call_sites),
            "{}: vector value %{vid} is live across a call",
            func.name
        );
        let home = alloc.of(torajs_core::ssa::ValueId(vid as u32));
        assert!(
            matches!(home, Reg::Fpr(_)),
            "{}: vector value %{vid} has no vector register ({home:?})",
            func.name
        );
    }
}
