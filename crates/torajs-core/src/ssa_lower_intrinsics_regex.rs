//! Pass 0 `declare_intrinsic` group: regex runtime (v0.2 #1 + Phase 1b/1c/2).
//!
//! chunk 116 continuation of the Pass 0 multi-sub-sibling split
//! (chunks 110-115). 18 declarations covering compile / test /
//! drop / surface methods / accessors / lastIndex / static-DFA bake.
//!
//! Surface contract (mirrors the per-decl ABI comments that
//! previously lived inline in lower_inner):
//! - `regex_compile(pattern, flags) -> RegExp` — backtracking-NFA
//!   compiler. Defined in `runtime_regex.c`; rc_dec routes RegExp
//!   drops through the universal heap-header type-tag dispatch.
//! - `regex_compile_from_static_dfa(meta_ptr, pattern, flags) -> RegExp`
//!   — V0.2 P14 chunk 7.7 Phase C-4. Takes the `BakedDfaMeta` pointer
//!   (`.rodata`-resident, chain-LC rebased) as a 3rd arg and stamps
//!   it onto `RegExp::baked_dfa` so the surface match path
//!   short-circuits the runtime `build_dfa`.
//! - `regex_test` / `regex_drop` — predicate + lifecycle.
//! - `regex_match` / `_replace` / `_replace_all` / `_split` /
//!   `_match_all` — Phase 1b/1c surface methods on Str receivers.
//! - `regex_replace{_all}_fn` — P9.5-A1.1/A1.2 callback form. 3rd arg
//!   = closure env block (env+8 holds the lifted body's fn_addr);
//!   4th = `n_caps` (capture-group count, static at ssa-lower); 5th =
//!   `has_off_input` (0 = `(m, g1..gN)` shape, 1 = `(m, g1..gN, off,
//!   input)` shape). Runtime builds N capture Strs from saves[].
//! - `regex_exec` — `re.exec(s)` materializes `[match, g1..gN]` from
//!   the per-thread saves[] array.
//! - `regex_get_source` / `_get_flags` / `_to_string` / `_has_flag` —
//!   ES §22.2.6.4-13 accessors.
//! - `regex_get_last_index` / `_set_last_index` — P9.4 lastIndex
//!   surface; `regex_exec` + non-global `s.match(re)` consult/update
//!   the same field when the regex carries `g` or `y`.

use std::collections::HashMap;

use crate::ssa::{FuncId, Module, Type};
use crate::ssa_lower::declare_intrinsic;

pub(crate) struct RegexIds {
    /// ES2025 §22.2.5.1 RegExp.escape — strict-String any shell.
    pub regexp_escape_any: FuncId,
    pub regex_compile: FuncId,
    /// `new RegExp(pattern, flags)` entry — wraps
    /// `__torajs_regex_compile` and records a catchable `SyntaxError`
    /// on the TLS pending-throw slot when the parser rejects the
    /// pattern. `lower_regexp` emits an `emit_throw_check_owned`
    /// right after so the throw propagates as a real JS exception
    /// (spec §22.2.3.1 CompilePattern) rather than the pre-existing
    /// silent never-match stub. Literal `/pat/flags` keeps calling
    /// the plain `regex_compile` — literal-time throw needs an
    /// entry-block-safe check shape (L3b).
    pub regex_compile_or_throw: FuncId,
    /// §22.2.3.1 with boxed operands — a RegExp pattern copies
    /// source (+ flags when flags is undefined); everything else
    /// runs ToString (rotation 267; the RegExp call→construct
    /// rewrite exposed non-Str patterns).
    pub regex_compile_any: FuncId,
    /// §22.1.3.{11,12,20} step 3 for a typed-receiver call whose
    /// pattern arrived in an `any` slot — a real RegExp cell passes
    /// through borrowed, everything else takes step 4's
    /// `RegExpCreate(ToString(P), F)`. Paired with
    /// [`RegexIds::regexp_drop_if_coerced`], which releases only what
    /// it minted.
    pub regexp_from_any: FuncId,
    pub regexp_drop_if_coerced: FuncId,
    /// §22.1.3.{19,20} step 2 with the searchValue in an `any` slot —
    /// a RegExp cell rides the regex kernel, everything else is a
    /// literal search over `ToString(searchValue)`.
    pub str_replace_any_pattern: FuncId,
    /// The callback-replacer twin — `n_caps` is what the CALLBACK
    /// declares, since a pattern known only as `any` cannot be
    /// counted at compile time.
    pub str_replace_any_pattern_fn: FuncId,
    pub regex_compile_from_static_dfa: FuncId,
    pub regex_test: FuncId,
    pub regex_drop: FuncId,
    pub regex_match: FuncId,
    /// §22.1.3.13 / §22.1.3.20 step 3 custom-matcher legs over an
    /// Any-typed pattern (torajs-anyvalue `str_match_custom`): the
    /// probe answers 1 when the pattern carries a non-nullish
    /// method under the well-known symbol index operand (6 =
    /// `@@match`, 9 = `@@search`), the invoke leg runs GetMethod +
    /// Call(matcher, pattern, «S»).
    pub any_str_symbol_probe: FuncId,
    pub any_str_symbol_invoke: FuncId,
    /// Two-extra-argument twin — §22.1.3.19 `@@replace` calls
    /// «O, replaceValue» (8 = `@@replace`).
    pub any_str_symbol_invoke2: FuncId,
    pub regex_replace: FuncId,
    pub regex_replace_all: FuncId,
    pub regex_replace_fn: FuncId,
    pub regex_replace_all_fn: FuncId,
    pub regex_split: FuncId,
    pub regex_search: FuncId,
    pub regex_exec: FuncId,
    pub regex_get_source: FuncId,
    pub regex_get_flags: FuncId,
    pub regex_to_string: FuncId,
    pub regex_has_flag: FuncId,
    pub regex_match_all: FuncId,
    pub regex_get_last_index: FuncId,
    pub regex_set_last_index: FuncId,
}

// CARVE-OUT: dispatch table — 33 back-to-back `declare_intrinsic` calls
// filling a single `RegexIds` struct literal (data-driven; each entry
// declares one runtime FuncId + records it in `fn_table`). Same shape
// and same rationale as `ssa_lower_intrinsics_map_set.rs::declare` and
// `ssa_lower_intrinsics_table.rs::build`: splitting `RegexIds` into
// sub-structs would change the `intrinsics.<field>` consumer surface
// across every regex read site, and a `decl!` macro would trade the
// line count for macro hygiene at ≤ 3 lines saved per entry. Crossed
// 200 in rotation 418, when the `any`-slot pattern family added three
// entries.
pub(crate) fn declare(module: &mut Module, fn_table: &mut HashMap<String, FuncId>) -> RegexIds {
    RegexIds {
        regexp_escape_any: declare_intrinsic(
            module,
            fn_table,
            "__torajs_regexp_escape_any",
            &[Type::Any],
            Type::Any,
        ),
        regex_compile: declare_intrinsic(
            module,
            fn_table,
            "__torajs_regex_compile",
            &[Type::Str, Type::Str],
            Type::RegExp,
        ),
        regex_compile_or_throw: declare_intrinsic(
            module,
            fn_table,
            "__torajs_regex_compile_or_throw",
            &[Type::Str, Type::Str],
            Type::RegExp,
        ),
        regex_compile_any: declare_intrinsic(
            module,
            fn_table,
            "__torajs_regex_compile_any",
            &[Type::Any, Type::Any],
            Type::RegExp,
        ),
        regexp_from_any: declare_intrinsic(
            module,
            fn_table,
            "__torajs_regexp_from_any",
            &[Type::Any, Type::Str],
            Type::RegExp,
        ),
        regexp_drop_if_coerced: declare_intrinsic(
            module,
            fn_table,
            "__torajs_regexp_drop_if_coerced",
            &[Type::Any, Type::RegExp],
            Type::Void,
        ),
        str_replace_any_pattern: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_replace_any_pattern",
            &[Type::Str, Type::Any, Type::Str, Type::I64],
            Type::Str,
        ),
        str_replace_any_pattern_fn: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_replace_any_pattern_fn",
            &[
                Type::Str,
                Type::Any,
                Type::Ptr,
                Type::I64,
                Type::I64,
                Type::I64,
            ],
            Type::Str,
        ),
        regex_compile_from_static_dfa: declare_intrinsic(
            module,
            fn_table,
            "__torajs_regex_compile_from_static_dfa",
            &[Type::Ptr, Type::Str, Type::Str],
            Type::RegExp,
        ),
        regex_test: declare_intrinsic(
            module,
            fn_table,
            "__torajs_regex_test",
            &[Type::RegExp, Type::Str],
            Type::Bool,
        ),
        regex_drop: declare_intrinsic(
            module,
            fn_table,
            "__torajs_regex_drop",
            &[Type::RegExp],
            Type::Void,
        ),
        regex_match: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_match_regex",
            &[Type::Str, Type::RegExp],
            Type::Ptr,
        ),
        any_str_symbol_probe: declare_intrinsic(
            module,
            fn_table,
            "__torajs_any_str_symbol_probe",
            &[Type::Any, Type::I64],
            Type::I64,
        ),
        any_str_symbol_invoke: declare_intrinsic(
            module,
            fn_table,
            "__torajs_any_str_symbol_invoke",
            &[Type::Str, Type::Any, Type::I64],
            Type::Any,
        ),
        any_str_symbol_invoke2: declare_intrinsic(
            module,
            fn_table,
            "__torajs_any_str_symbol_invoke2",
            &[Type::Str, Type::Any, Type::Any, Type::I64],
            Type::Any,
        ),
        regex_replace: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_replace_regex",
            &[Type::Str, Type::RegExp, Type::Str],
            Type::Str,
        ),
        regex_replace_all: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_replace_all_regex",
            &[Type::Str, Type::RegExp, Type::Str],
            Type::Str,
        ),
        regex_replace_fn: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_replace_regex_fn",
            &[Type::Str, Type::RegExp, Type::Ptr, Type::I64, Type::I64],
            Type::Str,
        ),
        regex_replace_all_fn: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_replace_all_regex_fn",
            &[Type::Str, Type::RegExp, Type::Ptr, Type::I64, Type::I64],
            Type::Str,
        ),
        regex_split: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_split_regex",
            &[Type::Str, Type::RegExp],
            Type::Ptr,
        ),
        regex_search: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_search_regex",
            &[Type::Str, Type::RegExp],
            Type::I64,
        ),
        regex_exec: declare_intrinsic(
            module,
            fn_table,
            "__torajs_regex_exec",
            &[Type::RegExp, Type::Str],
            Type::Ptr,
        ),
        regex_get_source: declare_intrinsic(
            module,
            fn_table,
            "__torajs_regex_get_source",
            &[Type::RegExp],
            Type::Str,
        ),
        regex_get_flags: declare_intrinsic(
            module,
            fn_table,
            "__torajs_regex_get_flags",
            &[Type::RegExp],
            Type::Str,
        ),
        regex_to_string: declare_intrinsic(
            module,
            fn_table,
            "__torajs_regex_to_string",
            &[Type::RegExp],
            Type::Str,
        ),
        regex_has_flag: declare_intrinsic(
            module,
            fn_table,
            "__torajs_regex_has_flag",
            &[Type::RegExp, Type::I64],
            Type::Bool,
        ),
        regex_match_all: declare_intrinsic(
            module,
            fn_table,
            "__torajs_str_match_all_regex",
            &[Type::Str, Type::RegExp],
            Type::Ptr,
        ),
        regex_get_last_index: declare_intrinsic(
            module,
            fn_table,
            "__torajs_regex_get_last_index",
            &[Type::RegExp],
            Type::F64,
        ),
        regex_set_last_index: declare_intrinsic(
            module,
            fn_table,
            "__torajs_regex_set_last_index",
            &[Type::RegExp, Type::F64],
            Type::Void,
        ),
    }
}
