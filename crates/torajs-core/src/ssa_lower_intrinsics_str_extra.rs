//! Pass 0 `declare_intrinsic` group: symbol_to_str +
//! `str_*_from` variants (`fromIndex` arg of String.prototype methods)
//! + `symbol_description` + Bool/Null/Undefined → String coerce.
//!
//! chunk 131 continuation of the Pass 0 multi-sub-sibling split
//! (chunks 110-130). 10 declarations covering the short contiguous
//! source block between `_arr_str_etc` and the math block.
//!
//! Subgroups (source order):
//! - `symbol_to_str(Symbol) -> Str` — `Symbol.prototype.toString()`.
//! - **`str_*_from` family** — 5 declarations (`str_index_of_from`,
//!   `_last_index_of_from`, `_starts_with_from`, `_ends_with_from`,
//!   `_includes_from`) for the `fromIndex` second-arg overload of
//!   String.prototype.{indexOf, lastIndexOf, startsWith, endsWith,
//!   includes}. All take `(Ptr, Ptr, I64)` — receiver str ptr +
//!   needle str ptr + fromIndex.
//! - `symbol_description(Symbol) -> Str` — `Symbol.prototype.description`.
//! - **Bool/Null/Undefined → String coerce** (V3-18 m1.d + S142):
//!   `bool_to_str(Bool) -> Str` (ToString(true)="true", false="false"),
//!   `null_to_str() -> Str` (always "null"),
//!   `undefined_to_str() -> Str` (always "undefined" — ES §13.15.3
//!   String + Undefined ToPrimitive(Default) → ToString).

use std::collections::HashMap;

use crate::ssa::{FuncId, Module, Type};
use crate::ssa_lower::declare_intrinsic;

pub(crate) struct StrExtraIds {
    pub symbol_to_str: FuncId,
    pub str_index_of_from: FuncId,
    pub str_last_index_of_from: FuncId,
    pub str_starts_with_from: FuncId,
    pub str_ends_with_from: FuncId,
    pub str_includes_from: FuncId,
    pub symbol_description: FuncId,
    pub bool_to_str: FuncId,
    pub null_to_str: FuncId,
    pub undefined_to_str: FuncId,
    pub str_uri_encode: FuncId,
    pub str_uri_decode: FuncId,
}

pub(crate) fn declare(module: &mut Module, fn_table: &mut HashMap<String, FuncId>) -> StrExtraIds {
    let ppi = &[Type::Ptr, Type::Ptr, Type::I64][..];
    StrExtraIds {
        symbol_to_str: declare_intrinsic(
            module,
            fn_table,
            "__torajs_symbol_to_str",
            &[Type::Symbol],
            Type::Str,
        ),
        str_index_of_from: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_index_of_from",
            ppi,
            Type::I64,
        ),
        str_last_index_of_from: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_last_index_of_from",
            ppi,
            Type::I64,
        ),
        str_starts_with_from: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_starts_with_from",
            ppi,
            Type::Bool,
        ),
        str_ends_with_from: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_ends_with_from",
            ppi,
            Type::Bool,
        ),
        str_includes_from: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_includes_from",
            ppi,
            Type::Bool,
        ),
        symbol_description: declare_intrinsic(
            module,
            fn_table,
            "__torajs_symbol_description",
            &[Type::Symbol],
            Type::Str,
        ),
        bool_to_str: declare_intrinsic(
            module,
            fn_table,
            "__torajs_bool_to_str",
            &[Type::Bool],
            Type::Str,
        ),
        null_to_str: declare_intrinsic(module, fn_table, "__torajs_null_to_str", &[], Type::Str),
        undefined_to_str: declare_intrinsic(
            module,
            fn_table,
            "__torajs_undefined_to_str",
            &[],
            Type::Str,
        ),
        // §19.2.6 URI kernels (torajs-str uri.rs) — (Str, component
        // flag) → fresh Str; the malformed path records a pending
        // URIError, so call sites emit a throw check.
        str_uri_encode: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_uri_encode",
            &[Type::Str, Type::I64],
            Type::Str,
        ),
        str_uri_decode: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_uri_decode",
            &[Type::Str, Type::I64],
            Type::Str,
        ),
    }
}
