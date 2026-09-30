//! Pass 0 `declare_intrinsic` group: Number method runtime.
//!
//! chunk 114 continuation of the Pass 0 multi-sub-sibling split
//! (chunks 110-113). 24 declarations covering Number's stringify /
//! parse / classification surface, with `_f` / `_i` width-specialized
//! pairs (and S341 `_any` tag-dispatched helpers for the four
//! `Number.is*` predicates).
//!
//! - `num_to_fixed_{f,i}(v, digits) -> Str` — `(123).toFixed(2)`.
//! - `num_to_string_radix_{i,f}(v, radix) -> Str` —
//!   `(255).toString(16)`.
//! - `num_to_exp_{f,i}(v, fraction_digits) -> Str` — `.toExponential`.
//! - `num_to_precision_{f,i}(v, precision) -> Str` — `.toPrecision`.
//! - `num_to_locale_{f,i}(v) -> Str` — `.toLocaleString` (default
//!   locale only).
//! - `num_parse_int(s, radix) -> F64` / `num_parse_float(s) -> F64`.
//! - `num_is_integer_{f,i,any}` / `_is_nan_{f,i,any}` /
//!   `_is_finite_{f,i,any}` / `_is_safe_integer_{f,i,any}` (S341 —
//!   the `_any` flavour takes `(tag: i64, val: i64)` so callers can
//!   feed `(any_unbox_tag, any_unbox_value)` straight in).

use std::collections::HashMap;

use crate::ssa::{FuncId, Module, Type};
use crate::ssa_lower::declare_intrinsic;

pub(crate) struct NumIds {
    pub num_to_fixed_f: FuncId,
    pub num_to_fixed_i: FuncId,
    pub num_to_string_radix_i: FuncId,
    pub num_to_string_radix_f: FuncId,
    pub num_to_exp_f: FuncId,
    pub num_to_exp_i: FuncId,
    pub num_to_precision_f: FuncId,
    pub num_to_precision_i: FuncId,
    pub num_to_locale_f: FuncId,
    pub num_to_locale_i: FuncId,
    /// §7.1.22 `ToIndex` — the range test an index-like parameter
    /// runs over its Number before it becomes an index.
    pub num_to_index: FuncId,
    pub num_parse_int: FuncId,
    pub num_parse_float: FuncId,
    pub num_is_integer_f: FuncId,
    pub num_is_integer_i: FuncId,
    pub num_is_nan_f: FuncId,
    pub num_is_nan_i: FuncId,
    pub num_is_finite_f: FuncId,
    pub num_is_finite_i: FuncId,
    pub num_is_safe_integer_f: FuncId,
    pub num_is_safe_integer_i: FuncId,
    pub num_is_integer_any: FuncId,
    pub num_is_nan_any: FuncId,
    pub num_is_finite_any: FuncId,
    pub num_is_safe_integer_any: FuncId,
}

pub(crate) fn declare(module: &mut Module, fn_table: &mut HashMap<String, FuncId>) -> NumIds {
    NumIds {
        num_to_fixed_f: declare_intrinsic(
            module,
            fn_table,
            "__torajs_num_to_fixed_f",
            &[Type::F64, Type::I64],
            Type::Str,
        ),
        num_to_fixed_i: declare_intrinsic(
            module,
            fn_table,
            "__torajs_num_to_fixed_i",
            &[Type::I64, Type::I64],
            Type::Str,
        ),
        num_to_string_radix_i: declare_intrinsic(
            module,
            fn_table,
            "__torajs_num_to_string_radix_i",
            &[Type::I64, Type::I64],
            Type::Str,
        ),
        num_to_string_radix_f: declare_intrinsic(
            module,
            fn_table,
            "__torajs_num_to_string_radix_f",
            &[Type::F64, Type::I64],
            Type::Str,
        ),
        num_to_exp_f: declare_intrinsic(
            module,
            fn_table,
            "__torajs_num_to_exp_f",
            &[Type::F64, Type::I64],
            Type::Str,
        ),
        num_to_exp_i: declare_intrinsic(
            module,
            fn_table,
            "__torajs_num_to_exp_i",
            &[Type::I64, Type::I64],
            Type::Str,
        ),
        num_to_precision_f: declare_intrinsic(
            module,
            fn_table,
            "__torajs_num_to_precision_f",
            &[Type::F64, Type::I64],
            Type::Str,
        ),
        num_to_precision_i: declare_intrinsic(
            module,
            fn_table,
            "__torajs_num_to_precision_i",
            &[Type::I64, Type::I64],
            Type::Str,
        ),
        num_to_locale_f: declare_intrinsic(
            module,
            fn_table,
            "__torajs_num_to_locale_f",
            &[Type::F64],
            Type::Str,
        ),
        num_to_locale_i: declare_intrinsic(
            module,
            fn_table,
            "__torajs_num_to_locale_i",
            &[Type::I64],
            Type::Str,
        ),
        num_to_index: declare_intrinsic(
            module,
            fn_table,
            "__torajs_to_index",
            &[Type::F64],
            Type::I64,
        ),
        num_parse_int: declare_intrinsic(
            module,
            fn_table,
            "__torajs_num_parse_int",
            &[Type::Str, Type::I64],
            Type::F64,
        ),
        num_parse_float: declare_intrinsic(
            module,
            fn_table,
            "__torajs_num_parse_float",
            &[Type::Str],
            Type::F64,
        ),
        num_is_integer_f: declare_intrinsic(
            module,
            fn_table,
            "__torajs_num_is_integer_f",
            &[Type::F64],
            Type::Bool,
        ),
        num_is_integer_i: declare_intrinsic(
            module,
            fn_table,
            "__torajs_num_is_integer_i",
            &[Type::I64],
            Type::Bool,
        ),
        num_is_nan_f: declare_intrinsic(
            module,
            fn_table,
            "__torajs_num_is_nan_f",
            &[Type::F64],
            Type::Bool,
        ),
        num_is_nan_i: declare_intrinsic(
            module,
            fn_table,
            "__torajs_num_is_nan_i",
            &[Type::I64],
            Type::Bool,
        ),
        num_is_finite_f: declare_intrinsic(
            module,
            fn_table,
            "__torajs_num_is_finite_f",
            &[Type::F64],
            Type::Bool,
        ),
        num_is_finite_i: declare_intrinsic(
            module,
            fn_table,
            "__torajs_num_is_finite_i",
            &[Type::I64],
            Type::Bool,
        ),
        num_is_safe_integer_f: declare_intrinsic(
            module,
            fn_table,
            "__torajs_num_is_safe_integer_f",
            &[Type::F64],
            Type::Bool,
        ),
        num_is_safe_integer_i: declare_intrinsic(
            module,
            fn_table,
            "__torajs_num_is_safe_integer_i",
            &[Type::I64],
            Type::Bool,
        ),
        num_is_integer_any: declare_intrinsic(
            module,
            fn_table,
            "__torajs_num_is_integer_any",
            &[Type::I64, Type::I64],
            Type::Bool,
        ),
        num_is_nan_any: declare_intrinsic(
            module,
            fn_table,
            "__torajs_num_is_nan_any",
            &[Type::I64, Type::I64],
            Type::Bool,
        ),
        num_is_finite_any: declare_intrinsic(
            module,
            fn_table,
            "__torajs_num_is_finite_any",
            &[Type::I64, Type::I64],
            Type::Bool,
        ),
        num_is_safe_integer_any: declare_intrinsic(
            module,
            fn_table,
            "__torajs_num_is_safe_integer_any",
            &[Type::I64, Type::I64],
            Type::Bool,
        ),
    }
}
