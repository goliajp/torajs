//! The slot and integer-view tables the escape walk
//! ([`crate::cmd_build_value_escape`]) reads per function.

use std::collections::HashMap;

use torajs_core::ssa::{Function, InstKind, Operand, Type, ValueId};

/// The slot type of every local slot (`Alloca`) and global address
/// (`GlobalRef`) of `f`; `None` for an untyped one (`AllocaBytes`, a
/// global the module does not declare).
pub(crate) fn slot_types(
    f: &Function,
    global_ty: &HashMap<&str, Type>,
) -> HashMap<ValueId, Option<Type>> {
    f.blocks
        .iter()
        .flat_map(|b| &b.insts)
        .filter_map(|i| {
            let ty = match &i.kind {
                InstKind::Alloca(ty) => Some(*ty),
                InstKind::AllocaBytes(_) => None,
                InstKind::GlobalRef(name) => global_ty.get(name.as_str()).copied(),
                _ => return None,
            };
            i.result.map(|r| (r, ty))
        })
        .collect()
}

/// The integer views (`PtrToInt`) of tracked values, by result.
pub(crate) fn int_views<K>(
    f: &Function,
    key_of: &impl Fn(Type) -> Option<K>,
) -> HashMap<ValueId, K> {
    f.blocks
        .iter()
        .flat_map(|b| &b.insts)
        .filter_map(|i| match (&i.kind, i.result) {
            (InstKind::PtrToInt(Operand::Value(v)), Some(r)) => {
                key_of(f.values[v.0 as usize].ty).map(|k| (r, k))
            }
            _ => None,
        })
        .collect()
}

/// The value an operand names; a constant maps to an id no value has.
pub(crate) fn slot_value(op: &Operand) -> ValueId {
    match op {
        Operand::Value(v) => *v,
        _ => ValueId(u32::MAX),
    }
}
