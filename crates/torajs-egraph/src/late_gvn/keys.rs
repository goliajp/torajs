//! What the walk needs to know about an instruction besides purity:
//! whether it reads memory, whether it can change memory at all, and
//! which address constants name one symbol.

use std::collections::{HashMap, HashSet};

use torajs_core::ssa::{Function, InstKind, ValueId};

/// A read of memory, at a constant or a computed offset.
pub(super) fn is_read(kind: &InstKind) -> bool {
    matches!(
        kind,
        InstKind::Load(..)
            | InstKind::LoadDyn(..)
            | InstKind::LoadDynScaled8(..)
            | InstKind::LoadU8Dyn(..)
            | InstKind::LoadU16Dyn(..)
    )
}

/// Neither pure for the tables nor able to store: an address constant
/// or a register copy.
pub(super) fn is_register_only(kind: &InstKind) -> bool {
    matches!(
        kind,
        InstKind::GlobalRef(_) | InstKind::StaticStrRef(_) | InstKind::Copy(..)
    )
}

/// Every single-def `global_ref` value mapped to the first one of its
/// symbol in block order.
pub(super) fn global_ref_twins(
    func: &Function,
    multi: &HashSet<ValueId>,
) -> HashMap<ValueId, ValueId> {
    let mut first: HashMap<&str, ValueId> = HashMap::new();
    let mut twin = HashMap::new();
    for inst in func.blocks.iter().flat_map(|b| &b.insts) {
        if let (InstKind::GlobalRef(sym), Some(r)) = (&inst.kind, inst.result)
            && !multi.contains(&r)
        {
            twin.insert(r, *first.entry(sym.as_str()).or_insert(r));
        }
    }
    twin
}
