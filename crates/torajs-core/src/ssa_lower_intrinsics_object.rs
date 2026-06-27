//! Pass 0 `declare_intrinsic` group: Object reflection + dynobj
//! substrate + Any-shape dispatch + own-names/keys/values/entries +
//! preventExtensions/seal.
//!
//! chunk 121 continuation of the Pass 0 multi-sub-sibling split
//! (chunks 110-120). 38 declarations covering the contiguous source
//! block between the arr_any group and the fnprops/T-27.b
//! function-as-object table.
//!
//! Subgroups (source order):
//! - dynobj basics: `dynobj_alloc`, `get_builtin_prototype`
//!   (singleton per tag).
//! - instanceof + in: `instanceof_class_any_tag`,
//!   `instanceof_builtin_any_tag`, `instanceof_object_any`,
//!   `in_op_any_num`, `in_op_any_str`.
//! - `any_is_arr` — `Array.isArray(v: any)` runtime tag dispatch.
//! - dynobj read/write: `dynobj_get_tag`, `dynobj_get_value`,
//!   `dynobj_set`, `dynobj_define` (P3.attribute-flag tracking with
//!   `flags_byte`-packed descriptor), `dynobj_define_from_desc` (RFC
//!   20260613 C1 runtime-descriptor variant).
//! - Accessor (RFC 20260613 C3): `accessor_pair_new`,
//!   `accessor_invoke_getter`.
//! - Reflection: `anyv_get_property_descriptor` (P3) +
//!   `anyv_throw_typeerror_if_not_object` (RFC C4b).
//! - reduce-empty: `arr_throw_reduce_empty`,
//!   `arr_throw_reduce_right_empty` (ES §22.1.3.21/22 step 3).
//! - Length descriptor: `arr_length_descriptor` (RFC C5a),
//!   `str_length_descriptor` (W-M).
//! - Own names / keys (W-N): `arr_index_strs`, `str_index_strs`,
//!   `arr_keys_only`, `str_keys_only`.
//! - Object.values / entries (W-O): `str_to_char_arr`,
//!   `arr_entries_by_tag`, `str_entries`.
//! - Struct Any reflection (W-J Phase C): `anyv_struct_keys`,
//!   `anyv_struct_values`, `anyv_struct_entries`.
//! - String index descriptor (W-M-rest): `str_index_descriptor`.
//! - preventExtensions / seal (RFC C5b): `anyv_prevent_extensions`,
//!   `anyv_is_extensible`, `anyv_seal`, `anyv_is_sealed`.
//! - dynobj `has` / `delete` (continuing the dynobj group).

use std::collections::HashMap;

use crate::ssa::{FuncId, Module, Type};
use crate::ssa_lower::declare_intrinsic;

pub(crate) struct ObjectIds {
    pub dynobj_alloc: FuncId,
    pub get_builtin_prototype: FuncId,
    pub instanceof_class_any_tag: FuncId,
    pub instanceof_builtin_any_tag: FuncId,
    pub instanceof_object_any: FuncId,
    pub in_op_any_num: FuncId,
    pub in_op_any_str: FuncId,
    pub any_is_arr: FuncId,
    pub dynobj_get_tag: FuncId,
    pub dynobj_get_value: FuncId,
    pub dynobj_set: FuncId,
    pub dynobj_define: FuncId,
    pub dynobj_define_from_desc: FuncId,
    pub accessor_pair_new: FuncId,
    pub accessor_invoke_getter: FuncId,
    pub get_property_descriptor: FuncId,
    pub throw_typeerror_if_not_object: FuncId,
    pub arr_throw_reduce_empty: FuncId,
    pub arr_throw_reduce_right_empty: FuncId,
    pub arr_length_descriptor: FuncId,
    pub str_length_descriptor: FuncId,
    pub arr_index_strs: FuncId,
    pub str_index_strs: FuncId,
    pub arr_keys_only: FuncId,
    pub str_keys_only: FuncId,
    pub str_to_char_arr: FuncId,
    pub arr_entries_by_tag: FuncId,
    pub str_entries: FuncId,
    pub anyv_struct_keys: FuncId,
    pub anyv_struct_values: FuncId,
    pub anyv_struct_entries: FuncId,
    pub str_index_descriptor: FuncId,
    pub anyv_prevent_extensions: FuncId,
    pub anyv_is_extensible: FuncId,
    pub anyv_seal: FuncId,
    pub anyv_is_sealed: FuncId,
    pub dynobj_has: FuncId,
    pub dynobj_delete: FuncId,
}

pub(crate) fn declare(module: &mut Module, fn_table: &mut HashMap<String, FuncId>) -> ObjectIds {
    ObjectIds {
        dynobj_alloc: declare_intrinsic(module, fn_table, "__torajs_dynobj_alloc", &[], Type::Ptr),
        get_builtin_prototype: declare_intrinsic(
            module,
            fn_table,
            "__torajs_get_builtin_prototype",
            &[Type::I64],
            Type::Ptr,
        ),
        instanceof_class_any_tag: declare_intrinsic(
            module,
            fn_table,
            "__torajs_instanceof_class_any_tag",
            &[Type::Any, Type::I64],
            Type::Bool,
        ),
        instanceof_builtin_any_tag: declare_intrinsic(
            module,
            fn_table,
            "__torajs_instanceof_builtin_any_tag",
            &[Type::Any, Type::I64],
            Type::Bool,
        ),
        instanceof_object_any: declare_intrinsic(
            module,
            fn_table,
            "__torajs_instanceof_object_any",
            &[Type::Any],
            Type::Bool,
        ),
        in_op_any_num: declare_intrinsic(
            module,
            fn_table,
            "__torajs_in_op_any_num",
            &[Type::Any, Type::I64],
            Type::Bool,
        ),
        in_op_any_str: declare_intrinsic(
            module,
            fn_table,
            "__torajs_in_op_any_str",
            &[Type::Any, Type::Ptr],
            Type::Bool,
        ),
        any_is_arr: declare_intrinsic(
            module,
            fn_table,
            "__torajs_any_is_arr",
            &[Type::Any],
            Type::Bool,
        ),
        dynobj_get_tag: declare_intrinsic(
            module,
            fn_table,
            "__torajs_dynobj_get_tag",
            &[Type::Ptr, Type::Ptr],
            Type::I64,
        ),
        dynobj_get_value: declare_intrinsic(
            module,
            fn_table,
            "__torajs_dynobj_get_value",
            &[Type::Ptr, Type::Ptr],
            Type::I64,
        ),
        dynobj_set: declare_intrinsic(
            module,
            fn_table,
            "__torajs_dynobj_set",
            &[Type::Ptr, Type::Ptr, Type::I64, Type::I64],
            Type::Void,
        ),
        dynobj_define: declare_intrinsic(
            module,
            fn_table,
            "__torajs_dynobj_define",
            &[Type::Ptr, Type::Ptr, Type::I64, Type::I64, Type::I64],
            Type::Void,
        ),
        dynobj_define_from_desc: declare_intrinsic(
            module,
            fn_table,
            "__torajs_dynobj_define_from_desc",
            &[Type::Ptr, Type::Ptr, Type::Ptr],
            Type::Void,
        ),
        accessor_pair_new: declare_intrinsic(
            module,
            fn_table,
            "__torajs_accessor_pair_new",
            &[Type::Ptr, Type::Ptr, Type::I64],
            Type::Ptr,
        ),
        accessor_invoke_getter: declare_intrinsic(
            module,
            fn_table,
            "__torajs_accessor_invoke_getter",
            &[Type::Ptr],
            Type::Any,
        ),
        get_property_descriptor: declare_intrinsic(
            module,
            fn_table,
            "__torajs_anyv_get_property_descriptor",
            &[Type::Any, Type::Ptr],
            Type::Any,
        ),
        throw_typeerror_if_not_object: declare_intrinsic(
            module,
            fn_table,
            "__torajs_anyv_throw_typeerror_if_not_object",
            &[Type::Any],
            Type::Void,
        ),
        arr_throw_reduce_empty: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_throw_reduce_empty",
            &[],
            Type::Void,
        ),
        arr_throw_reduce_right_empty: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_throw_reduce_right_empty",
            &[],
            Type::Void,
        ),
        arr_length_descriptor: declare_intrinsic(
            module,
            fn_table,
            "__torajs_anyv_arr_length_descriptor",
            &[Type::I64],
            Type::Any,
        ),
        str_length_descriptor: declare_intrinsic(
            module,
            fn_table,
            "__torajs_anyv_str_length_descriptor",
            &[Type::Ptr],
            Type::Any,
        ),
        arr_index_strs: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_index_strs",
            &[Type::I64],
            Type::Ptr,
        ),
        str_index_strs: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_index_strs",
            &[Type::Ptr],
            Type::Ptr,
        ),
        arr_keys_only: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_keys_only",
            &[Type::I64],
            Type::Ptr,
        ),
        str_keys_only: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_keys_only",
            &[Type::Ptr],
            Type::Ptr,
        ),
        str_to_char_arr: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_to_char_arr",
            &[Type::Ptr],
            Type::Ptr,
        ),
        arr_entries_by_tag: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_entries_by_tag",
            &[Type::Ptr, Type::I64],
            Type::Ptr,
        ),
        str_entries: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_entries",
            &[Type::Ptr],
            Type::Ptr,
        ),
        anyv_struct_keys: declare_intrinsic(
            module,
            fn_table,
            "__torajs_anyv_struct_keys",
            &[Type::Any],
            Type::Ptr,
        ),
        anyv_struct_values: declare_intrinsic(
            module,
            fn_table,
            "__torajs_anyv_struct_values",
            &[Type::Any],
            Type::Ptr,
        ),
        anyv_struct_entries: declare_intrinsic(
            module,
            fn_table,
            "__torajs_anyv_struct_entries",
            &[Type::Any],
            Type::Ptr,
        ),
        str_index_descriptor: declare_intrinsic(
            module,
            fn_table,
            "__torajs_anyv_str_index_descriptor",
            &[Type::Ptr, Type::I64],
            Type::Any,
        ),
        anyv_prevent_extensions: declare_intrinsic(
            module,
            fn_table,
            "__torajs_anyv_prevent_extensions",
            &[Type::Any],
            Type::Any,
        ),
        anyv_is_extensible: declare_intrinsic(
            module,
            fn_table,
            "__torajs_anyv_is_extensible",
            &[Type::Any],
            Type::Bool,
        ),
        anyv_seal: declare_intrinsic(
            module,
            fn_table,
            "__torajs_anyv_seal",
            &[Type::Any],
            Type::Any,
        ),
        anyv_is_sealed: declare_intrinsic(
            module,
            fn_table,
            "__torajs_anyv_is_sealed",
            &[Type::Any],
            Type::Bool,
        ),
        dynobj_has: declare_intrinsic(
            module,
            fn_table,
            "__torajs_dynobj_has",
            &[Type::Ptr, Type::Ptr],
            Type::I32,
        ),
        dynobj_delete: declare_intrinsic(
            module,
            fn_table,
            "__torajs_dynobj_delete",
            &[Type::Ptr, Type::Ptr],
            Type::I32,
        ),
    }
}
