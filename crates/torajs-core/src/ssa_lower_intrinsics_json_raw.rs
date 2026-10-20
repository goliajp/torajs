//! Pass 0 `declare_intrinsic` group: ES2026 json-parse-with-source
//! kernels (torajs-anyvalue `json_raw.rs`).
//!
//! `__torajs_json_raw_json(text_any) -> Any` mints the frozen
//! `[[IsRawJSON]]` carrier (§25.5.1; TypeError / SyntaxError via
//! pending-throw). `__torajs_json_is_raw_json(v_any) -> Any` answers
//! the boxed bool slot probe (§25.5.3; never throws). New group per
//! the rotation-255 rule — the `any_substrate` declare fn is a
//! no-growth debt item.

use std::collections::HashMap;

use crate::ssa::{FuncId, Module, Type};
use crate::ssa_lower::declare_intrinsic;

pub(crate) struct JsonRawIds {
    pub json_raw_json: FuncId,
    pub json_is_raw_json: FuncId,
    pub json_parse_any: FuncId,
    pub json_parse_reviver: FuncId,
    /// The JSON kind of a node of an any-lane parse (0 number,
    /// 1 boolean, 2 string, 3 object, 4 array): the shape walk of an
    /// annotated parse whose typed lane stopped.
    pub json_value_is: FuncId,
    /// §25.5.2 with a `replacer` — value + replacer + gap + starting
    /// depth. The static unfold cannot serve it (a replacer may
    /// substitute any node's value at run time), so the whole walk
    /// happens in the kernel.
    pub json_stringify_full: FuncId,
    /// §25.5.2.3 with step 2 already done. The struct lane calls the
    /// `toJSON` hook in the compiler — it has the static signature and
    /// the property key the runtime's own step 2 does not pass — so
    /// its answer is serialized as a VALUE. The ordinary entry would
    /// run step 2 a second time, on the result.
    pub json_stringify_value: FuncId,
    /// The walk over a value under a property of a statically unfolded
    /// composite — value + gap + depth + the key its `toJSON` hook is
    /// called with (§25.5.2.3 step 2).
    pub json_stringify_keyed: FuncId,
    /// RFC 20260801-ns-object-value (JSON extension) — the `JSON`
    /// namespace singleton in a value position.
    pub ns_object_json: FuncId,
}

pub(crate) fn declare(module: &mut Module, fn_table: &mut HashMap<String, FuncId>) -> JsonRawIds {
    JsonRawIds {
        json_raw_json: declare_intrinsic(
            module,
            fn_table,
            "__torajs_json_raw_json",
            &[Type::Any],
            Type::Any,
        ),
        json_parse_any: declare_intrinsic(
            module,
            fn_table,
            "__torajs_json_parse_any",
            &[Type::Any],
            Type::Any,
        ),
        json_parse_reviver: declare_intrinsic(
            module,
            fn_table,
            "__torajs_json_parse_reviver",
            &[Type::Any, Type::Any],
            Type::Any,
        ),
        json_value_is: declare_intrinsic(
            module,
            fn_table,
            "__torajs_json_value_is",
            &[Type::Any, Type::I64],
            Type::I64,
        ),
        json_stringify_full: declare_intrinsic(
            module,
            fn_table,
            "__torajs_anyv_json_stringify_full",
            &[Type::Any, Type::Any, Type::Str, Type::I64],
            Type::Str,
        ),
        json_stringify_value: declare_intrinsic(
            module,
            fn_table,
            "__torajs_anyv_json_stringify_value",
            &[Type::Any, Type::Str, Type::I64],
            Type::Str,
        ),
        json_stringify_keyed: declare_intrinsic(
            module,
            fn_table,
            "__torajs_anyv_json_stringify_keyed",
            &[Type::Any, Type::Str, Type::I64, Type::Str],
            Type::Str,
        ),
        json_is_raw_json: declare_intrinsic(
            module,
            fn_table,
            "__torajs_json_is_raw_json",
            &[Type::Any],
            Type::Any,
        ),
        ns_object_json: declare_intrinsic(
            module,
            fn_table,
            "__torajs_ns_object_json",
            &[],
            Type::Any,
        ),
    }
}
