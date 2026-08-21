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

/// Mach-O symbol of the immortal Str `undefined` sentinel cell
/// (`#[unsafe(no_mangle)] pub static __TORAJS_STR_UNDEF_CELL` in
/// torajs-str `undef_sentinel.rs`; the leading `_` is the Mach-O
/// C-symbol prefix, matching the archive nlist key verbatim).
/// Emit sites materialize the sentinel address with
/// `InstKind::GlobalRef(...)` — ADRP+ADD, zero calls — resolved by
/// the linker's member defined-extern table, which indexes DATA
/// symbols alongside text ones.
pub(crate) const STR_UNDEF_CELL_SYM: &str = "___TORAJS_STR_UNDEF_CELL";

pub(crate) struct StrBIds {
    pub str_slice: FuncId,
    pub str_char_code_at: FuncId,
    pub str_code_point_at: FuncId,
    pub str_starts_with: FuncId,
    pub str_ends_with: FuncId,
    pub str_index_of: FuncId,
    pub str_last_index_of: FuncId,
    pub str_locale_compare: FuncId,
    pub str_sort_cmp: FuncId,
    pub str_sort_undef_pre: FuncId,
    pub str_includes: FuncId,
    pub str_eq: FuncId,
    /// RFC 20260716 刀 13 — typed-Str `.hasOwnProperty(key)` per
    /// ES §22.1.4 String Exotic Object. Answers 1 for `"length"`
    /// or a canonical index `[0, len)`, 0 otherwise.
    pub str_prop_has: FuncId,
    /// RFC 20260716 刀 20 — typed-Str `.propertyIsEnumerable(key)`
    /// per ES §22.1.4 String Exotic Object. Answers 1 ONLY for a
    /// canonical index `[0, len)`; `"length"` is spec non-enumerable
    /// per §22.1.5.1 so this returns 0 for it (unlike `str_prop_has`).
    pub str_prop_enumerable: FuncId,
    pub str_null_check: FuncId,
    pub str_is_nullish: FuncId,
    pub str_is_undef: FuncId,
    pub is_undef_cell: FuncId,
    /// RFC 20260722-find-miss chunk B — nullish guard for pointer-
    /// shaped heap slots (torajs-rc null_guard.rs): arms a catchable
    /// TypeError on NULL or the generic undefined cell before a
    /// member read dereferences a find/findLast miss.
    pub heap_nullish_check: FuncId,
    pub str_split: FuncId,
    pub str_split_no_sep: FuncId,
    pub str_split_any_sep: FuncId,
    /// Rotation 264 — the limit-carrying twin for the `@@split`
    /// probe-miss lane (limit rides RAW; the kernel runs step 4's
    /// ToUint32 itself). Returns the result as an AnyValue box.
    pub str_split_any_sep_lim: FuncId,
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
            Type::F64,
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
        // ES §23.1.3.30.2 SortCompare (default-comparator sort lane):
        // undefined (either sentinel repr) sorts last before ToString;
        // NULL participates as "null".
        str_sort_cmp: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_sort_cmp",
            &[Type::Str, Type::Str],
            Type::I64,
        ),
        // §23.1.3.30.2 steps 5-8 pre-probe (user-comparator lane):
        // 1/-1/0 = SortCompare answer, 2 = call the comparator.
        str_sort_undef_pre: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_sort_undef_pre",
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
        str_prop_has: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_prop_has",
            &[Type::Str, Type::Str],
            Type::I64,
        ),
        str_prop_enumerable: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_prop_enumerable",
            &[Type::Str, Type::Str],
            Type::I64,
        ),
        str_null_check: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_null_check",
            &[Type::Ptr],
            Type::Void,
        ),
        str_is_nullish: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_is_nullish",
            &[Type::Str],
            Type::Bool,
        ),
        // Undefined-ONLY probe (either sentinel repr; NULL answers
        // false) — the JSON object key-skip lane (§25.5.2.4 8.b).
        str_is_undef: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_is_undef",
            &[Type::Str],
            Type::Bool,
        ),
        // RFC 20260710 C2b — generic Tag::Undefined oddball identity
        // probe (torajs-rc undef_cell.rs): tells "JS undefined" from
        // a live heap cell on a refcounted pointer slot (Obj / Arr /
        // Closure). Sibling of str_is_undef; lives here with the
        // sentinel family.
        is_undef_cell: declare_intrinsic(
            module,
            fn_table,
            "__torajs_is_undef_cell",
            &[Type::Ptr],
            Type::Bool,
        ),
        heap_nullish_check: declare_intrinsic(
            module,
            fn_table,
            "__torajs_heap_nullish_check",
            &[Type::Ptr],
            Type::Void,
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
        str_split_any_sep: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_split_any_sep",
            &[Type::Str, Type::Any],
            Type::Ptr,
        ),
        str_split_any_sep_lim: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_split_any_sep_lim",
            &[Type::Str, Type::Any, Type::Any],
            Type::Any,
        ),
    }
}
