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
    pub regex_compile: FuncId,
    pub regex_compile_from_static_dfa: FuncId,
    pub regex_test: FuncId,
    pub regex_drop: FuncId,
    pub regex_match: FuncId,
    pub regex_replace: FuncId,
    pub regex_replace_all: FuncId,
    pub regex_replace_fn: FuncId,
    pub regex_replace_all_fn: FuncId,
    pub regex_split: FuncId,
    pub regex_exec: FuncId,
    pub regex_get_source: FuncId,
    pub regex_get_flags: FuncId,
    pub regex_to_string: FuncId,
    pub regex_has_flag: FuncId,
    pub regex_match_all: FuncId,
    pub regex_get_last_index: FuncId,
    pub regex_set_last_index: FuncId,
}

pub(crate) fn declare(module: &mut Module, fn_table: &mut HashMap<String, FuncId>) -> RegexIds {
    RegexIds {
        regex_compile: declare_intrinsic(
            module,
            fn_table,
            "__torajs_regex_compile",
            &[Type::Str, Type::Str],
            Type::RegExp,
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
            Type::I64,
        ),
        regex_set_last_index: declare_intrinsic(
            module,
            fn_table,
            "__torajs_regex_set_last_index",
            &[Type::RegExp, Type::I64],
            Type::Void,
        ),
    }
}
