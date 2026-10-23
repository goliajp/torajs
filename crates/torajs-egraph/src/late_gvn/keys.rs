//! What the walk needs to know about an instruction besides purity:
//! whether it reads memory, whether it can change memory at all, and
//! which address constants name one symbol.

use crate::id_hash::{HashMap, HashSet};

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

/// Neither pure for the tables nor able to store: an address constant,
/// a register copy, or a reinterpretation of a register's bits (the
/// element address of an inline array is a `ptrtoint` / `inttoptr`
/// pair, and must not end the memory epoch it is computed in).
pub(super) fn is_register_only(kind: &InstKind) -> bool {
    matches!(
        kind,
        InstKind::GlobalRef(_)
            | InstKind::StaticStrRef(_)
            | InstKind::Copy(..)
            | InstKind::PtrToInt(_)
            | InstKind::IntToPtr(_)
            | InstKind::BitCastF64ToI64(_)
            | InstKind::BitCastI64ToF64(_)
            | InstKind::TruncI64ToBool(_)
            | InstKind::FnAddr(_)
            | InstKind::BoxedEntryAddr(_)
    )
}

/// Every single-def `global_ref` value mapped to the first one of its
/// symbol in block order.
pub(super) fn global_ref_twins(
    func: &Function,
    multi: &HashSet<ValueId>,
) -> HashMap<ValueId, ValueId> {
    let mut first: HashMap<&str, ValueId> = HashMap::default();
    let mut twin = HashMap::default();
    for inst in func.blocks.iter().flat_map(|b| &b.insts) {
        if let (InstKind::GlobalRef(sym), Some(r)) = (&inst.kind, inst.result)
            && !multi.contains(&r)
        {
            twin.insert(r, *first.entry(sym.as_str()).or_insert(r));
        }
    }
    twin
}
