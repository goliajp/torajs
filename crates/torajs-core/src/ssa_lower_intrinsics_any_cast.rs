//! Pass 0 `declare_intrinsic` group: checked casts from `any` into an
//! annotated typed slot (RFC 20260922-any-to-typed-checked-cast, S2
//! kernels in `torajs-anyvalue::anyv_cast`).
//!
//! Every kernel takes the value and a Str literal naming the slot; a
//! value not of the slot's type records a pending TypeError, so each
//! call is followed by the lowering's throw check.

use std::collections::HashMap;

use crate::ssa::{FuncId, Module, Type};
use crate::ssa_lower::declare_intrinsic;

pub(crate) struct AnyCastIds {
    /// `number` slot: the number itself, or NaN after the reject.
    pub anyv_cast_number: FuncId,
    /// `boolean` slot.
    pub anyv_cast_bool: FuncId,
    /// `string` slot: an owned Str, or NULL after the reject.
    pub anyv_cast_str: FuncId,
    /// One heap family by tag: the cell with +1, or NULL.
    pub anyv_cast_cell: FuncId,
}

pub(crate) fn declare(module: &mut Module, fn_table: &mut HashMap<String, FuncId>) -> AnyCastIds {
    let mut decl = |name: &str, params: &[Type], ret: Type| {
        declare_intrinsic(module, fn_table, name, params, ret)
    };
    AnyCastIds {
        anyv_cast_number: decl(
            "__torajs_anyv_cast_number",
            &[Type::Any, Type::Str],
            Type::F64,
        ),
        anyv_cast_bool: decl(
            "__torajs_anyv_cast_bool",
            &[Type::Any, Type::Str],
            Type::Bool,
        ),
        anyv_cast_str: decl("__torajs_anyv_cast_str", &[Type::Any, Type::Str], Type::Str),
        anyv_cast_cell: decl(
            "__torajs_anyv_cast_cell",
            &[Type::Any, Type::I64, Type::Str],
            Type::Ptr,
        ),
    }
}
