//! What the walk needs to know about an instruction besides purity:
//! whether it reads memory, whether it can change memory at all, and
//! which address constants name one symbol.

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{
    Function, InstKind, Operand, Terminator, Type, ValueId, visit_value_operands,
};

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

/// Not able to store: an address constant, a register copy, or a
/// reinterpretation of a register's bits (the element address of an
/// inline array is a `ptrtoint` / `inttoptr` pair, and must not end
/// the memory epoch it is computed in).
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

/// A cast between a raw address and its bits (`ty` the result's): an
/// element address is computed through one, and a second computation
/// of it keys as the first. The casts of boxed values, far more
/// numerous, are not tabled: nothing reads two of them as an address.
pub(super) fn is_address_cast(func: &Function, kind: &InstKind, ty: Type) -> bool {
    match kind {
        InstKind::PtrToInt(Operand::Value(v)) => func.values[v.0 as usize].ty == Type::Ptr,
        InstKind::IntToPtr(_) => matches!(ty, Type::Ptr | Type::Inline(_)),
        _ => false,
    }
}

/// Drop the casts that stood in for a dominating twin and lost their
/// last reader to the dedup (an address recomputed for a load that
/// now reads the first load): left in, they hold a block apart that
/// the compare fusions downstream would take whole.
pub(super) fn drop_unread_casts(func: &mut Function, twins: &HashSet<ValueId>) {
    loop {
        let mut read: HashSet<ValueId> = HashSet::default();
        for block in &func.blocks {
            for inst in &block.insts {
                visit_value_operands(&inst.kind, |v| {
                    read.insert(v);
                });
            }
            match &block.term {
                Terminator::CondBr {
                    cond: Operand::Value(v),
                    ..
                }
                | Terminator::Ret(Some(Operand::Value(v))) => {
                    read.insert(*v);
                }
                _ => {}
            }
        }
        let mut dropped = false;
        for block in func.blocks.iter_mut() {
            block.insts.retain(|i| {
                let dead = i
                    .result
                    .is_some_and(|r| twins.contains(&r) && !read.contains(&r));
                dropped |= dead;
                !dead
            });
        }
        if !dropped {
            return;
        }
    }
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
