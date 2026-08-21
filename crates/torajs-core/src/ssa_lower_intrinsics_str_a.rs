//! Pass 0 `declare_intrinsic` group: StrRepr method runtime — group A
//! (alloc/transform/lookup, contiguous source block just after the
//! arr intrinsics, before num).
//!
//! 14 declarations:
//! - Repeat / case: `str_repeat(s, n)`, `str_to_upper(s)`,
//!   `str_to_lower(s)`.
//! - Trim: `str_trim`, `str_trim_start`, `str_trim_end` (whitespace
//!   per ES §22.1.3.32, §22.1.3.33).
//! - Pad: `str_pad_start(s, target_len, pad_str)`, `str_pad_end(...)`.
//! - From code: `str_from_char_code(cu)` (UTF-16 code unit) /
//!   `str_from_code_point(cp)` (Unicode code point, surrogate-pair
//!   aware).
//! - Normalize: `str_normalize(s, form)` — form is one of "NFC" /
//!   "NFD" / "NFKC" / "NFKD" (validated runtime-side).
//! - At: `str_at(s, idx)` — UTF-16 code-unit index, negative-from-end
//!   per ES §22.1.3.1.
//! - Replace: `str_replace(s, pat, repl)` — first occurrence only;
//!   `str_replace_all` — every occurrence (string pattern only; regex
//!   patterns go through the regex sibling).
//!
//! chunk 113 continuation of the Pass 0 multi-sub-sibling split
//! (chunks 110/111/112).

use std::collections::HashMap;

use crate::ssa::{FuncId, Module, Type};
use crate::ssa_lower::declare_intrinsic;

pub(crate) struct StrAIds {
    pub str_repeat: FuncId,
    pub str_to_upper: FuncId,
    pub str_to_lower: FuncId,
    pub str_trim: FuncId,
    pub str_trim_start: FuncId,
    pub str_trim_end: FuncId,
    pub str_pad_start: FuncId,
    pub str_pad_end: FuncId,
    pub str_from_char_code: FuncId,
    /// §22.1.2.1's `ToUint16` over the Number itself — ±∞ and NaN
    /// map to +0, which an i64 parameter cannot express.
    pub str_from_char_code_f64: FuncId,
    pub str_from_code_point: FuncId,
    /// §22.1.2.2's step over the Number itself — rejects a
    /// non-integral code point, which the i64 sibling cannot see.
    pub str_from_code_point_f64: FuncId,
    pub string_raw: FuncId,
    pub str_normalize: FuncId,
    pub str_to_locale_upper: FuncId,
    pub str_to_locale_lower: FuncId,
    pub str_locale_case_arr: FuncId,
    pub str_at: FuncId,
    pub str_replace: FuncId,
    pub str_replace_all: FuncId,
    pub str_replace_fn: FuncId,
    pub str_replace_all_fn: FuncId,
}

pub(crate) fn declare(module: &mut Module, fn_table: &mut HashMap<String, FuncId>) -> StrAIds {
    StrAIds {
        str_repeat: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_repeat",
            &[Type::Str, Type::I64],
            Type::Str,
        ),
        str_to_upper: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_to_upper",
            &[Type::Str],
            Type::Str,
        ),
        str_to_lower: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_to_lower",
            &[Type::Str],
            Type::Str,
        ),
        str_trim: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_trim",
            &[Type::Str],
            Type::Str,
        ),
        str_trim_start: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_trim_start",
            &[Type::Str],
            Type::Str,
        ),
        str_trim_end: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_trim_end",
            &[Type::Str],
            Type::Str,
        ),
        str_pad_start: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_pad_start",
            &[Type::Str, Type::I64, Type::Str],
            Type::Str,
        ),
        str_pad_end: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_pad_end",
            &[Type::Str, Type::I64, Type::Str],
            Type::Str,
        ),
        str_from_char_code: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_from_char_code",
            &[Type::I64],
            Type::Str,
        ),
        str_from_char_code_f64: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_from_char_code_f64",
            &[Type::F64],
            Type::Str,
        ),
        str_from_code_point: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_from_code_point",
            &[Type::I64],
            Type::Str,
        ),
        str_from_code_point_f64: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_from_code_point_f64",
            &[Type::F64],
            Type::Str,
        ),
        string_raw: declare_intrinsic(
            module,
            fn_table,
            "__torajs_string_raw",
            &[Type::Any, Type::Ptr, Type::I64],
            Type::Str,
        ),
        str_normalize: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_normalize",
            &[Type::Str, Type::Str],
            Type::Str,
        ),
        str_to_locale_upper: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_to_locale_upper",
            &[Type::Str, Type::Str],
            Type::Str,
        ),
        str_to_locale_lower: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_to_locale_lower",
            &[Type::Str, Type::Str],
            Type::Str,
        ),
        str_locale_case_arr: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_locale_case_arr",
            &[Type::Str, Type::Ptr, Type::I64],
            Type::Str,
        ),
        str_at: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_at",
            &[Type::Str, Type::I64],
            Type::Str,
        ),
        str_replace: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_replace",
            &[Type::Str, Type::Str, Type::Str],
            Type::Str,
        ),
        str_replace_all: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_replace_all",
            &[Type::Str, Type::Str, Type::Str],
            Type::Str,
        ),
        str_replace_fn: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_replace_fn",
            &[Type::Str, Type::Str, Type::Ptr],
            Type::Str,
        ),
        str_replace_all_fn: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_replace_all_fn",
            &[Type::Str, Type::Str, Type::Ptr],
            Type::Str,
        ),
    }
}
