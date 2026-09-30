//! Byte width and value agreement of constant-offset cell accesses,
//! as the backend lowers them: an `I32` load or store moves 4 bytes,
//! every other type 8.

use torajs_core::ssa::{Function, Operand, Type};

/// Bytes a load of `ty` reads.
pub(crate) fn load_width(ty: Type) -> u64 {
    if ty == Type::I32 { 4 } else { 8 }
}

/// Bytes a store of `val` writes.
pub(crate) fn store_width(func: &Function, val: &Operand) -> u64 {
    operand_ty(func, val).map_or(8, load_width)
}

pub(crate) fn operand_ty(func: &Function, op: &Operand) -> Option<Type> {
    match op {
        Operand::Value(v) => Some(func.value_type(*v)),
        Operand::ConstI64(_) => Some(Type::I64),
        Operand::ConstI32(_) => Some(Type::I32),
        Operand::ConstF64(_) => Some(Type::F64),
        Operand::ConstBool(_) => Some(Type::Bool),
        Operand::ConstPtrNull => None,
    }
}

/// A value of `op` read back as `ty` is the same value: same type, or
/// null into a pointer-shaped field.
pub(crate) fn fits(func: &Function, op: &Operand, ty: Type) -> bool {
    match operand_ty(func, op) {
        Some(t) => t == ty,
        None => ty.is_pointer_shaped(),
    }
}
