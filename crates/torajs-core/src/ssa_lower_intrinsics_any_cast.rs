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
    /// The `string | undefined` variant: the union's other member
    /// passes as the Str slot's undefined sentinel.
    pub anyv_cast_str_or_undef: FuncId,
    /// One heap family by tag: the cell with +1, or NULL.
    pub anyv_cast_cell: FuncId,
    /// An object-layout slot, or an array of them `(value, cast plan,
    /// vtables, site)`: the cell with +1, a fresh cell copied from an
    /// object of the slot's type, a nullish spelling, or NULL.
    pub anyv_cast_obj: FuncId,
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
        anyv_cast_str_or_undef: decl(
            "__torajs_anyv_cast_str_or_undef",
            &[Type::Any, Type::Str],
            Type::Str,
        ),
        anyv_cast_cell: decl(
            "__torajs_anyv_cast_cell",
            &[Type::Any, Type::I64, Type::Str],
            Type::Ptr,
        ),
        anyv_cast_obj: decl(
            "__torajs_anyv_cast_obj",
            &[Type::Any, Type::I64, Type::Ptr, Type::Str],
            Type::Ptr,
        ),
    }
}

/// The object cast of a plan with an optional member among its fields,
/// which the emitters find by name (`ssa_lower_checked_cast::plan_emit`,
/// the boxed entries' parameter casts). Only a program that writes an
/// optional member (`m?`) has such a plan, so no other declares it.
pub(crate) fn declare_members(
    module: &mut Module,
    fn_table: &mut HashMap<String, FuncId>,
    ast: &crate::ast::Ast,
) {
    if ast.optional_members.none() {
        return;
    }
    let params = [Type::Any, Type::I64, Type::Ptr, Type::Str];
    declare_intrinsic(module, fn_table, MEMBERS_CAST, &params, Type::Ptr);
}

pub(crate) const MEMBERS_CAST: &str = "__torajs_anyv_cast_obj_members";
