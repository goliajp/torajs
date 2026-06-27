//! Pass 0 `declare_intrinsic` group: StrRepr method runtime — group B
//! (M6.1 slice / code-unit / lookup + split, contiguous source block
//! just after the num intrinsics).
//!
//! 12 declarations:
//! - `str_slice(s, start, end) -> Str` — fresh heap StrRepr.
//! - `str_char_code_at(s, idx) -> i64` (UTF-16 code unit, zext'd).
//! - `str_code_point_at(s, idx) -> i64` (surrogate-pair aware).
//! - `str_starts_with` / `str_ends_with` / `str_includes` -> Bool.
//! - `str_index_of` / `str_last_index_of` -> i64 (-1 = not found).
//! - `str_locale_compare(a, b) -> i64` (default locale only).
//! - `str_eq(a, b) -> Bool` — spec-correct `===` / `!==` on strings
//!   per ECMA-262 §7.2.16 (content-equal, not pointer-equal). Without
//!   this `"a" === "a"` could be false depending on whether the
//!   literals shared a pool.
//! - `str_split(s, sep) -> Ptr` / `str_split_no_sep(s) -> Ptr` —
//!   ES §22.1.3.21. The no-sep flavour exists because the 2-arg
//!   `str_split` silently missed the `sep` slot and read register
//!   garbage when callers omitted the argument, surviving single-call
//!   programs but SIGSEGV'ing after any prior `.split(arg)` shifted
//!   residual register state. The split output array's element type
//!   is interned lazily — we don't intern `Type::Arr(Str)` up-front
//!   because the `arr_layouts` interner is keyed by element Type and
//!   ordering must stay deterministic across compilation runs.
//!
//! chunk 115 continuation of the Pass 0 multi-sub-sibling split
//! (chunks 110-114).

use std::collections::HashMap;

use crate::ssa::{FuncId, Module, Type};
use crate::ssa_lower::declare_intrinsic;

pub(crate) struct StrBIds {
    pub str_slice: FuncId,
    pub str_char_code_at: FuncId,
    pub str_code_point_at: FuncId,
    pub str_starts_with: FuncId,
    pub str_ends_with: FuncId,
    pub str_index_of: FuncId,
    pub str_last_index_of: FuncId,
    pub str_locale_compare: FuncId,
    pub str_includes: FuncId,
    pub str_eq: FuncId,
    pub str_split: FuncId,
    pub str_split_no_sep: FuncId,
}

pub(crate) fn declare(module: &mut Module, fn_table: &mut HashMap<String, FuncId>) -> StrBIds {
    StrBIds {
        str_slice: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_slice",
            &[Type::Str, Type::I64, Type::I64],
            Type::Str,
        ),
        str_char_code_at: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_char_code_at",
            &[Type::Str, Type::I64],
            Type::I64,
        ),
        str_code_point_at: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_code_point_at",
            &[Type::Str, Type::I64],
            Type::I64,
        ),
        str_starts_with: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_starts_with",
            &[Type::Str, Type::Str],
            Type::Bool,
        ),
        str_ends_with: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_ends_with",
            &[Type::Str, Type::Str],
            Type::Bool,
        ),
        str_index_of: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_index_of",
            &[Type::Str, Type::Str],
            Type::I64,
        ),
        str_last_index_of: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_last_index_of",
            &[Type::Str, Type::Str],
            Type::I64,
        ),
        str_locale_compare: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_locale_compare",
            &[Type::Str, Type::Str],
            Type::I64,
        ),
        str_includes: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_includes",
            &[Type::Str, Type::Str],
            Type::Bool,
        ),
        str_eq: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_eq",
            &[Type::Str, Type::Str],
            Type::Bool,
        ),
        str_split: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_split",
            &[Type::Str, Type::Str],
            Type::Ptr,
        ),
        str_split_no_sep: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_split_no_sep",
            &[Type::Str],
            Type::Ptr,
        ),
    }
}
