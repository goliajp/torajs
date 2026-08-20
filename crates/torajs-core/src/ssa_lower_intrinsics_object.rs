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
//!   `anyv_own_values`, `anyv_own_entries`.
//! - String index descriptor (W-M-rest): `str_index_descriptor`.
//! - preventExtensions / seal (RFC C5b): `anyv_prevent_extensions`,
//!   `anyv_is_extensible`, `anyv_seal`, `anyv_is_sealed`.
//! - dynobj `has` / `delete` (continuing the dynobj group).

use std::collections::HashMap;

use crate::ssa::{FuncId, Module, Type};
use crate::ssa_lower::declare_intrinsic;

pub(crate) struct ObjectIds {
    pub dynobj_alloc: FuncId,
    pub dynobj_mark_null_proto: FuncId,
    pub get_builtin_prototype: FuncId,
    pub instanceof_class_any_tag: FuncId,
    pub instanceof_builtin_any_tag: FuncId,
    pub instanceof_object_any: FuncId,
    pub instanceof_generic_any: FuncId,
    pub instanceof_generic_tag: FuncId,
    pub instanceof_fn_value: FuncId,
    pub instanceof_dynamic: FuncId,
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
    pub throw_typeerror_if_not_desc_object: FuncId,
    pub throw_typeerror_if_props_nullish: FuncId,
    /// §20.1.2.2 `Object.create` step-3 outer gate — throws on
    /// `null` only (undefined skips the whole ObjectDefineProperties
    /// clause). Sibling of `throw_typeerror_if_props_nullish` which
    /// throws on both.
    pub throw_typeerror_if_props_null_only: FuncId,
    pub define_props_source_gate: FuncId,
    pub arr_throw_reduce_empty: FuncId,
    pub arr_throw_reduce_right_empty: FuncId,
    pub throw_readonly_assign: FuncId,
    pub arr_length_descriptor: FuncId,
    pub str_length_descriptor: FuncId,
    pub arr_index_strs: FuncId,
    pub str_index_strs: FuncId,
    pub arr_keys_only: FuncId,
    pub arr_keys_only_of: FuncId,
    pub str_keys_only: FuncId,
    /// RC-4 F1c — runtime chooser for keys/gOPN on struct receivers
    /// that may have been dynobj-converted by defineProperty.
    pub obj_own_keys: FuncId,
    /// RC-4 F1c — any-receiver keys/gOPN: DynObj walk or struct arm.
    pub anyv_own_keys: FuncId,
    /// §28.1.11 — the string buckets AND the symbol bucket, the one
    /// walk `Reflect.ownKeys` needs; the two narrower faces each give
    /// only half of it.
    pub anyv_own_keys_all: FuncId,
    /// chunk B2 — for-in keys source: `anyv_own_keys` enumerable
    /// surface, but null / undefined enumerates nothing (§14.7.5).
    pub anyv_forin_keys: FuncId,
    pub anyv_own_symbols: FuncId,
    pub str_to_char_arr: FuncId,
    pub arr_entries_by_tag: FuncId,
    pub str_entries: FuncId,
    pub anyv_struct_keys: FuncId,
    /// Chunk 706 — any-receiver values/entries chooser: DynObj
    /// walk (getter-invoking, ES order) or the struct arm.
    pub anyv_own_values: FuncId,
    pub anyv_own_entries: FuncId,
    /// RFC 20260806-declared-field-redefine — "is this member hidden
    /// from the enumerable-only surfaces right now?", asked per member
    /// by a static unfold that has already failed the header-bit gate.
    pub obj_key_is_nonenumerable: FuncId,
    pub anyv_from_entries: FuncId,
    /// `Object.groupBy(items, cb)` per ES §20.1.2.10 — Array items
    /// lane walker + kb dispatch through the uniform any-call ABI.
    pub object_group_by: FuncId,
    /// `Map.groupBy(items, cb)` per ES §24.2.2.4 — sister to
    /// object_group_by; accumulator is a Map (SameValueZero keys).
    pub map_group_by: FuncId,
    /// `Object.assign` any-target runtime walk (§20.1.2.1 [[Get]]/
    /// [[Set]] per own enumerable key; one source per call).
    pub anyv_assign: FuncId,
    pub str_index_descriptor: FuncId,
    pub anyv_prevent_extensions: FuncId,
    pub anyv_is_extensible: FuncId,
    pub anyv_seal: FuncId,
    pub anyv_is_sealed: FuncId,
    /// RFC 20260716 刀 24 — full `Object.freeze` (FLAG_FROZEN +
    /// FLAG_SEALED + FLAG_NON_EXTENSIBLE + per-entry writable /
    /// configurable clear). Replaces the header-only `obj_freeze` /
    /// `obj_freeze_any` for the SSA lower's `Object.freeze` route so
    /// downstream `getOwnPropertyDescriptor` / `isSealed` /
    /// `isExtensible` observe the frozen level correctly.
    pub anyv_freeze: FuncId,
    pub dynobj_has: FuncId,
    pub dynobj_delete: FuncId,
    /// §28.1.3 Reflect.deleteProperty — the OrdinaryDelete kernel's
    /// no-throw flavor (refusal answers 0 with no pending throw).
    pub any_prop_delete_soft: FuncId,
    /// §28.1.13 Reflect.set — the [[Set]] kernel's no-throw flavor
    /// (refusal answers 0 with no pending throw; setter throws
    /// still propagate).
    pub any_member_set_soft: FuncId,
    /// §10.1.9.2 OrdinarySet with the lookup object and the write
    /// object pulled apart — `Reflect.set`'s four-argument form. The
    /// receiver rides a slot so a DynObj relocation writes back.
    pub any_member_set_with_receiver: FuncId,
    /// §28.1.12 Reflect.setPrototypeOf — boolean-answer flavor of
    /// the OrdinarySetPrototypeOf core (refusal = 0, no throw;
    /// invalid proto still throws).
    pub reflect_set_prototype_of: FuncId,
    /// §28.1.2 Reflect.defineProperty — boolean-answer flavor of the
    /// runtime-descriptor define (refusal = 0, no throw; a
    /// ToPropertyDescriptor throw — getter-backed desc field,
    /// accessor/data mix — still records).
    pub dynobj_define_from_desc_soft: FuncId,
    /// §28.1.1 Reflect.apply — IsCallable gate + the
    /// Function.prototype.apply kernel (nullish argumentsList
    /// throws).
    pub reflect_apply: FuncId,
    /// §28.1.2 Reflect.construct — IsConstructor gates on target and
    /// newTarget, CreateListFromArrayLike, factory-adapter construct,
    /// newTarget [[Prototype]] re-wire (rotation 293).
    pub reflect_construct: FuncId,
    /// RFC 20260801-ns-object-value (Reflect extension) — the
    /// Reflect namespace singleton in a value position.
    pub ns_object_reflect: FuncId,
    /// §19.2.1 — the global `eval` cell in a value position
    /// (identity / typeof / reflection; the call face is the
    /// recorded loud TypeError).
    pub global_eval_value: FuncId,
    /// §7.3.25 CopyDataProperties into the dynobj lane's fresh
    /// literal (`{ ...anySrc }`, rotation 267) — pointer-slot form
    /// so a member_set resize writes the relocated block back.
    pub dynobj_spread_from: FuncId,
    pub object_create_check_proto: FuncId,
    pub object_create_link_proto: FuncId,
    pub anyv_set_prototype_of: FuncId,
    pub anyv_proto_member_set: FuncId,
    pub dynobj_define_properties_from: FuncId,
}

pub(crate) fn declare(module: &mut Module, fn_table: &mut HashMap<String, FuncId>) -> ObjectIds {
    // defined inside the fn so `module` / `fn_table` resolve at the
    // macro definition site (macro_rules locals are hygienic)
    macro_rules! decl {
        ($name:literal, [$($param:ident),*], $ret:ident) => {
            declare_intrinsic(module, fn_table, $name, &[$(Type::$param),*], Type::$ret)
        };
    }
    ObjectIds {
        dynobj_alloc: decl!("__torajs_dynobj_alloc", [], Ptr),
        dynobj_mark_null_proto: decl!("__torajs_dynobj_mark_null_proto", [Ptr], Void),
        get_builtin_prototype: decl!("__torajs_get_builtin_prototype", [I64], Ptr),
        instanceof_class_any_tag: decl!("__torajs_instanceof_class_any_tag", [Any, I64], Bool),
        instanceof_builtin_any_tag: decl!("__torajs_instanceof_builtin_any_tag", [Any, I64], Bool),
        instanceof_object_any: decl!("__torajs_instanceof_object_any", [Any], Bool),
        instanceof_generic_any: decl!("__torajs_instanceof_generic_any", [Any, I64], Bool),
        instanceof_generic_tag: decl!("__torajs_generic_tag_match", [I64, I64], Bool),
        instanceof_fn_value: decl!("__torajs_instanceof_fn_value", [Any, Ptr], Bool),
        instanceof_dynamic: decl!("__torajs_instanceof_dynamic", [Any, Any], Bool),
        in_op_any_num: decl!("__torajs_in_op_any_num", [Any, I64], Bool),
        in_op_any_str: decl!("__torajs_in_op_any_str", [Any, Ptr], Bool),
        any_is_arr: decl!("__torajs_any_is_arr", [Any], Bool),
        dynobj_get_tag: decl!("__torajs_dynobj_get_tag", [Ptr, Ptr], I64),
        dynobj_get_value: decl!("__torajs_dynobj_get_value", [Ptr, Ptr], I64),
        dynobj_set: decl!("__torajs_dynobj_set", [Ptr, Ptr, I64, I64], Void),
        dynobj_define: decl!("__torajs_dynobj_define", [Ptr, Ptr, I64, I64, I64], Void),
        dynobj_define_from_desc: decl!("__torajs_dynobj_define_from_desc", [Ptr, Ptr, Ptr], Void),
        accessor_pair_new: decl!("__torajs_accessor_pair_new", [Ptr, Ptr, I64], Ptr),
        accessor_invoke_getter: decl!("__torajs_accessor_invoke_getter", [Ptr, Any], Any),
        get_property_descriptor: decl!("__torajs_anyv_get_property_descriptor", [Any, Ptr], Any),
        throw_typeerror_if_not_object: decl!(
            "__torajs_anyv_throw_typeerror_if_not_object",
            [Any],
            Void
        ),
        throw_typeerror_if_not_desc_object: decl!(
            "__torajs_anyv_throw_typeerror_if_not_desc_object",
            [Any],
            Void
        ),
        throw_typeerror_if_props_nullish: decl!(
            "__torajs_anyv_throw_typeerror_if_props_nullish",
            [Any],
            Void
        ),
        throw_typeerror_if_props_null_only: decl!(
            "__torajs_anyv_throw_typeerror_if_props_null_only",
            [Any],
            Void
        ),
        define_props_source_gate: decl!("__torajs_anyv_define_props_source_gate", [Any], Ptr),
        arr_throw_reduce_empty: decl!("__torajs_arr_throw_reduce_empty", [], Void),
        arr_throw_reduce_right_empty: decl!("__torajs_arr_throw_reduce_right_empty", [], Void),
        throw_readonly_assign: decl!("__torajs_throw_readonly_assign", [], Void),
        arr_length_descriptor: decl!("__torajs_anyv_arr_length_descriptor", [I64], Any),
        str_length_descriptor: decl!("__torajs_anyv_str_length_descriptor", [Ptr], Any),
        arr_index_strs: decl!("__torajs_arr_index_strs", [I64], Ptr),
        arr_keys_only_of: decl!("__torajs_arr_keys_only_of", [Ptr], Ptr),
        str_index_strs: decl!("__torajs_str_index_strs", [Ptr], Ptr),
        arr_keys_only: decl!("__torajs_arr_keys_only", [I64], Ptr),
        str_keys_only: decl!("__torajs_str_keys_only", [Ptr], Ptr),
        obj_own_keys: decl!("__torajs_obj_own_keys", [Ptr, Ptr, I64], Ptr),
        anyv_own_keys: decl!("__torajs_anyv_own_keys", [Any, I64], Ptr),
        anyv_own_keys_all: decl!("__torajs_anyv_own_keys_all", [Any], Ptr),
        anyv_forin_keys: decl!("__torajs_anyv_forin_keys", [Any], Ptr),
        anyv_own_symbols: decl!("__torajs_anyv_own_symbols", [Any], Ptr),
        str_to_char_arr: decl!("__torajs_str_to_char_arr", [Ptr], Ptr),
        arr_entries_by_tag: decl!("__torajs_arr_entries_by_tag", [Ptr, I64], Ptr),
        str_entries: decl!("__torajs_str_entries", [Ptr], Ptr),
        anyv_struct_keys: decl!("__torajs_anyv_struct_keys", [Any, I64], Ptr),
        anyv_own_values: decl!("__torajs_anyv_own_values", [Any], Ptr),
        anyv_own_entries: decl!("__torajs_anyv_own_entries", [Any], Ptr),
        obj_key_is_nonenumerable: decl!("__torajs_obj_key_is_nonenumerable", [Ptr, Ptr], I64),
        anyv_from_entries: decl!("__torajs_anyv_from_entries", [Any], Any),
        object_group_by: decl!("__torajs_object_group_by", [Any, Any], Any),
        map_group_by: decl!("__torajs_map_group_by", [Any, Any], Any),
        anyv_assign: decl!("__torajs_anyv_assign", [Any, Any], Void),
        str_index_descriptor: decl!("__torajs_anyv_str_index_descriptor", [Ptr, I64], Any),
        anyv_prevent_extensions: decl!("__torajs_anyv_prevent_extensions", [Any], Any),
        anyv_is_extensible: decl!("__torajs_anyv_is_extensible", [Any], Bool),
        anyv_seal: decl!("__torajs_anyv_seal", [Any], Any),
        anyv_is_sealed: decl!("__torajs_anyv_is_sealed", [Any], Bool),
        anyv_freeze: decl!("__torajs_anyv_freeze", [Any], Any),
        dynobj_has: decl!("__torajs_dynobj_has", [Ptr, Ptr], I32),
        dynobj_delete: decl!("__torajs_dynobj_delete", [Ptr, Ptr], I32),
        any_prop_delete_soft: decl!("__torajs_any_prop_delete_soft", [Any, Ptr], I64),
        any_member_set_soft: decl!(
            "__torajs_any_member_set_soft",
            [Ptr, Ptr, I64, I64, I64],
            I64
        ),
        any_member_set_with_receiver: decl!(
            "__torajs_any_member_set_with_receiver",
            [Any, Ptr, I64, I64, Ptr],
            I64
        ),
        reflect_set_prototype_of: decl!("__torajs_reflect_set_prototype_of", [Any, Any], I64),
        dynobj_define_from_desc_soft: decl!(
            "__torajs_dynobj_define_from_desc_soft",
            [Ptr, Ptr, Ptr],
            I64
        ),
        reflect_apply: decl!("__torajs_reflect_apply", [Any, Any, Any], Any),
        reflect_construct: decl!("__torajs_reflect_construct", [Any, Any, Any], Any),
        ns_object_reflect: decl!("__torajs_ns_object_reflect", [], Any),
        global_eval_value: decl!("__torajs_global_eval_value", [], Any),
        dynobj_spread_from: decl!("__torajs_dynobj_spread_from", [Ptr, Any, Ptr], Void),
        object_create_check_proto: decl!("__torajs_object_create_check_proto", [Any], Void),
        object_create_link_proto: decl!("__torajs_object_create_link_proto", [Ptr, Any], Void),
        anyv_set_prototype_of: decl!("__torajs_anyv_set_prototype_of", [Any, Any], Void),
        anyv_proto_member_set: decl!("__torajs_anyv_proto_member_set", [Any, Any], Void),
        dynobj_define_properties_from: decl!(
            "__torajs_dynobj_define_properties_from",
            [Ptr, Ptr],
            Void
        ),
    }
}
