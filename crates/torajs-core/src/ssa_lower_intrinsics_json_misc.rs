//! Pass 0 `declare_intrinsic` group: JSON.stringify/parse runtime +
//! str_eq_cstr + stderr print + Array immutable ops (flat / concat
//! / reverse / fill / copy_within).
//!
//! chunk 133 continuation of the Pass 0 multi-sub-sibling split
//! (chunks 110-132). 28 declarations covering the contiguous source
//! block from `json_quote_str` through `arr_copy_within`, just
//! before the throw state runtime.
//!
//! Subgroups (source order):
//! - **`json_quote_str(s) -> Str`** — quote helper.
//! - **JSON builder** (V0.2 P14-S5 fast path for `lower_json_stringify`
//!   Type::Obj arm — replaces a 16-call `str_concat` chain with the
//!   accumulator API; O(N²) → O(N)). `jsb_new(cap)` / `_push_byte` /
//!   `_push_str_raw` / `_push_str_quoted` / `_push_i64` / `_push_bool`
//!   / `_finalize -> Str`. See `crates/torajs-str/src/json_builder.rs`.
//! - **JSON.parse helpers** (M6.3): cursor (`*i64`) threaded through
//!   every helper, each advances past the consumed token. On
//!   syntactic mismatch a `__torajs_throw_set` fires so ssa_lower's
//!   `throw_check` after the call propagates.
//!   - `json_eat_char(s, cursor_ptr, expected)`
//!   - `json_parse_int(s, cursor) -> i64`
//!   - `json_parse_float(s, cursor) -> f64`
//!   - `json_parse_bool(s, cursor) -> i64`
//!   - `json_parse_string(s, cursor) -> Str`
//!   - `json_arr_step(s, cursor, depth) -> i64`
//!   - `json_arr_first(s, cursor, depth) -> i64`
//! - **`str_eq_cstr(s, cstr_ptr, cstr_len) -> i64`** — needle is a
//!   constant byte buffer (lower wraps as `Ptr + I64`).
//! - **Array immutable / bulk ops**:
//!   - `arr_flat(arr) -> Ptr` (depth-1 flatten of typed Array<Array<T>>).
//!   - `arr_flat_any(arr) -> Ptr` (Array<Any> flatten).
//!   - `arr_extend_typed_into_any(dst, src, src_tag) -> Ptr` — bulk
//!     extend a typed src into an Array<Any> dst, wrapping each src
//!     element as the given AnyValue tag.
//!   - `arr_concat(a, b) -> Ptr`.
//!   - `arr_reverse(arr) -> Ptr` (in-place; returns receiver).
//!   - `arr_fill(arr, val, start, end) -> Ptr`.
//!   - `arr_copy_within(arr, target, start, end) -> Ptr`.

use std::collections::HashMap;

use crate::ssa::{FuncId, Module, Type};
use crate::ssa_lower::declare_intrinsic;

pub(crate) struct JsonMiscIds {
    pub json_quote_str: FuncId,
    pub json_quote_str_top: FuncId,
    pub jsb_new: FuncId,
    pub jsb_push_byte: FuncId,
    pub jsb_push_str_raw: FuncId,
    pub jsb_push_str_quoted: FuncId,
    pub jsb_push_i64: FuncId,
    pub jsb_push_bool: FuncId,
    pub jsb_finalize: FuncId,
    pub jsb_begin_field: FuncId,
    pub jsb_push_field_str: FuncId,
    pub json_obj_sep: FuncId,
    pub json_indent: FuncId,
    pub json_colon: FuncId,
    pub json_close_indent: FuncId,
    pub json_eat_char: FuncId,
    pub json_parse_int: FuncId,
    pub json_parse_float: FuncId,
    pub json_parse_bool: FuncId,
    pub json_parse_string: FuncId,
    pub json_arr_step: FuncId,
    pub json_arr_first: FuncId,
    pub str_eq_cstr: FuncId,
    pub arr_flat: FuncId,
    pub arr_flat_any: FuncId,
    pub arr_extend_typed_into_any: FuncId,
    pub arr_concat: FuncId,
    pub arr_reverse: FuncId,
    pub arr_fill: FuncId,
    pub arr_copy_within: FuncId,
}

pub(crate) fn declare(module: &mut Module, fn_table: &mut HashMap<String, FuncId>) -> JsonMiscIds {
    let s_pp = &[Type::Str, Type::Ptr][..];
    let s_pi = &[Type::Str, Type::Ptr, Type::I64][..];
    let ptr1 = &[Type::Ptr][..];
    JsonMiscIds {
        json_quote_str: declare_intrinsic(
            module,
            fn_table,
            "__torajs_json_quote_str",
            &[Type::Str],
            Type::Str,
        ),
        json_quote_str_top: declare_intrinsic(
            module,
            fn_table,
            "__torajs_json_quote_str_top",
            &[Type::Str],
            Type::Str,
        ),
        jsb_new: declare_intrinsic(
            module,
            fn_table,
            "__torajs_jsb_new",
            &[Type::I64],
            Type::Ptr,
        ),
        jsb_push_byte: declare_intrinsic(
            module,
            fn_table,
            "__torajs_jsb_push_byte",
            &[Type::Ptr, Type::I64],
            Type::Void,
        ),
        jsb_push_str_raw: declare_intrinsic(
            module,
            fn_table,
            "__torajs_jsb_push_str_raw",
            &[Type::Ptr, Type::Str],
            Type::Void,
        ),
        jsb_push_str_quoted: declare_intrinsic(
            module,
            fn_table,
            "__torajs_jsb_push_str_quoted",
            &[Type::Ptr, Type::Str],
            Type::Void,
        ),
        jsb_push_i64: declare_intrinsic(
            module,
            fn_table,
            "__torajs_jsb_push_i64",
            &[Type::Ptr, Type::I64],
            Type::Void,
        ),
        jsb_push_bool: declare_intrinsic(
            module,
            fn_table,
            "__torajs_jsb_push_bool",
            &[Type::Ptr, Type::Bool],
            Type::Void,
        ),
        jsb_finalize: declare_intrinsic(module, fn_table, "__torajs_jsb_finalize", ptr1, Type::Str),
        // Chunk 658 — runtime comma protocol + Str-field key skip
        // (§25.5.2.4 step 8.b: undefined fields don't emit a key).
        jsb_begin_field: declare_intrinsic(
            module,
            fn_table,
            "__torajs_jsb_begin_field",
            ptr1,
            Type::Void,
        ),
        jsb_push_field_str: declare_intrinsic(
            module,
            fn_table,
            "__torajs_jsb_push_field_str",
            &[Type::Ptr, Type::Str, Type::Str],
            Type::Void,
        ),
        json_obj_sep: declare_intrinsic(
            module,
            fn_table,
            "__torajs_json_obj_sep",
            &[Type::Str],
            Type::Str,
        ),
        json_indent: declare_intrinsic(
            module,
            fn_table,
            "__torajs_json_indent",
            &[Type::Str, Type::I64],
            Type::Str,
        ),
        json_colon: declare_intrinsic(
            module,
            fn_table,
            "__torajs_json_colon",
            &[Type::Str],
            Type::Str,
        ),
        json_close_indent: declare_intrinsic(
            module,
            fn_table,
            "__torajs_json_close_indent",
            &[Type::Str, Type::Str, Type::I64],
            Type::Str,
        ),
        json_eat_char: declare_intrinsic(
            module,
            fn_table,
            "__torajs_json_eat_char",
            s_pi,
            Type::Void,
        ),
        json_parse_int: declare_intrinsic(
            module,
            fn_table,
            "__torajs_json_parse_int",
            s_pp,
            Type::I64,
        ),
        json_parse_float: declare_intrinsic(
            module,
            fn_table,
            "__torajs_json_parse_float",
            s_pp,
            Type::F64,
        ),
        json_parse_bool: declare_intrinsic(
            module,
            fn_table,
            "__torajs_json_parse_bool",
            s_pp,
            Type::I64,
        ),
        json_parse_string: declare_intrinsic(
            module,
            fn_table,
            "__torajs_json_parse_string",
            s_pp,
            Type::Str,
        ),
        json_arr_step: declare_intrinsic(
            module,
            fn_table,
            "__torajs_json_arr_step",
            s_pi,
            Type::I64,
        ),
        json_arr_first: declare_intrinsic(
            module,
            fn_table,
            "__torajs_json_arr_first",
            s_pi,
            Type::I64,
        ),
        str_eq_cstr: declare_intrinsic(module, fn_table, "__torajs_str_eq_cstr", s_pi, Type::I64),
        arr_flat: declare_intrinsic(module, fn_table, "__torajs_arr_flat", ptr1, Type::Ptr),
        arr_flat_any: declare_intrinsic(module, fn_table, "__torajs_arr_flat_any", ptr1, Type::Ptr),
        arr_extend_typed_into_any: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_extend_typed_into_any",
            &[Type::Ptr, Type::Ptr, Type::I64],
            Type::Ptr,
        ),
        arr_concat: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_concat",
            &[Type::Ptr, Type::Ptr],
            Type::Ptr,
        ),
        arr_reverse: declare_intrinsic(module, fn_table, "__torajs_arr_reverse", ptr1, Type::Ptr),
        arr_fill: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_fill",
            &[Type::Ptr, Type::I64, Type::I64, Type::I64],
            Type::Ptr,
        ),
        arr_copy_within: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_copy_within",
            &[Type::Ptr, Type::I64, Type::I64, Type::I64],
            Type::Ptr,
        ),
    }
}
