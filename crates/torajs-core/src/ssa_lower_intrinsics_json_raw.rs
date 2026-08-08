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
        json_is_raw_json: declare_intrinsic(
            module,
            fn_table,
            "__torajs_json_is_raw_json",
            &[Type::Any],
            Type::Any,
        ),
    }
}
