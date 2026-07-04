//! Pass 0 `declare_intrinsic` group: fn/arr-as-object side tables +
//! Array<Any> drop + AnyValue ops + proto/class registry +
//! any-unbox/box-drop.
//!
//! chunk 122 continuation of the Pass 0 multi-sub-sibling split
//! (chunks 110-121). 26 declarations covering the contiguous source
//! block from `fnprops_set` through `any_box_drop` (i.e. the
//! `__torajs_fnprops_*` / `__torajs_arrprops_*` / `__torajs_arr_drop_any`
//! / canonical `__torajs_anyv_*` NaN-box AnyValue family / proto +
//! class registry / `__torajs_anyv_unbox_*` + `_rc_dec` block).
//!
//! Subgroups (source order):
//! - **fn-as-object** (T-27.b): `fnprops_set`, `fnprops_get_tag`,
//!   `fnprops_get_value` — hashmap keyed by fn pointer; lazy dynobj
//!   alloc on first prop write.
//! - **arr-as-object** (T-29): `arrprops_set` / `_get_tag` /
//!   `_get_value`.
//! - **Array<Any> drop**: `arr_drop_any`.
//! - **AnyValue ops** (Step 7f-B canonical `__torajs_anyv_*`): `typeof`,
//!   `to_bool`, `to_number`, `add_pair`, `arith_pair`, `compare_pair`,
//!   `strict_eq_imm_pair` (i.e. one operand still typed Any, the
//!   other split into i64-pair), `strict_eq` (both Any), `box_from_pair`,
//!   `payload_rc_inc_pair`.
//! - **Proto/class registry**: `proto_register`, `register_native_error`
//!   (P7.4-a-2 — slot enum: 0=Error 1=TypeError 2=RangeError; factory
//!   = codegen'd `__new_<C>` address), `proto_get`, `class_register`,
//!   `class_get`, `get_proto_of_any`.
//! - **Any unbox / drop**: `anyv_unbox_tag`, `anyv_unbox_value`,
//!   `anyv_rc_dec` (= legacy `any_box_drop`).

use std::collections::HashMap;

use crate::ssa::{FuncId, Module, Type};
use crate::ssa_lower::declare_intrinsic;

pub(crate) struct AnySubstrateIds {
    pub fnprops_set: FuncId,
    pub fnprops_get_tag: FuncId,
    pub fnprops_get_value: FuncId,
    pub arrprops_set: FuncId,
    pub arrprops_get_tag: FuncId,
    pub arrprops_get_value: FuncId,
    pub arr_drop_any: FuncId,
    pub any_typeof: FuncId,
    pub any_to_bool: FuncId,
    pub any_to_number: FuncId,
    pub any_add: FuncId,
    pub any_arith: FuncId,
    pub any_compare: FuncId,
    pub any_strict_eq: FuncId,
    pub any_any_strict_eq: FuncId,
    pub any_box: FuncId,
    pub any_payload_rc_inc: FuncId,
    pub proto_register: FuncId,
    pub register_native_error: FuncId,
    pub proto_get: FuncId,
    pub class_register: FuncId,
    pub class_get: FuncId,
    pub get_proto_of_any: FuncId,
    pub any_index_get: FuncId,
    pub any_index_set: FuncId,
    pub any_length_get: FuncId,
    pub any_size_get: FuncId,
    pub any_regexp_prop: FuncId,
    pub any_iter_next: FuncId,
    pub any_call: FuncId,
    pub any_method_call: FuncId,
    pub any_unbox_tag: FuncId,
    pub any_unbox_value: FuncId,
    pub any_box_drop: FuncId,
}

pub(crate) fn declare(
    module: &mut Module,
    fn_table: &mut HashMap<String, FuncId>,
) -> AnySubstrateIds {
    AnySubstrateIds {
        fnprops_set: declare_intrinsic(
            module,
            fn_table,
            "__torajs_fnprops_set",
            &[Type::Ptr, Type::Ptr, Type::I64, Type::I64],
            Type::Void,
        ),
        fnprops_get_tag: declare_intrinsic(
            module,
            fn_table,
            "__torajs_fnprops_get_tag",
            &[Type::Ptr, Type::Ptr],
            Type::I64,
        ),
        fnprops_get_value: declare_intrinsic(
            module,
            fn_table,
            "__torajs_fnprops_get_value",
            &[Type::Ptr, Type::Ptr],
            Type::I64,
        ),
        arrprops_set: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arrprops_set",
            &[Type::Ptr, Type::Ptr, Type::I64, Type::I64],
            Type::Void,
        ),
        arrprops_get_tag: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arrprops_get_tag",
            &[Type::Ptr, Type::Ptr],
            Type::I64,
        ),
        arrprops_get_value: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arrprops_get_value",
            &[Type::Ptr, Type::Ptr],
            Type::I64,
        ),
        arr_drop_any: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_drop_any",
            &[Type::Ptr],
            Type::Void,
        ),
        any_typeof: declare_intrinsic(
            module,
            fn_table,
            "__torajs_anyv_typeof",
            &[Type::Any],
            Type::Str,
        ),
        any_to_bool: declare_intrinsic(
            module,
            fn_table,
            "__torajs_anyv_to_bool",
            &[Type::Any],
            Type::Bool,
        ),
        any_to_number: declare_intrinsic(
            module,
            fn_table,
            "__torajs_anyv_to_number",
            &[Type::Any],
            Type::F64,
        ),
        any_add: declare_intrinsic(
            module,
            fn_table,
            "__torajs_anyv_add_pair",
            &[Type::I64, Type::I64, Type::I64, Type::I64],
            Type::Any,
        ),
        any_arith: declare_intrinsic(
            module,
            fn_table,
            "__torajs_anyv_arith_pair",
            &[Type::I64, Type::I64, Type::I64, Type::I64, Type::I64],
            Type::Any,
        ),
        any_compare: declare_intrinsic(
            module,
            fn_table,
            "__torajs_anyv_compare_pair",
            &[Type::I64, Type::I64, Type::I64, Type::I64, Type::I64],
            Type::Bool,
        ),
        any_strict_eq: declare_intrinsic(
            module,
            fn_table,
            "__torajs_anyv_strict_eq_imm_pair",
            &[Type::Any, Type::I64, Type::I64],
            Type::Bool,
        ),
        any_any_strict_eq: declare_intrinsic(
            module,
            fn_table,
            "__torajs_anyv_strict_eq",
            &[Type::Any, Type::Any],
            Type::Bool,
        ),
        any_box: declare_intrinsic(
            module,
            fn_table,
            "__torajs_anyv_box_from_pair",
            &[Type::I64, Type::I64],
            Type::Any,
        ),
        any_payload_rc_inc: declare_intrinsic(
            module,
            fn_table,
            "__torajs_anyv_payload_rc_inc_pair",
            &[Type::I64, Type::I64],
            Type::Void,
        ),
        proto_register: declare_intrinsic(
            module,
            fn_table,
            "__torajs_anyv_proto_register",
            &[Type::I64, Type::Any],
            Type::Void,
        ),
        register_native_error: declare_intrinsic(
            module,
            fn_table,
            "__torajs_register_native_error",
            &[Type::I64, Type::Ptr],
            Type::Void,
        ),
        proto_get: declare_intrinsic(
            module,
            fn_table,
            "__torajs_anyv_proto_get",
            &[Type::I64],
            Type::Any,
        ),
        class_register: declare_intrinsic(
            module,
            fn_table,
            "__torajs_anyv_class_register",
            &[Type::I64, Type::Any],
            Type::Void,
        ),
        class_get: declare_intrinsic(
            module,
            fn_table,
            "__torajs_anyv_class_get",
            &[Type::I64],
            Type::Any,
        ),
        get_proto_of_any: declare_intrinsic(
            module,
            fn_table,
            "__torajs_anyv_get_proto_of_any",
            &[Type::Any],
            Type::Any,
        ),
        // Any-dynamic-access RFC (20260704) S3 — recv[idx] where
        // recv is an `any` value (Arr kind-aware / Str / primitive
        // dispatch in torajs-anyvalue::index_any).
        any_index_get: declare_intrinsic(
            module,
            fn_table,
            "__torajs_any_index_get",
            &[Type::Any, Type::I64],
            Type::Any,
        ),
        // RFC 20260704 S3-set — recv[idx] = (tag, value) pair write.
        any_index_set: declare_intrinsic(
            module,
            fn_table,
            "__torajs_any_index_set",
            &[Type::Any, Type::I64, Type::I64, Type::I64],
            Type::Void,
        ),
        // RFC 20260704 S4 — recv.length runtime dispatch.
        any_length_get: declare_intrinsic(
            module,
            fn_table,
            "__torajs_any_length_get",
            &[Type::Any],
            Type::Any,
        ),
        // RFC 20260704 C4-2 — recv.size runtime dispatch (Map/Set
        // live count + dynobj probe).
        any_size_get: declare_intrinsic(
            module,
            fn_table,
            "__torajs_any_size_get",
            &[Type::Any],
            Type::Any,
        ),
        // RFC 20260704 C4-3c-2 — RegExp accessor surface (source /
        // flags / lastIndex / flag booleans) + dynobj probe.
        any_regexp_prop: declare_intrinsic(
            module,
            fn_table,
            "__torajs_any_regexp_prop",
            &[Type::Any, Type::I64, Type::Ptr],
            Type::Any,
        ),
        // RFC 20260704 S5+ — unified for-of iteration protocol
        // (indexed strings/arrays + stateful MapIter/ArrIter cells;
        // throws on non-iterable receivers). (recv, idx-cursor slot,
        // owned-AnyValue out slot) → live flag.
        any_iter_next: declare_intrinsic(
            module,
            fn_table,
            "__torajs_any_iter_next",
            &[Type::Any, Type::Ptr, Type::Ptr],
            Type::I64,
        ),
        // RFC C4+ — bare any-call `f(args…)`: (callee, argv, argc)
        // → Any; non-closures raise a catchable TypeError.
        any_call: declare_intrinsic(
            module,
            fn_table,
            "__torajs_any_call",
            &[Type::Any, Type::Ptr, Type::I64],
            Type::Any,
        ),
        // Any-method-call RFC 20260704 C1 — recv.name(args…) runtime
        // dispatch: (recv, method-id, name ptr/len for TypeError
        // messages, receiver write-back slot, argv, argc) → Any.
        any_method_call: declare_intrinsic(
            module,
            fn_table,
            "__torajs_any_method_call",
            &[
                Type::Any,
                Type::I64,
                Type::Ptr,
                Type::I64,
                Type::Ptr,
                Type::Ptr,
                Type::I64,
            ],
            Type::Any,
        ),
        any_unbox_tag: declare_intrinsic(
            module,
            fn_table,
            "__torajs_anyv_unbox_tag",
            &[Type::Any],
            Type::I64,
        ),
        any_unbox_value: declare_intrinsic(
            module,
            fn_table,
            "__torajs_anyv_unbox_value",
            &[Type::Any],
            Type::I64,
        ),
        any_box_drop: declare_intrinsic(
            module,
            fn_table,
            "__torajs_anyv_rc_dec",
            &[Type::Any],
            Type::Void,
        ),
    }
}
