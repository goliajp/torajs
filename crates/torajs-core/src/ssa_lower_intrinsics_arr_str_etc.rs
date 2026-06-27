//! Pass 0 `declare_intrinsic` group: Substr concat + view-of-string
//! ops + Array length mutate + Array `toReversed`/`with`/`join` +
//! number↔string coerce + typed Array.console.log + substr_print +
//! str_char_at + typed Array.join variants.
//!
//! chunk 130 continuation of the Pass 0 multi-sub-sibling split
//! (chunks 110-129). 27 declarations covering the contiguous block
//! between the `ssa_lower_substr_trim_into::declare_all` sibling
//! call and the `symbol_to_str` declaration (the next inline group).
//!
//! Subgroups (source order):
//! - **View-aware concat** — one alloc + two memcpys, no intermediate
//!   materialize. Variants for each Substr-on-side combination:
//!   `substr_concat_substr_str`, `_str_substr`, `_substr_substr`.
//!   `substr_to_owned(s) -> Str` materializes a fresh owned Str.
//! - **Array<Str> from string** + **string-side substring/substr**:
//!   `arr_from_string`, `str_substring`, `str_substr`.
//! - **Array length mutate** (ES §10.4.2.5 step 4): `_validate`
//!   (RangeError) + `_truncate_scalar` (combines validation with the
//!   `len` write for non-refcounted `Array<I64|F64|Bool>`;
//!   truncate-with-rc_dec follow-up).
//! - **Array immutable mutators**: `arr_to_reversed`,
//!   `arr_with(arr, idx, val)`.
//! - **Array.prototype.join** (Str element + view-aware Substr
//!   element variant).
//! - **Number→String coerce** for `+` mixed-type concat (separate
//!   `i64_to_str` / `f64_to_str` to preserve SSA-level type
//!   distinction at the call boundary).
//! - **String→Number ToNumber** (V3-18 m1.h.9, spec §7.1.4) — NaN on
//!   parse failure; caller narrows to i64 if appropriate.
//! - **Typed Array.console.log** pretty-print (V3-18 m1.h.12) — one
//!   helper per element type (`i64`/`f64`/`bool`/`str`/`substr`).
//!   Format: `[]` for empty, `[ a, b, c ]` for non-empty (bun-match
//!   spaces).
//! - **substr_print** for solo Substr console.log.
//! - **str_char_at** — returns a Substr view (`charAt` per ES
//!   §22.1.3.1; uses Substr to avoid the alloc that a Str-returning
//!   variant would force).
//! - **Typed Array.join variants**: `arr_join_{i64,f64,i64_locale,
//!   f64_locale,bool,any}` (per ES §22.1.3.32 — `toLocaleString` arm
//!   routes through `_locale` variants which format each element via
//!   `Number.prototype.toLocaleString`, en-US `,`-grouping default;
//!   plain `join` / `toString` stays on ToString helpers).

use std::collections::HashMap;

use crate::ssa::{FuncId, Module, Type};
use crate::ssa_lower::declare_intrinsic;

pub(crate) struct ArrStrEtcIds {
    pub substr_concat_substr_str: FuncId,
    pub substr_concat_str_substr: FuncId,
    pub substr_concat_substr_substr: FuncId,
    pub substr_to_owned: FuncId,
    pub arr_from_string: FuncId,
    pub str_substring: FuncId,
    pub str_substr: FuncId,
    pub arr_set_length_validate: FuncId,
    pub arr_set_length_truncate_scalar: FuncId,
    pub arr_to_reversed: FuncId,
    pub arr_with: FuncId,
    pub arr_join: FuncId,
    pub arr_join_substr: FuncId,
    pub i64_to_str: FuncId,
    pub f64_to_str: FuncId,
    pub str_to_number: FuncId,
    pub arr_print_i64: FuncId,
    pub arr_print_f64: FuncId,
    pub arr_print_bool: FuncId,
    pub arr_print_str: FuncId,
    pub arr_print_substr: FuncId,
    pub substr_print: FuncId,
    pub str_char_at: FuncId,
    pub arr_join_i64: FuncId,
    pub arr_join_f64: FuncId,
    pub arr_join_i64_locale: FuncId,
    pub arr_join_f64_locale: FuncId,
    pub arr_join_bool: FuncId,
    pub arr_join_any: FuncId,
}

pub(crate) fn declare(module: &mut Module, fn_table: &mut HashMap<String, FuncId>) -> ArrStrEtcIds {
    let ptr_str = &[Type::Ptr, Type::Str][..];
    let ptr_ptr = &[Type::Ptr, Type::Ptr][..];
    let ptr1 = &[Type::Ptr][..];
    let str_ii = &[Type::Str, Type::I64, Type::I64][..];
    ArrStrEtcIds {
        substr_concat_substr_str: declare_intrinsic(
            module,
            fn_table,
            "__torajs_substr_concat_substr_str",
            ptr_str,
            Type::Str,
        ),
        substr_concat_str_substr: declare_intrinsic(
            module,
            fn_table,
            "__torajs_substr_concat_str_substr",
            &[Type::Str, Type::Ptr],
            Type::Str,
        ),
        substr_concat_substr_substr: declare_intrinsic(
            module,
            fn_table,
            "__torajs_substr_concat_substr_substr",
            ptr_ptr,
            Type::Str,
        ),
        substr_to_owned: declare_intrinsic(
            module,
            fn_table,
            "__torajs_substr_to_owned",
            &[Type::Substr],
            Type::Str,
        ),
        arr_from_string: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_from_string",
            &[Type::Str],
            Type::Ptr,
        ),
        str_substring: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_substring",
            str_ii,
            Type::Str,
        ),
        str_substr: declare_intrinsic(module, fn_table, "__torajs_str_substr", str_ii, Type::Str),
        arr_set_length_validate: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_set_length_validate",
            &[Type::I64, Type::I64],
            Type::Void,
        ),
        arr_set_length_truncate_scalar: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_set_length_truncate_scalar",
            &[Type::Ptr, Type::I64, Type::I64],
            Type::Void,
        ),
        arr_to_reversed: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_to_reversed",
            ptr1,
            Type::Ptr,
        ),
        arr_with: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_with",
            &[Type::Ptr, Type::I64, Type::I64],
            Type::Ptr,
        ),
        arr_join: declare_intrinsic(module, fn_table, "__torajs_arr_join", ptr_str, Type::Str),
        arr_join_substr: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_join_substr",
            ptr_str,
            Type::Str,
        ),
        i64_to_str: declare_intrinsic(
            module,
            fn_table,
            "__torajs_i64_to_str",
            &[Type::I64],
            Type::Str,
        ),
        f64_to_str: declare_intrinsic(
            module,
            fn_table,
            "__torajs_f64_to_str",
            &[Type::F64],
            Type::Str,
        ),
        str_to_number: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_to_number",
            &[Type::Str],
            Type::F64,
        ),
        arr_print_i64: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_print_i64",
            ptr1,
            Type::Void,
        ),
        arr_print_f64: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_print_f64",
            ptr1,
            Type::Void,
        ),
        arr_print_bool: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_print_bool",
            ptr1,
            Type::Void,
        ),
        arr_print_str: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_print_str",
            ptr1,
            Type::Void,
        ),
        arr_print_substr: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_print_substr",
            ptr1,
            Type::Void,
        ),
        substr_print: declare_intrinsic(
            module,
            fn_table,
            "__torajs_substr_print",
            ptr1,
            Type::Void,
        ),
        str_char_at: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_char_at",
            &[Type::Ptr, Type::I64],
            Type::Substr,
        ),
        arr_join_i64: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_join_i64",
            ptr_ptr,
            Type::Str,
        ),
        arr_join_f64: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_join_f64",
            ptr_ptr,
            Type::Str,
        ),
        arr_join_i64_locale: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_join_i64_locale",
            ptr_ptr,
            Type::Str,
        ),
        arr_join_f64_locale: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_join_f64_locale",
            ptr_ptr,
            Type::Str,
        ),
        arr_join_bool: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_join_bool",
            ptr_ptr,
            Type::Str,
        ),
        arr_join_any: declare_intrinsic(
            module,
            fn_table,
            "__torajs_arr_join_any",
            ptr_ptr,
            Type::Str,
        ),
    }
}
