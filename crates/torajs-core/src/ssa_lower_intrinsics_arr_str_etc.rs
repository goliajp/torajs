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
    pub arr_set_length_any: FuncId,
    pub arr_to_reversed: FuncId,
    pub arr_with: FuncId,
    pub arr_flat_runtime_depth: FuncId,
    pub arr_oob_throw: FuncId,
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
    pub substr_print_inline: FuncId,
    pub str_char_at: FuncId,
    pub arr_join_i64: FuncId,
    pub arr_join_f64: FuncId,
    pub arr_join_i64_locale: FuncId,
    pub arr_join_f64_locale: FuncId,
    pub arr_join_bool: FuncId,
    pub arr_join_any: FuncId,
}

pub(crate) fn declare(module: &mut Module, fn_table: &mut HashMap<String, FuncId>) -> ArrStrEtcIds {
    // defined inside the fn so `module` / `fn_table` resolve at the
    // macro definition site (macro_rules locals are hygienic) — the
    // chunk 457 intrinsics_object convergence pattern
    macro_rules! decl {
        ($name:literal, [$($param:ident),*], $ret:ident) => {
            declare_intrinsic(module, fn_table, $name, &[$(Type::$param),*], Type::$ret)
        };
    }
    ArrStrEtcIds {
        substr_concat_substr_str: decl!("__torajs_substr_concat_substr_str", [Ptr, Str], Str),
        substr_concat_str_substr: decl!("__torajs_substr_concat_str_substr", [Str, Ptr], Str),
        substr_concat_substr_substr: decl!("__torajs_substr_concat_substr_substr", [Ptr, Ptr], Str),
        substr_to_owned: decl!("__torajs_substr_to_owned", [Substr], Str),
        arr_from_string: decl!("__torajs_arr_from_string", [Str], Ptr),
        str_substring: decl!("__torajs_str_substring", [Str, I64, I64], Str),
        str_substr: decl!("__torajs_str_substr", [Str, I64, I64], Str),
        arr_set_length_validate: decl!("__torajs_arr_set_length_validate", [I64, I64], Void),
        arr_set_length_any: decl!("__torajs_arr_set_length_any", [Ptr, I64, I64], Void),
        arr_set_length_truncate_scalar: decl!(
            "__torajs_arr_set_length_truncate_scalar",
            [Ptr, I64, I64],
            Void
        ),
        arr_to_reversed: decl!("__torajs_arr_to_reversed", [Ptr], Ptr),
        arr_with: decl!("__torajs_arr_with", [Ptr, I64, I64], Ptr),
        arr_flat_runtime_depth: decl!("__torajs_arr_flat_runtime_depth", [Ptr, Any], Ptr),
        arr_oob_throw: decl!("__torajs_arr_oob_throw", [], Void),
        arr_join: decl!("__torajs_arr_join", [Ptr, Str], Str),
        arr_join_substr: decl!("__torajs_arr_join_substr", [Ptr, Str], Str),
        i64_to_str: decl!("__torajs_i64_to_str", [I64], Str),
        f64_to_str: decl!("__torajs_f64_to_str", [F64], Str),
        str_to_number: decl!("__torajs_str_to_number", [Str], F64),
        arr_print_i64: decl!("__torajs_arr_print_i64", [Ptr], Void),
        arr_print_f64: decl!("__torajs_arr_print_f64", [Ptr], Void),
        arr_print_bool: decl!("__torajs_arr_print_bool", [Ptr], Void),
        arr_print_str: decl!("__torajs_arr_print_str", [Ptr], Void),
        arr_print_substr: decl!("__torajs_arr_print_substr", [Ptr], Void),
        substr_print: decl!("__torajs_substr_print", [Ptr], Void),
        substr_print_inline: decl!("__torajs_substr_print_inline", [Ptr], Void),
        str_char_at: decl!("__torajs_str_char_at", [Ptr, I64], Substr),
        arr_join_i64: decl!("__torajs_arr_join_i64", [Ptr, Ptr], Str),
        arr_join_f64: decl!("__torajs_arr_join_f64", [Ptr, Ptr], Str),
        arr_join_i64_locale: decl!("__torajs_arr_join_i64_locale", [Ptr, Ptr], Str),
        arr_join_f64_locale: decl!("__torajs_arr_join_f64_locale", [Ptr, Ptr], Str),
        arr_join_bool: decl!("__torajs_arr_join_bool", [Ptr, Ptr], Str),
        arr_join_any: decl!("__torajs_arr_join_any", [Ptr, Ptr], Str),
    }
}
