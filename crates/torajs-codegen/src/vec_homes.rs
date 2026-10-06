//! The register contract of `Type::Vec` values.
//!
//! A spill slot holds 8 bytes and a callee keeps only the low 64 bits
//! of V8-V15 (AAPCS64), so a vector always has a register home
//! (`Sweep::unspillable`, seeded here) and one live across a call is
//! stored whole before the call and reloaded after it, whichever
//! register it sits in (`Assignment::vec_call_saves`).

use torajs_core::ssa::id_hash::HashSet;

use torajs_core::ssa::{Function, Type};

use crate::coalesce::Classes;
use crate::reg::Reg;
use crate::regalloc::Assignment;

/// The values the sweep allocates as vectors: every vector value, as
/// the leader of its copy class (a lane pair shares a class with its
/// low f64 operand).
pub(crate) fn vector_values(func: &Function, classes: &Classes) -> HashSet<u32> {
    (0..func.values.len() as u32)
        .filter(|&v| matches!(func.values[v as usize].ty, Type::Vec(_)))
        .flat_map(|v| [v, classes.leader(v)])
        .collect()
}

pub(crate) fn assert_vec_homes(func: &Function, alloc: &Assignment) {
    for (vid, info) in func.values.iter().enumerate() {
        let v = torajs_core::ssa::ValueId(vid as u32);
        if !matches!(info.ty, Type::Vec(_)) || !alloc.contains(v) {
            continue;
        }
        let home = alloc.of(v);
        assert!(
            matches!(home, Reg::Fpr(_)),
            "{}: vector value %{vid} has no vector register ({home:?})",
            func.name
        );
    }
}
