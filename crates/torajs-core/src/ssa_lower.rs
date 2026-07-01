#![allow(dead_code)] // step 2: minimum-scope lowerer; some helpers used by step 2.x onward

// AST → SSA lowerer (P3.5.a step 2).
//
// Scope of this step: just enough to lower fib40.tora.ts. That means:
//   - Top-level `Stmt::FnDecl` → `ssa::Function`
//   - `Stmt::If { else? }` → CondBr with no-else fall-through to merge block
//   - `Stmt::Return(expr?)` → Terminator::Ret
//   - `Stmt::Block`, `Stmt::Expr` (for chained calls)
//   - `Expr::Number` (i64 only — no f64 narrowing yet), `Bool`, `Ident`
//   - `Expr::BinOp` for the arith / compare / bitwise ops in the AST
//   - `Expr::Call { callee: Ident("...") }` resolving to a same-module FnDecl
//
// Deferred to step 2.x:
//   - `Stmt::LetDecl` + `Stmt::While` + `Expr::Assign` (need phi nodes)
//   - f64 numeric narrowing (number → f64 vs i64)
//   - `console.log(...)` at top level + a synthesized `main()` (step 3 wires
//     this when the Inkwell backend lands; right now `tr ssa` ignores
//     non-FnDecl top-level statements)
//   - Member-call resolution (only `Ident("...")` callees handled here)
//
// On unsupported shapes we panic with a clear message — labs material, not a
// user-facing tool yet. Will switch to a Result<_, LowerError> path when this
// is wired into a full `tr build` driver.

use std::collections::HashMap;

use crate::ast::{Ast, BinOp as AstBinOp, Expr, ExprId, Stmt};
use crate::short_str_encode::encode_short_str_literal;
use crate::ssa::{
    self, BinOp as SsaBinOp, BlockId, FPred, FuncId, IPred, InstKind, Module, Operand, Terminator,
    Type, ValueId,
};
pub(crate) use crate::ssa_lower_ctx_struct::LowerCtx;
pub(crate) use crate::ssa_lower_deep_clone::deep_clone_stmt;
pub(crate) use crate::ssa_lower_env_drop_and_ret_ty::{effective_ret_ty, synthesize_env_drop};
pub(crate) use crate::ssa_lower_generics_monomorph::{monomorphize_generics, substitute_in_ann};
use crate::ssa_lower_inner::lower_inner;
use crate::ssa_lower_push_loop_detect::let_counter_zero_name;
pub(crate) use crate::ssa_lower_rewrite_inner_generics::rewrite_inner_generic_calls;
pub(crate) use crate::ssa_lower_synthesize_main::{declare_intrinsic, synthesize_main};
use crate::ssa_lower_while_push_fast::lower_while_inner;

/// Phase 2B refcount: every heap-allocated Obj reserves a 24-byte
/// header:
///   offset 0  — universal heap header (refcount u32 + type_tag u16 + flags u16)
///   offset 8  — class tag (u64-slot; low 32 bits = per-class id, high
///               32 reserved)
///   offset 16 — vtable pointer (per-class const global; null for plain
///               `type` aliases)
/// Field 0 lives at `OBJ_HEADER_SIZE`, field i at
/// `OBJ_HEADER_SIZE + i*8`. Closure env layout is unaffected — it has
/// its own fn-ptr header at offset 0 and lives in a separate alloc path.
pub(crate) const OBJ_HEADER_SIZE: u64 = 24;
pub(crate) const OBJ_CLASS_TAG_OFF: u64 = 8;
pub(crate) const OBJ_VTABLE_OFF: u64 = 16;

/// Phase 2A refcount + T-13.5 deque layout (mirrors the `ARR_HDR_*`
/// constants in torajs-arr):
///
///   offset 0  — universal heap header (refcount u32 + type_tag u16 + flags u16)
///   offset 8  — len (u64)
///   offset 16 — cap (u32)
///   offset 20 — head (u32) — physical-slot offset of logical[0]; O(1) shift
///   offset 24 — props_dynobj (Round 4 chunk 5a)
///   offset 32 — slot data (N * 8 bytes physical capacity)
///
/// Logical index i lives at physical offset `32 + (head + i) * 8`.
/// Sites that access elements on a possibly-shifted array (Index, drop
/// walk, inc walk, pop) must add `head*8` to the byte offset; sites that
/// operate on freshly-allocated arrays (literal init, freshly-built dst
/// in concat/slice/spread) can skip the head load since head=0 there.
pub(crate) const ARR_LEN_OFF: u64 = 8;
const ARR_HEAD_OFF: u64 = 20;
/// Inline `props_dynobj` slot (Round 4 chunk 5a). Mirrors
/// `torajs_arr::layout::ARR_PROPS_OFF`. NULL when no `arr.x = v`
/// was ever written; chunk 5b+ flips arrprops_set to inline it.
#[allow(dead_code)]
pub(crate) const ARR_PROPS_OFF: u64 = 24;
/// Slot data offset — bumped 24 → 32 in Round 4 chunk 5a so the
/// new `props_dynobj` u64 fits at offset 24. Cross-crate sync:
/// `torajs_arr::layout::ARR_SLOTS_OFF` + `torajs_str::split::pool::ARR_HDR_SIZE`
/// must equal this.
pub(crate) const ARR_DATA_OFF: u64 = 32;

/// Phase 2C refcount: Closure env layout:
///
///   offset 0  — universal heap header (refcount u32 + type_tag u16 + flags u16)
///   offset 8  — fn_addr (entry point)
///   offset 16 — drop_fn  (per-closure cleanup, populated in Pass 2.5)
///   offset 24 — props_dynobj  (T-27 — Function as Object property bag,
///                              NULL until first `f.x = v` write; lazy-
///                              alloc'd dynobj per ECMAScript §10.2)
///   offset 32 — cap0
///   offset 40 — cap1
///   ...
///
/// `__torajs_obj_alloc` stays the underlying allocator (plain malloc);
/// the lowerer writes the universal header at the closure construction
/// site via `emit_obj_header_init` adapted for type_tag=CLOSURE.
pub const CLOSURE_FN_ADDR_OFF: u64 = 8;
pub const CLOSURE_DROP_FN_OFF: u64 = 16;
pub(crate) const CLOSURE_PROPS_OFF: u64 = 24;
pub(crate) const CLOSURE_CAP_BASE_OFF: u64 = 32;

/// M3 — generic call-site retargeting. For each `Expr::Call` whose ExprId
/// is a generic call site, the typechecker has already inferred the
/// concrete type args; this map remembers the **specialized fn name** the
/// monomorphization pre-pass picked for that call site, so the lowerer's
/// `Expr::Call` arm rewrites the callee to point at the specialized fn.
pub(crate) type CallRetargets = HashMap<ExprId, String>;

/// V3-18 m2.b — per-namespace known-own-property table for the
/// hasOwnProperty / propertyIsEnumerable subset stub. Only literal-
/// string keys land in this lookup; runtime keys default to `false`.
/// P9.5-A1.1 — count capture groups in a regex literal pattern. Used at
/// ssa-lower time by the `s.replace(re, fn)` dispatch to determine the
/// callback's expected arity (one match arg + N capture args). Mirrors
/// the runtime parser's group counter but operates on the raw source-
/// level pattern string (Expr::Regex.pattern) before tora's regex
/// compiler runs.
///
/// Counting rules per ES spec §22.2.1:
///   - `(` opens a capture group → +1
///   - `(?:` `(?=` `(?!` `(?<=` `(?<!` open non-capturing constructs → 0
///   - `(?<name>` is a named capture → +1 (rule for `<` not followed by `=`/`!`)
///   - `\(` is a literal paren → 0
///   - `[...]` char class: parens inside don't count
///   - `\\` followed by any char escapes that char
pub(crate) fn count_capture_groups(pattern: &str) -> usize {
    let bytes = pattern.as_bytes();
    let mut n = 0usize;
    let mut in_class = false;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'\\' {
            i += 2;
            continue;
        }
        if in_class {
            if b == b']' {
                in_class = false;
            }
            i += 1;
            continue;
        }
        if b == b'[' {
            in_class = true;
            i += 1;
            continue;
        }
        if b == b'(' {
            // (?:, (?=, (?!, (?<=, (?<! → non-capturing
            // (?<name> → capturing named group
            if i + 2 < bytes.len() && bytes[i + 1] == b'?' {
                let c = bytes[i + 2];
                if c == b':' || c == b'=' || c == b'!' {
                    i += 3;
                    continue;
                }
                if c == b'<' && i + 3 < bytes.len() {
                    let d = bytes[i + 3];
                    if d == b'=' || d == b'!' {
                        i += 4;
                        continue;
                    }
                    // (?<name>... — capturing named group, fall through to +1
                }
            }
            n += 1;
        }
        i += 1;
    }
    n
}

#[cfg(test)]
mod count_capture_groups_tests {
    use super::count_capture_groups;
    #[test]
    fn plain() {
        assert_eq!(count_capture_groups("foo"), 0);
        assert_eq!(count_capture_groups(""), 0);
    }
    #[test]
    fn one_group() {
        assert_eq!(count_capture_groups("(a)"), 1);
    }
    #[test]
    fn nested_groups() {
        assert_eq!(count_capture_groups("(a(b))"), 2);
        assert_eq!(count_capture_groups("((a))"), 2);
    }
    #[test]
    fn non_capturing() {
        assert_eq!(count_capture_groups("(?:a)"), 0);
        assert_eq!(count_capture_groups("(?=a)b"), 0);
        assert_eq!(count_capture_groups("(?!a)b"), 0);
        assert_eq!(count_capture_groups("(?<=a)b"), 0);
        assert_eq!(count_capture_groups("(?<!a)b"), 0);
    }
    #[test]
    fn named_capture() {
        assert_eq!(count_capture_groups("(?<n>a)"), 1);
        assert_eq!(count_capture_groups("(?<first>\\w+) (?<last>\\w+)"), 2);
    }
    #[test]
    fn mixed() {
        assert_eq!(count_capture_groups("(a)(?:b)(c)"), 2);
        assert_eq!(count_capture_groups("(\\w+) (\\w+)"), 2);
        assert_eq!(count_capture_groups("(a)(b)(c)"), 3);
    }
    #[test]
    fn char_class_parens() {
        assert_eq!(count_capture_groups("[(]"), 0);
        assert_eq!(count_capture_groups("[(ab)]"), 0);
        assert_eq!(count_capture_groups("([(a)])"), 1);
    }
    #[test]
    fn escaped_parens() {
        assert_eq!(count_capture_groups("\\("), 0);
        assert_eq!(count_capture_groups("\\(a\\)"), 0);
        assert_eq!(count_capture_groups("(a)\\("), 1);
    }
    #[test]
    fn complex() {
        // The common bun idiom: (\w+) (\w+) — 2 groups.
        assert_eq!(count_capture_groups("(\\w+) (\\w+)"), 2);
        // Mix: 1 named + 1 positional + 1 non-capturing.
        assert_eq!(count_capture_groups("(?<key>\\w+)(?:=)(\\w+)"), 2);
    }
}

/// T-28 — lower with the per-Call arity-pad map. Pads missing trailing
/// args with ANY_UNDEF Any-box operands at the call site for fns whose
/// trailing missing params are Type::Any. ssa_lower's Expr::Call arm
/// reads this and emits the padding before invoking the callee.
pub fn lower_with_arity(ast: &Ast, artifacts: &crate::check::CheckArtifacts) -> Module {
    let (generic_call_sites, expr_types, arity_pad_count, demoted_cm_rewrites) = artifacts;
    lower_inner(
        ast,
        generic_call_sites,
        expr_types,
        arity_pad_count,
        demoted_cm_rewrites,
    )
}

/// FuncIds of every backend-provided runtime entry point. Threaded through
/// every lowering site that needs to emit a runtime call. Single struct so
/// adding a new intrinsic later (e.g. `__torajs_str_concat` for P2.2.c)
/// only touches one type signature.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Intrinsics {
    /// Per-call-site trivial closure-wrapper drop. (FuncId, SigId).
    /// Used by the Return arm when wrapping a top-level FnAddr into
    /// a Closure-typed value to satisfy a fn signature returning
    /// `(...) => R` from a non-capturing return path. The wrapper
    /// env has just `fn_addr@0 + drop_fn@8` and no captures; the
    /// drop body just frees the env block.
    pub(crate) env_drop_trivial: (FuncId, ssa::SigId),
    pub(crate) print_i64: FuncId,
    pub(crate) print_f64: FuncId,
    pub(crate) print_bool: FuncId,
    pub(crate) str_alloc: FuncId,
    pub(crate) str_print: FuncId,
    pub(crate) str_drop: FuncId,
    pub(crate) str_concat: FuncId,
    /// Phase B refcount — `__torajs_rc_inc(ptr)` increments the heap
    /// header's refcount (NULL passes through). Emitted at every
    /// slot-copy / shared-ownership site for non-Copy heap values.
    pub(crate) rc_inc: FuncId,
    pub(crate) obj_alloc: FuncId,
    pub(crate) capture_box_alloc: FuncId,
    pub(crate) capture_box_inc: FuncId,
    pub(crate) capture_box_drop: FuncId,
    pub(crate) obj_drop_sized: FuncId,
    pub(crate) value_drop_heap: FuncId,
    pub(crate) cycle_unbuffer: FuncId,
    pub(crate) arr_alloc: FuncId,
    pub(crate) arr_push: FuncId,
    pub(crate) arr_push_non_deque: FuncId,
    pub(crate) arr_shift: FuncId,
    pub(crate) arr_unshift: FuncId,
    pub(crate) arr_splice: FuncId,
    pub(crate) arr_drop: FuncId,
    pub(crate) arr_reserve: FuncId,
    pub(crate) arr_push_unchecked: FuncId,
    pub(crate) arr_extend_unchecked: FuncId,
    pub(crate) arr_slice: FuncId,
    pub(crate) str_repeat: FuncId,
    pub(crate) str_to_upper: FuncId,
    pub(crate) str_to_lower: FuncId,
    pub(crate) str_trim: FuncId,
    pub(crate) str_trim_start: FuncId,
    pub(crate) str_trim_end: FuncId,
    pub(crate) str_pad_start: FuncId,
    pub(crate) str_pad_end: FuncId,
    pub(crate) str_from_char_code: FuncId,
    pub(crate) str_from_code_point: FuncId,
    pub(crate) str_normalize: FuncId,
    pub(crate) str_at: FuncId,
    pub(crate) str_replace: FuncId,
    pub(crate) str_replace_all: FuncId,
    pub(crate) num_to_fixed_f: FuncId,
    pub(crate) num_to_fixed_i: FuncId,
    pub(crate) num_to_string_radix_i: FuncId,
    pub(crate) num_to_string_radix_f: FuncId,
    pub(crate) num_to_exp_f: FuncId,
    pub(crate) num_to_exp_i: FuncId,
    pub(crate) num_to_precision_f: FuncId,
    pub(crate) num_to_precision_i: FuncId,
    pub(crate) num_to_locale_f: FuncId,
    pub(crate) num_to_locale_i: FuncId,
    pub(crate) num_parse_int: FuncId,
    pub(crate) num_parse_float: FuncId,
    pub(crate) num_is_integer_f: FuncId,
    pub(crate) num_is_integer_i: FuncId,
    pub(crate) num_is_nan_f: FuncId,
    pub(crate) num_is_nan_i: FuncId,
    pub(crate) num_is_finite_f: FuncId,
    pub(crate) num_is_finite_i: FuncId,
    pub(crate) num_is_safe_integer_f: FuncId,
    pub(crate) num_is_safe_integer_i: FuncId,
    pub(crate) num_is_integer_any: FuncId,
    pub(crate) num_is_nan_any: FuncId,
    pub(crate) num_is_finite_any: FuncId,
    pub(crate) num_is_safe_integer_any: FuncId,
    pub(crate) str_slice: FuncId,
    pub(crate) str_char_code_at: FuncId,
    pub(crate) str_code_point_at: FuncId,
    pub(crate) str_starts_with: FuncId,
    pub(crate) str_ends_with: FuncId,
    pub(crate) str_index_of: FuncId,
    pub(crate) str_last_index_of: FuncId,
    pub(crate) str_locale_compare: FuncId,
    pub(crate) str_includes: FuncId,
    pub(crate) str_eq: FuncId,
    pub(crate) str_split: FuncId,
    pub(crate) str_split_no_sep: FuncId,
    /// Phase Substr.A — substring view runtime helpers.
    pub(crate) substr_create: FuncId,
    pub(crate) substr_drop: FuncId,
    pub(crate) substr_char_code_at: FuncId,
    pub(crate) substr_code_point_at: FuncId,
    pub(crate) substr_eq_str: FuncId,
    pub(crate) substr_to_owned: FuncId,
    pub(crate) substr_starts_with: FuncId,
    pub(crate) substr_ends_with: FuncId,
    pub(crate) substr_includes: FuncId,
    pub(crate) substr_index_of: FuncId,
    pub(crate) substr_slice: FuncId,
    pub(crate) substr_substring: FuncId,
    pub(crate) substr_trim: FuncId,
    pub(crate) substr_trim_into: FuncId,
    pub(crate) substr_trim_start: FuncId,
    pub(crate) substr_trim_end: FuncId,
    pub(crate) substr_concat_substr_str: FuncId,
    pub(crate) substr_concat_str_substr: FuncId,
    pub(crate) substr_concat_substr_substr: FuncId,
    /// v0.2 #1 — regex matching engine. `regex_compile` parses the
    /// pattern + flag string at runtime into an NFA + flag bitset
    /// (Thompson construction); `regex_test` runs the backtracking
    /// matcher against a string and returns 1/0. Subsequent surface
    /// methods (`s.match`, `s.replace`, `re.exec`, ...) land in
    /// follow-up sub-phases as more `__torajs_regex_*` helpers.
    pub(crate) regex_compile: FuncId,
    /// V0.2 P14 chunk 7.7 v2 step 12 C2 Phase C-4 — AOT-baked DFA
    /// variant. See the declare site for the contract.
    pub(crate) regex_compile_from_static_dfa: FuncId,
    pub(crate) regex_test: FuncId,
    pub(crate) regex_get_source: FuncId,
    pub(crate) regex_get_flags: FuncId,
    pub(crate) regex_to_string: FuncId,
    pub(crate) regex_has_flag: FuncId,
    pub(crate) regex_drop: FuncId,
    pub(crate) regex_match: FuncId,
    pub(crate) regex_replace: FuncId,
    pub(crate) regex_replace_all: FuncId,
    /// P9.5-A1 — fn-callback variants of `s.replace(re, fn)` /
    /// `s.replaceAll(re, fn)`. 3rd arg is the closure env block (env+8
    /// = lifted body's fn_addr). Runtime invokes (env, match_str) ->
    /// ret_str per match.
    pub(crate) regex_replace_fn: FuncId,
    pub(crate) regex_replace_all_fn: FuncId,
    pub(crate) regex_split: FuncId,
    pub(crate) regex_exec: FuncId,
    pub(crate) regex_match_all: FuncId,
    /// P9.4 — `RegExp.prototype.lastIndex` accessors. Get returns the
    /// raw int64 field on the RegExp heap object; set stores it
    /// without coercion (typed-tier passes integer literals or
    /// arithmetic results that already lower to I64).
    pub(crate) regex_get_last_index: FuncId,
    pub(crate) regex_set_last_index: FuncId,
    pub(crate) date_now: FuncId,
    pub(crate) date_from_ms: FuncId,
    pub(crate) date_drop: FuncId,
    pub(crate) date_now_static: FuncId,
    pub(crate) date_get_time: FuncId,
    pub(crate) date_to_iso_string: FuncId,
    pub(crate) date_set_time: FuncId,
    pub(crate) date_get_year: FuncId,
    pub(crate) date_set_year: FuncId,
    pub(crate) date_to_gmt_string: FuncId,
    pub(crate) date_to_date_string: FuncId,
    pub(crate) date_to_locale_string: FuncId,
    pub(crate) date_to_locale_date_string: FuncId,
    pub(crate) date_to_locale_time_string: FuncId,
    pub(crate) date_set_full_year: FuncId,
    pub(crate) date_set_month: FuncId,
    pub(crate) date_set_date: FuncId,
    pub(crate) date_set_hours: FuncId,
    pub(crate) date_set_minutes: FuncId,
    pub(crate) date_set_seconds: FuncId,
    pub(crate) date_set_milliseconds: FuncId,
    pub(crate) date_get_full_year: FuncId,
    pub(crate) date_get_month: FuncId,
    pub(crate) date_get_date: FuncId,
    pub(crate) date_get_hours: FuncId,
    pub(crate) date_get_minutes: FuncId,
    pub(crate) date_get_seconds: FuncId,
    pub(crate) date_get_milliseconds: FuncId,
    pub(crate) date_get_day: FuncId,
    pub(crate) date_get_timezone_offset: FuncId,
    pub(crate) date_get_utc_full_year: FuncId,
    pub(crate) date_get_utc_month: FuncId,
    pub(crate) date_get_utc_date: FuncId,
    pub(crate) date_get_utc_hours: FuncId,
    pub(crate) date_get_utc_minutes: FuncId,
    pub(crate) date_get_utc_seconds: FuncId,
    pub(crate) date_get_utc_milliseconds: FuncId,
    pub(crate) date_get_utc_day: FuncId,
    pub(crate) date_from_components: FuncId,
    pub(crate) date_utc_components: FuncId,
    pub(crate) date_from_iso: FuncId,
    pub(crate) date_parse_iso: FuncId,
    pub(crate) fs_read_file_sync: FuncId,
    pub(crate) fs_write_file_sync: FuncId,
    pub(crate) fs_exists_sync: FuncId,
    pub(crate) fs_append_file_sync: FuncId,
    pub(crate) fs_unlink_sync: FuncId,
    pub(crate) fs_mkdir_sync: FuncId,
    pub(crate) fs_readdir_sync: FuncId,
    pub(crate) fs_size_sync: FuncId,
    pub(crate) process_exit: FuncId,
    pub(crate) process_cwd: FuncId,
    pub(crate) process_platform: FuncId,
    pub(crate) process_getenv: FuncId,
    pub(crate) argv_init: FuncId,
    pub(crate) process_argv: FuncId,
    pub(crate) process_stdout_write: FuncId,
    pub(crate) process_stderr_write: FuncId,
    pub(crate) arr_alloc_any: FuncId,
    pub(crate) arr_push_any: FuncId,
    pub(crate) arr_fill_any: FuncId,
    pub(crate) arr_extend_any: FuncId,
    pub(crate) arr_set_any: FuncId,
    pub(crate) arr_set_any_grow: FuncId,
    pub(crate) arr_oob_write_reject: FuncId,
    pub(crate) arr_get_any_tag: FuncId,
    pub(crate) arr_get_any_value: FuncId,
    pub(crate) dynobj_alloc: FuncId,
    pub(crate) get_builtin_prototype: FuncId,
    pub(crate) instanceof_class_any_tag: FuncId,
    pub(crate) instanceof_builtin_any_tag: FuncId,
    pub(crate) instanceof_object_any: FuncId,
    pub(crate) in_op_any_num: FuncId,
    pub(crate) in_op_any_str: FuncId,
    pub(crate) any_is_arr: FuncId,
    pub(crate) fnprops_set: FuncId,
    pub(crate) fnprops_get_tag: FuncId,
    pub(crate) fnprops_get_value: FuncId,
    pub(crate) arrprops_set: FuncId,
    pub(crate) arrprops_get_tag: FuncId,
    pub(crate) arrprops_get_value: FuncId,
    pub(crate) dynobj_get_tag: FuncId,
    pub(crate) dynobj_get_value: FuncId,
    pub(crate) dynobj_set: FuncId,
    pub(crate) dynobj_define: FuncId,
    pub(crate) dynobj_define_from_desc: FuncId,
    pub(crate) accessor_pair_new: FuncId,
    pub(crate) accessor_invoke_getter: FuncId,
    pub(crate) get_property_descriptor: FuncId,
    pub(crate) throw_typeerror_if_not_object: FuncId,
    pub(crate) arr_throw_reduce_empty: FuncId,
    pub(crate) arr_throw_reduce_right_empty: FuncId,
    pub(crate) arr_length_descriptor: FuncId,
    pub(crate) str_length_descriptor: FuncId,
    pub(crate) arr_index_strs: FuncId,
    pub(crate) str_index_strs: FuncId,
    pub(crate) arr_keys_only: FuncId,
    pub(crate) str_keys_only: FuncId,
    pub(crate) str_to_char_arr: FuncId,
    pub(crate) arr_entries_by_tag: FuncId,
    pub(crate) str_entries: FuncId,
    pub(crate) anyv_struct_keys: FuncId,
    pub(crate) anyv_struct_values: FuncId,
    pub(crate) anyv_struct_entries: FuncId,
    pub(crate) str_index_descriptor: FuncId,
    pub(crate) anyv_prevent_extensions: FuncId,
    pub(crate) anyv_is_extensible: FuncId,
    pub(crate) anyv_seal: FuncId,
    pub(crate) anyv_is_sealed: FuncId,
    pub(crate) dynobj_has: FuncId,
    pub(crate) dynobj_delete: FuncId,
    pub(crate) arr_drop_any: FuncId,
    pub(crate) any_box: FuncId,
    pub(crate) any_payload_rc_inc: FuncId,
    pub(crate) proto_register: FuncId,
    pub(crate) register_native_error: FuncId,
    pub(crate) proto_get: FuncId,
    pub(crate) class_register: FuncId,
    pub(crate) class_get: FuncId,
    pub(crate) get_proto_of_any: FuncId,
    pub(crate) any_typeof: FuncId,
    pub(crate) any_to_bool: FuncId,
    pub(crate) any_to_number: FuncId,
    pub(crate) any_add: FuncId,
    pub(crate) any_arith: FuncId,
    pub(crate) any_compare: FuncId,
    pub(crate) any_strict_eq: FuncId,
    pub(crate) any_any_strict_eq: FuncId,
    pub(crate) any_unbox_tag: FuncId,
    pub(crate) any_unbox_value: FuncId,
    pub(crate) any_box_drop: FuncId,
    pub(crate) print_any: FuncId,
    pub(crate) print_any_inline_top: FuncId,
    pub(crate) io_putc_stdout: FuncId,
    pub(crate) arr_print_i64_inline: FuncId,
    pub(crate) arr_print_f64_inline: FuncId,
    pub(crate) arr_print_bool_inline: FuncId,
    pub(crate) arr_print_str_inline: FuncId,
    pub(crate) arr_print_substr_inline: FuncId,
    pub(crate) map_print_outer: FuncId,
    pub(crate) set_print_outer: FuncId,
    pub(crate) fn_print_outer: FuncId,
    pub(crate) any_to_str: FuncId,
    pub(crate) obj_freeze: FuncId,
    pub(crate) obj_is_frozen: FuncId,
    pub(crate) obj_is_frozen_any: FuncId,
    pub(crate) obj_check_not_frozen: FuncId,
    /// v0.5 T-15.e — drains the microtask queue. Auto-called at
    /// main exit so chained Promise callbacks run before the
    /// process returns.
    pub(crate) microtask_drain: FuncId,
    /// P10.1-A1 — queueMicrotask(cb) closure-path enqueue. Takes
    /// the closure env pointer; runtime rc-inc's + dispatches at
    /// the next drain. cb's ABI is `(env*) -> void` (mirrors
    /// finally_closure).
    pub(crate) microtask_enqueue_closure: FuncId,
    /// P10.1-A1.1 — queueMicrotask(cb) simple-fn (no-env) enqueue.
    /// Takes a raw fn pointer; runtime dispatcher casts back to
    /// `void ()` and invokes. No rc (fn pointers are not heap
    /// objects). Selection happens at the queueMicrotask call
    /// site based on cb's static type (Type::Closure → _closure,
    /// Type::FnSig → this one), mirroring promise_then dispatch.
    pub(crate) microtask_enqueue_simple: FuncId,
    /// v0.5 T-15.g — Promise.resolve / Promise.reject runtime
    /// constructors + drop. The arg value is i64-packed (heap-ptr
    /// cast, bool widened, f64 bitcast).
    pub(crate) promise_alloc_fulfilled: FuncId,
    pub(crate) promise_resolve_thenable: FuncId,
    pub(crate) promise_alloc_rejected: FuncId,
    pub(crate) promise_alloc_fulfilled_heap: FuncId,
    pub(crate) promise_alloc_rejected_heap: FuncId,
    pub(crate) promise_drop: FuncId,
    pub(crate) promise_get_value: FuncId,
    pub(crate) promise_then_simple: FuncId,
    pub(crate) promise_then_closure: FuncId,
    pub(crate) promise_catch_simple: FuncId,
    pub(crate) promise_finally: FuncId,
    pub(crate) promise_catch_closure: FuncId,
    pub(crate) promise_finally_closure: FuncId,
    pub(crate) fetch_sync: FuncId,
    pub(crate) promise_all_sync: FuncId,
    pub(crate) promise_race_sync: FuncId,
    pub(crate) promise_any_sync: FuncId,
    pub(crate) promise_allsettled_sync: FuncId,
    pub(crate) bigint_from_decimal: FuncId,
    pub(crate) bigint_from_hex: FuncId,
    pub(crate) bigint_add: FuncId,
    pub(crate) bigint_sub: FuncId,
    pub(crate) bigint_mul: FuncId,
    pub(crate) bigint_div: FuncId,
    pub(crate) bigint_mod: FuncId,
    pub(crate) bigint_pow: FuncId,
    pub(crate) bigint_and: FuncId,
    pub(crate) bigint_or: FuncId,
    pub(crate) bigint_xor: FuncId,
    pub(crate) bigint_not: FuncId,
    pub(crate) bigint_shl: FuncId,
    pub(crate) bigint_shr: FuncId,
    pub(crate) bigint_from_str: FuncId,
    pub(crate) bigint_from_number: FuncId,
    pub(crate) bigint_clone: FuncId,
    pub(crate) bigint_neg: FuncId,
    pub(crate) bigint_cmp: FuncId,
    pub(crate) bigint_to_string: FuncId,
    pub(crate) bigint_to_string_radix: FuncId,
    pub(crate) bigint_as_int_n: FuncId,
    pub(crate) bigint_as_uint_n: FuncId,
    pub(crate) bigint_drop_rc: FuncId,
    pub(crate) weakref_create: FuncId,
    pub(crate) weakref_deref: FuncId,
    pub(crate) weakref_drop: FuncId,
    pub(crate) weakref_target_dying: FuncId,
    pub(crate) weakmap_create: FuncId,
    pub(crate) weakmap_set: FuncId,
    pub(crate) weakmap_get: FuncId,
    pub(crate) weakmap_has: FuncId,
    pub(crate) weakmap_delete: FuncId,
    pub(crate) weakmap_drop: FuncId,
    /* P6.1 — strong-ref Map<K,V> intrinsics. */
    pub(crate) map_create: FuncId,
    /* `__torajs_set_create()` — fresh Set heap (Map layout + TAG_SET).
     * Tag::Set discriminates Set from Map for the AnyValue tag-walker
     * (inspect.rs) so `const s: any = new Set()` console.log can route
     * to the bun `Set(N) {…}` printer instead of the Map walker. */
    pub(crate) set_create: FuncId,
    pub(crate) set_is_subset_of: FuncId,
    pub(crate) set_is_superset_of: FuncId,
    pub(crate) set_is_disjoint_from: FuncId,
    pub(crate) set_union: FuncId,
    pub(crate) set_intersection: FuncId,
    pub(crate) set_difference: FuncId,
    pub(crate) set_symmetric_difference: FuncId,
    pub(crate) map_clone: FuncId,
    pub(crate) map_set: FuncId,
    pub(crate) map_get: FuncId,
    pub(crate) map_has: FuncId,
    pub(crate) map_delete: FuncId,
    pub(crate) map_clear: FuncId,
    pub(crate) map_size: FuncId,
    pub(crate) map_drop: FuncId,
    pub(crate) map_iter_next: FuncId,
    pub(crate) map_iter_create_keys: FuncId,
    pub(crate) map_iter_create_values: FuncId,
    pub(crate) map_iter_create_entries: FuncId,
    pub(crate) map_iter_create_set_entries: FuncId,
    pub(crate) map_iter_step: FuncId,
    pub(crate) map_iter_drop: FuncId,
    pub(crate) arr_iter_create_keys: FuncId,
    pub(crate) arr_iter_create_values: FuncId,
    pub(crate) arr_iter_create_entries: FuncId,
    pub(crate) arr_iter_step: FuncId,
    pub(crate) arr_iter_drop: FuncId,
    pub(crate) weakset_create: FuncId,
    pub(crate) weakset_add: FuncId,
    pub(crate) weakset_has: FuncId,
    pub(crate) weakset_delete: FuncId,
    pub(crate) weakset_drop: FuncId,
    pub(crate) cycle_buffer: FuncId,
    pub(crate) cycle_at_exit_drain: FuncId,
    pub(crate) cycle_collect: FuncId,
    pub(crate) symbol_alloc: FuncId,
    pub(crate) symbol_drop: FuncId,
    pub(crate) symbol_print: FuncId,
    pub(crate) symbol_for: FuncId,
    pub(crate) symbol_key_for: FuncId,
    pub(crate) symbol_iterator: FuncId,
    pub(crate) symbol_async_iterator: FuncId,
    pub(crate) symbol_to_primitive: FuncId,
    pub(crate) object_is_f64: FuncId,
    pub(crate) split_iter_init: FuncId,
    pub(crate) split_iter_next: FuncId,
    pub(crate) split_iter_drop: FuncId,
    pub(crate) arr_from_string: FuncId,
    pub(crate) str_substring: FuncId,
    pub(crate) str_substr: FuncId,
    pub(crate) arr_set_length_validate: FuncId,
    pub(crate) arr_set_length_truncate_scalar: FuncId,
    pub(crate) arr_to_reversed: FuncId,
    pub(crate) arr_with: FuncId,
    pub(crate) arr_join: FuncId,
    pub(crate) arr_join_substr: FuncId,
    pub(crate) i64_to_str: FuncId,
    pub(crate) bool_to_str: FuncId,
    pub(crate) null_to_str: FuncId,
    pub(crate) undefined_to_str: FuncId,
    pub(crate) str_to_number: FuncId,
    pub(crate) arr_print_i64: FuncId,
    pub(crate) arr_print_f64: FuncId,
    pub(crate) arr_print_bool: FuncId,
    pub(crate) arr_print_str: FuncId,
    pub(crate) arr_print_substr: FuncId,
    pub(crate) substr_print: FuncId,
    pub(crate) str_char_at: FuncId,
    pub(crate) arr_join_i64: FuncId,
    pub(crate) arr_join_f64: FuncId,
    pub(crate) arr_join_i64_locale: FuncId,
    pub(crate) arr_join_f64_locale: FuncId,
    pub(crate) arr_join_bool: FuncId,
    pub(crate) arr_join_any: FuncId,
    pub(crate) symbol_to_str: FuncId,
    pub(crate) str_index_of_from: FuncId,
    pub(crate) str_last_index_of_from: FuncId,
    pub(crate) str_starts_with_from: FuncId,
    pub(crate) str_ends_with_from: FuncId,
    pub(crate) str_includes_from: FuncId,
    pub(crate) symbol_description: FuncId,
    pub(crate) f64_to_str: FuncId,
    pub(crate) math_sqrt: FuncId,
    pub(crate) math_abs: FuncId,
    pub(crate) math_floor: FuncId,
    pub(crate) math_ceil: FuncId,
    pub(crate) math_log: FuncId,
    pub(crate) math_exp: FuncId,
    pub(crate) math_sign: FuncId,
    pub(crate) math_round: FuncId,
    pub(crate) math_trunc: FuncId,
    pub(crate) math_pow: FuncId,
    pub(crate) math_min: FuncId,
    pub(crate) math_max: FuncId,
    pub(crate) math_sin: FuncId,
    pub(crate) math_cos: FuncId,
    pub(crate) math_tan: FuncId,
    pub(crate) math_asin: FuncId,
    pub(crate) math_acos: FuncId,
    pub(crate) math_atan: FuncId,
    pub(crate) math_atan2: FuncId,
    pub(crate) math_log2: FuncId,
    pub(crate) math_log10: FuncId,
    pub(crate) math_cbrt: FuncId,
    pub(crate) math_sinh: FuncId,
    pub(crate) math_cosh: FuncId,
    pub(crate) math_tanh: FuncId,
    pub(crate) math_asinh: FuncId,
    pub(crate) math_acosh: FuncId,
    pub(crate) math_atanh: FuncId,
    pub(crate) math_expm1: FuncId,
    pub(crate) math_log1p: FuncId,
    pub(crate) math_imul: FuncId,
    pub(crate) math_clz32: FuncId,
    pub(crate) math_fround: FuncId,
    pub(crate) math_f16round: FuncId,
    pub(crate) math_sum_precise: FuncId,
    pub(crate) math_sum_precise_i64: FuncId,
    pub(crate) math_random: FuncId,
    pub(crate) json_quote_str: FuncId,
    /// V0.2 P14-S5 — JSON builder fast path intrinsics for
    /// `JSON.stringify(struct)`. See `crates/torajs-str/src/
    /// json_builder.rs` and the `Type::Obj` arm of
    /// `lower_json_stringify`.
    pub(crate) jsb_new: FuncId,
    pub(crate) jsb_push_byte: FuncId,
    pub(crate) jsb_push_str_raw: FuncId,
    pub(crate) jsb_push_str_quoted: FuncId,
    pub(crate) jsb_push_i64: FuncId,
    pub(crate) jsb_push_bool: FuncId,
    pub(crate) jsb_finalize: FuncId,
    /// M6.3 — JSON.parse runtime helpers. See `runtime_str.c` for the
    /// per-helper contract. Cursor is `int64_t *`, threaded by the
    /// caller via an alloca slot; helpers advance it past the
    /// consumed token. Throws via `__torajs_throw_set` on mismatch.
    pub(crate) json_eat_char: FuncId,
    pub(crate) json_parse_int: FuncId,
    pub(crate) json_parse_float: FuncId,
    pub(crate) json_parse_bool: FuncId,
    pub(crate) json_parse_string: FuncId,
    pub(crate) json_arr_step: FuncId,
    pub(crate) json_arr_first: FuncId,
    pub(crate) str_eq_cstr: FuncId,
    pub(crate) print_i64_err: FuncId,
    pub(crate) print_f64_err: FuncId,
    pub(crate) print_bool_err: FuncId,
    pub(crate) str_print_err: FuncId,
    pub(crate) arr_flat: FuncId,
    pub(crate) arr_flat_any: FuncId,
    pub(crate) arr_extend_typed_into_any: FuncId,
    pub(crate) arr_concat: FuncId,
    pub(crate) arr_reverse: FuncId,
    pub(crate) arr_fill: FuncId,
    pub(crate) arr_copy_within: FuncId,
    /// M4 — exception state. `throw_set(value)` writes to module-level
    /// throw_active=1 + throw_value; `throw_check()` returns active flag;
    /// `throw_take()` reads value + clears flag. The backend defines the
    /// underlying globals.
    pub(crate) throw_set: FuncId,
    pub(crate) throw_check: FuncId,
    pub(crate) throw_take: FuncId,
    pub(crate) throw_take_tag: FuncId,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct LocalInfo {
    /// Pointer to the alloca slot — Type::Ptr.
    pub(crate) slot: ValueId,
    /// Type of the slot's *contents* (what Load returns).
    pub(crate) ty: Type,
    /// True after the binding's value has been consumed. Drop emission at
    /// fn-end skips moved locals.
    pub(crate) moved: bool,
    /// True if the binding never owned its heap value — it aliases a
    /// reference whose canonical owner lives elsewhere: non-Copy
    /// params (caller owns), closure captures (env owns), for-of
    /// bindings (the iterated container / iterator step owns),
    /// alias-init lets (`let v = o.f` / cross-scope `let n = s` —
    /// source owns). Distinct from `moved`: an owned local becomes
    /// `moved: true` when consumed, but `borrowed` is set at birth
    /// and never changes. `Stmt::Return` uses it to retain (+1) a
    /// returned borrow so the caller receives the owned reference
    /// the call-result convention promises.
    pub(crate) borrowed: bool,
    /// Lexical scope depth this binding was declared at. 0 = fn-root,
    /// each enclosing `Block` increments. Used by M1.3 to (a) drop
    /// inner-block locals at the closing `}` and (b) prevent cross-
    /// scope `let n = s` from transferring ownership (would dangle the
    /// outer-scope reference); see LetDecl in lower_stmt for the rule.
    pub(crate) scope_depth: usize,
}

/// Decode the `__env(name1|name2|...)` annotation lift_arrow_fns put on
/// a capturing closure's hidden first param. Returns the ordered capture
/// names. Returns `None` for anything that doesn't match the form.
pub(crate) fn decode_env_ann(ann: &str) -> Option<Vec<String>> {
    let inner = ann.strip_prefix("__env(")?.strip_suffix(')')?;
    if inner.is_empty() {
        return Some(Vec::new());
    }
    Some(inner.split('|').map(|s| s.to_string()).collect())
}

pub(crate) use crate::ssa_lower_parse_type::parse_type;

pub(crate) fn intern_arr_layout(arr_layouts: &mut Vec<Type>, elem: Type) -> ssa::ArrId {
    for (i, ex) in arr_layouts.iter().enumerate() {
        if *ex == elem {
            return ssa::ArrId(i as u32);
        }
    }
    let id = ssa::ArrId(arr_layouts.len() as u32);
    arr_layouts.push(elem);
    id
}

// P11.1-S2-a build-time encoding lives on `ssa::StringLiteral` as
// `StringLiteral::encode_from_str` — see crates/torajs-core/src/
// ssa/module_methods.rs.

pub(crate) fn intern_fn_sig(
    fn_sigs: &mut Vec<(Vec<Type>, Type)>,
    params: Vec<Type>,
    ret: Type,
) -> ssa::SigId {
    for (i, ex) in fn_sigs.iter().enumerate() {
        if ex.0 == params && ex.1 == ret {
            return ssa::SigId(i as u32);
        }
    }
    let id = ssa::SigId(fn_sigs.len() as u32);
    fn_sigs.push((params, ret));
    id
}

/// v0.6+1 perf checkpoint — per-array hoisted state for the push-loop
/// pre-reserve fast-push. See `LowerCtx::push_unchecked_for`.
#[derive(Clone, Copy)]
pub(crate) struct PreReserveState {
    /// The array's heap pointer at loop entry (= `arr_reserve`'s
    /// return). Used as the StoreDyn base + post-loop len-writeback
    /// target.
    pub(crate) arr_ptr: ValueId,
    /// Pre-computed `head_x8 + 24` — the byte offset from `arr_ptr`
    /// to slot[0]. Loop-invariant since the pattern detector
    /// excludes any body that could shift/unshift the array.
    pub(crate) head_off: ValueId,
    /// Local alloca'd i64 holding the running length. Initialized
    /// to the array's len at loop entry; bumped per push; written
    /// back to the array's len field at loop exit. mem2reg promotes
    /// this to a phi-register at -O1+.
    pub(crate) len_slot: ValueId,
}

impl<'a> LowerCtx<'a> {
    /// W1 — the num_width SlotKey for a let binding in the current fn.
    /// Top-level bindings key as Global regardless of whether Pass 1.5
    /// promoted them (the analysis keys every top-level let that way).
    pub(crate) fn num_width_local_key(&self, name: &str) -> crate::num_width::SlotKey {
        if self.is_main_fn {
            crate::num_width::SlotKey::Global(name.to_string())
        } else {
            crate::num_width::SlotKey::Local(self.f.name.clone(), name.to_string())
        }
    }

    /// True iff the current block hasn't been terminated yet (still has the
    /// default `Unreachable` placeholder). Used after lowering a sub-statement
    /// to decide whether we still need to emit a fall-through Br.
    pub(crate) fn cur_open(&self) -> bool {
        matches!(
            self.f.blocks[self.cur_block.0 as usize].term,
            Terminator::Unreachable
        )
    }

    /// 12-c-1 — route `while` through [`lower_while_inner`] with the
    /// let-zero counter derived from `prev`. Returns `true` iff `s`
    /// was a While; caller lowers non-Whiles normally. See the module
    /// doc on [`crate::ssa_lower_while_push_fast`].
    pub(crate) fn try_lower_while_fast(&mut self, prev: Option<&Stmt>, s: &Stmt) -> bool {
        let Stmt::While { cond, body } = s else {
            return false;
        };
        let counter = let_counter_zero_name(self.ast, prev);
        lower_while_inner(self, *cond, body, counter.as_deref());
        true
    }

    /// Top-level statement lowering inside the synthesized `main` function.
    /// `console.log(<expr>)` dispatches on the lowered operand's type:
    ///   - Type::Str → `call print_str(<ptr>)`
    ///   - Type::I64 / others → `call print_i64(<value>)`
    /// Same dispatch handles literal strings (`Expr::String`) and string
    /// bindings — the literal path interns through `lower_expr`'s general
    /// `Expr::String` arm and gets the same Type::Str operand.
    pub(crate) fn lower_top_stmt(&mut self, s: &Stmt) {
        if let Stmt::Expr(eid) = s
            && let Expr::Call { callee, args } = self.ast.get_expr(*eid)
            && let Some(method) = self.console_method_member(*callee)
            && args.len() == 1
        {
            // V3-18 Phase D + S139 — `console.log(null)` / `console.
            // log(undefined)` print 'null' / 'undefined' (per Node
            // util.inspect), not '0' as the generic Type::Ptr path
            // would. Both lower to ConstPtrNull at the runtime layer,
            // so we use the frontend type (expr_types) as the source
            // of truth. This covers the literal forms (Expr::Null,
            // Expr::Ident("undefined")) AND derived expressions like
            // `null && 'x'` (S138) whose result type is statically
            // Null/Undefined.
            let arg_check_ty = self.expr_types.get(&args[0]).cloned();
            let prim_label = match arg_check_ty {
                Some(crate::check::Type::Null) => Some("null"),
                Some(crate::check::Type::Undefined) => Some("undefined"),
                _ => None,
            };
            if let Some(label) = prim_label {
                // Side-effects: lower the arg first (in case it's a
                // Call), discard its value; then emit the literal
                // label via the str print path.
                let _ = self.lower_expr(args[0]);
                let lit = self.intern_string_literal(label);
                let target = self.console_print_target(method, Type::Str);
                self.f.append_void(
                    self.cur_block,
                    InstKind::Call(target, vec![Operand::Value(lit)]),
                );
                return;
            }
            let is_borrow = matches!(
                self.ast.get_expr(args[0]),
                Expr::Ident(_) | Expr::Member { .. } | Expr::Index { .. }
            );
            let arg = self.lower_expr(args[0]);
            let arg_ty = self.operand_ty(&arg);
            // Substr: materialize to owned Str (always-drop), then print as Str.
            if arg_ty == Type::Substr {
                let owned = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.substr_to_owned, vec![arg]),
                    Type::Str,
                    None,
                );
                let target = self.console_print_target(method, Type::Str);
                self.f.append_void(
                    self.cur_block,
                    InstKind::Call(target, vec![Operand::Value(owned)]),
                );
                self.emit_drop_value(Operand::Value(owned), Type::Str);
                if !is_borrow {
                    self.emit_drop_value(arg, Type::Substr);
                }
                return;
            }
            /* T-25 — BigInt prints via bigint_to_string + str_concat
             * with `"n"` (matches node/bun console.log formatting,
             * which appends the `n` suffix even though `toString()`
             * itself doesn't). The two intermediate Strs are
             * fresh-owned: drop both after print. The BigInt input
             * drops if the source binding wasn't a borrow target. */
            if arg_ty == Type::BigInt {
                let body = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.bigint_to_string, vec![arg]),
                    Type::Str,
                    None,
                );
                let n_lit = self.intern_string_literal("n");
                let formatted = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(
                        self.intrinsics.str_concat,
                        vec![Operand::Value(body), Operand::Value(n_lit)],
                    ),
                    Type::Str,
                    None,
                );
                let target = self.console_print_target(method, Type::Str);
                self.f.append_void(
                    self.cur_block,
                    InstKind::Call(target, vec![Operand::Value(formatted)]),
                );
                self.emit_drop_value(Operand::Value(formatted), Type::Str);
                self.emit_drop_value(Operand::Value(body), Type::Str);
                if !is_borrow {
                    self.emit_drop_value(arg, Type::BigInt);
                }
                return;
            }
            let is_str = arg_ty == Type::Str;
            let target = self.console_print_target(method, arg_ty);
            self.f
                .append_void(self.cur_block, InstKind::Call(target, vec![arg]));
            if is_str && !is_borrow {
                self.emit_drop_value(arg, Type::Str);
            }
            return;
        }
        // Multi-arg `console.log` per-arg inspect dispatch lives in
        // the sibling module so the `lower_stmt` (try-body) caller
        // can share it.
        if crate::ssa_lower_console_log_multiarg::try_lower(self, s) {
            return;
        }
        // Multi-arg `console.error` / `console.warn` — pre-existing
        // Str-coerce + str_concat joiner path. typed Arr / Obj /
        // Map / Set still panic here (only the `log` variant is
        // upgraded to per-arg inspect dispatch above); the stderr
        // arms see less coverage in conformance and the panic
        // surface is unchanged from the baseline.
        if let Stmt::Expr(eid) = s
            && let Expr::Call { callee, args } = self.ast.get_expr(*eid)
            && let Some(method) = self.console_method_member(*callee)
            && args.len() > 1
        {
            let arg_ids: Vec<ExprId> = args.clone();
            let space_str = self.intern_string_literal(" ");
            let mut acc: Option<Operand> = None;
            for (i, &aid) in arg_ids.iter().enumerate() {
                let arg = self.lower_expr(aid);
                let arg_ty = self.operand_ty(&arg);
                let s_op = self.coerce_to_str(arg, arg_ty);
                if i > 0 {
                    let prev = acc.unwrap();
                    let with_sep = self.f.append_inst(
                        self.cur_block,
                        InstKind::Call(
                            self.intrinsics.str_concat,
                            vec![prev, Operand::Value(space_str)],
                        ),
                        Type::Str,
                        None,
                    );
                    let combined = self.f.append_inst(
                        self.cur_block,
                        InstKind::Call(
                            self.intrinsics.str_concat,
                            vec![Operand::Value(with_sep), s_op],
                        ),
                        Type::Str,
                        None,
                    );
                    acc = Some(Operand::Value(combined));
                } else {
                    acc = Some(s_op);
                }
            }
            let target = self.console_print_target(method, Type::Str);
            let final_str = acc.unwrap();
            self.f
                .append_void(self.cur_block, InstKind::Call(target, vec![final_str]));
            self.emit_drop_value(final_str, Type::Str);
            return;
        }
        self.lower_stmt(s);
    }

    /// Coerce a value of any type to Type::Str. Used by multi-arg
    /// console.X to build a space-joined output line.
    /// M6.3 — peek at an Expr to see whether it's the
    /// `JSON.parse(text)` call shape that drives caller-typed JSON
    /// parsing. Used by Stmt::LetDecl to switch the init-lowering to
    /// `lower_json_parse` when the slot's annotation gives us a
    /// concrete target type.
    pub(crate) fn is_json_parse_call(&self, eid: ExprId) -> bool {
        let Expr::Call { callee, args } = self.ast.get_expr(eid) else {
            return false;
        };
        if args.len() != 1 {
            return false;
        }
        let Expr::Member { obj, name } = self.ast.get_expr(*callee) else {
            return false;
        };
        if name != "parse" {
            return false;
        }
        matches!(self.ast.get_expr(*obj), Expr::Ident(s) if s == "JSON")
    }

    /// T-19.d (v0.5.0) — `await Bun.file(p).json()` shape detection.
    /// After the parser's `await e` → `e.value` desugar, the init
    /// is `Member{obj=<Bun.file(p).json() call>, name: "value"}`.
    /// Returns Some(path_arg_eid) when the chain matches; None
    /// otherwise. Used by the LetDecl arm to dispatch to the
    /// caller-driven JSON parser when the slot has a concrete T.
    pub(crate) fn is_bun_file_json_await(&self, eid: ExprId) -> Option<ExprId> {
        let Expr::Member {
            obj: outer_call,
            name,
        } = self.ast.get_expr(eid)
        else {
            return None;
        };
        if name != "value" {
            return None;
        }
        let Expr::Call {
            callee: json_callee,
            args: json_args,
        } = self.ast.get_expr(*outer_call)
        else {
            return None;
        };
        if !json_args.is_empty() {
            return None;
        }
        let Expr::Member {
            obj: file_call,
            name: jname,
        } = self.ast.get_expr(*json_callee)
        else {
            return None;
        };
        if jname != "json" {
            return None;
        }
        let Expr::Call {
            callee: file_callee,
            args: file_args,
        } = self.ast.get_expr(*file_call)
        else {
            return None;
        };
        if file_args.len() != 1 {
            return None;
        }
        let Expr::Member {
            obj: bun_id,
            name: fname,
        } = self.ast.get_expr(*file_callee)
        else {
            return None;
        };
        if fname != "file" {
            return None;
        }
        if !matches!(self.ast.get_expr(*bun_id), Expr::Ident(s) if s == "Bun") {
            return None;
        }
        Some(file_args[0])
    }

    /// T-09.c (v0.4.0) — `Object.fromEntries(entries)` call shape.
    /// Routes to `lower_fromentries` from ssa_lower's LetDecl arm
    /// when the slot annotation gives a concrete struct type.
    pub(crate) fn is_fromentries_call(&self, eid: ExprId) -> bool {
        let Expr::Call { callee, args } = self.ast.get_expr(eid) else {
            return false;
        };
        // S309 — ES §20.1.2.7 silently ignores trailing args; widen
        // gate to accept >= 1. LetDecl fast-path lowers args[0] then
        // drops args[1..] before consuming entries.
        if args.is_empty() {
            return false;
        }
        let Expr::Member { obj, name } = self.ast.get_expr(*callee) else {
            return false;
        };
        if name != "fromEntries" {
            return false;
        }
        matches!(self.ast.get_expr(*obj), Expr::Ident(s) if s == "Object")
    }

    /// M6.3 — wrapper around `parse_type` that returns `None` when the
    /// annotation is missing or doesn't resolve to a concrete Type
    /// the JSON parser knows how to handle. Lets the LetDecl fast-
    /// path skip to the regular flow when the slot has no usable
    /// type info.
    pub(crate) fn try_resolve_type_ann(&mut self, ann: Option<&str>) -> Option<Type> {
        let ann = ann?;
        let ty = parse_type(
            Some(ann),
            self.aliases,
            self.arr_layouts,
            self.fn_sigs,
            self.generic_struct_decls,
            self.struct_layouts,
            self.inst_memo,
        );
        if matches!(ty, Type::Void) {
            return None;
        }
        Some(ty)
    }

    /// True when an expression's lowered Operand represents a freshly-
    /// allocated owned value the surrounding lowering site must drop.
    /// False for borrow-shaped exprs (Ident / Member / Index / OptChain
    /// / This — source binding owns the heap) and for string literals
    /// (`Expr::String(_)`: post-P-rpn lowers to `StaticStrRef`, rc-noop
    /// via STATIC_LITERAL; emitting `__torajs_str_drop`'s BL still
    /// clobbers caller-saved X0 and silently destroyed `n + "x"`-style
    /// ret values). Used by Expr::BinOp's post-call drop pass.
    pub(crate) fn expr_is_fresh_owned(&self, eid: ExprId) -> bool {
        !matches!(
            self.ast.get_expr(eid),
            Expr::Ident(_)
                | Expr::Member { .. }
                | Expr::Index { .. }
                | Expr::OptChain { .. }
                | Expr::This
                | Expr::String(_)
        )
    }

    pub(crate) fn coerce_to_str(&mut self, val: Operand, ty: Type) -> Operand {
        match ty {
            Type::Str => val,
            Type::Substr => {
                let v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.substr_to_owned, vec![val]),
                    Type::Str,
                    None,
                );
                Operand::Value(v)
            }
            Type::I64 => {
                let v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.i64_to_str, vec![val]),
                    Type::Str,
                    None,
                );
                Operand::Value(v)
            }
            Type::F64 => {
                let v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.f64_to_str, vec![val]),
                    Type::Str,
                    None,
                );
                Operand::Value(v)
            }
            Type::Bool => {
                let true_ptr = self.intern_string_literal("true");
                let false_ptr = self.intern_string_literal("false");
                let then_blk = self.f.add_block();
                let else_blk = self.f.add_block();
                let after_blk = self.f.add_block();
                let slot = self.alloca_in_entry(Type::Str, Some("__c_bool"));
                self.f.set_term(
                    self.cur_block,
                    Terminator::CondBr {
                        cond: val,
                        then_blk,
                        else_blk,
                    },
                );
                self.f.append_void(
                    then_blk,
                    InstKind::Store(Operand::Value(true_ptr), Operand::Value(slot), 0),
                );
                self.f.set_term(then_blk, Terminator::Br(after_blk));
                self.f.append_void(
                    else_blk,
                    InstKind::Store(Operand::Value(false_ptr), Operand::Value(slot), 0),
                );
                self.f.set_term(else_blk, Terminator::Br(after_blk));
                self.cur_block = after_blk;
                let v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Load(Type::Str, Operand::Value(slot), 0),
                    Type::Str,
                    None,
                );
                Operand::Value(v)
            }
            Type::BigInt => {
                /* T-25 — bigint_to_string + concat with `"n"` to
                 * match node/bun's console.log formatting. The
                 * caller will drop the resulting Str. The BigInt
                 * input itself is dropped by the caller's binding-
                 * lifetime walk; nothing to do here. */
                let body = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.bigint_to_string, vec![val]),
                    Type::Str,
                    None,
                );
                let n_lit = self.intern_string_literal("n");
                let formatted = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(
                        self.intrinsics.str_concat,
                        vec![Operand::Value(body), Operand::Value(n_lit)],
                    ),
                    Type::Str,
                    None,
                );
                self.emit_drop_value(Operand::Value(body), Type::Str);
                Operand::Value(formatted)
            }
            Type::Any => {
                /* Any-boxed value (catch param default / dynobj
                 * lookup result / etc.): split into tag + raw value
                 * via the unbox intrinsics, then route through the
                 * runtime's tag-dispatched ToString implementation.
                 * Returns a fresh-owned Str (rc=1; caller's
                 * post-call drop reclaims). Heap inputs are rc-inc'd
                 * by the runtime so the caller still sees a single
                 * owned ref. */
                let tag = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.any_unbox_tag, vec![val.clone()]),
                    Type::I64,
                    None,
                );
                let raw = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.any_unbox_value, vec![val]),
                    Type::I64,
                    None,
                );
                let s = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(
                        self.intrinsics.any_to_str,
                        vec![Operand::Value(tag), Operand::Value(raw)],
                    ),
                    Type::Str,
                    None,
                );
                Operand::Value(s)
            }
            other => {
                panic!("ssa-lower: console multi-arg coercion of type {other:?} not supported")
            }
        }
    }

    /// `console.log` recognized as an Ident("console") + Member.name == "log".
    fn is_console_log_member(&self, eid: ExprId) -> bool {
        match self.ast.get_expr(eid) {
            Expr::Member { obj, name } if name == "log" => {
                matches!(self.ast.get_expr(*obj), Expr::Ident(s) if s == "console")
            }
            _ => false,
        }
    }

    /// `console.{log,error,warn}` recognizer returning the method name as
    /// a static string (or None). Used to dispatch the appropriate
    /// print intrinsic in lower_top_stmt + the in-expr console-call arm.
    pub(crate) fn console_method_member(&self, eid: ExprId) -> Option<&'static str> {
        if let Expr::Member { obj, name } = self.ast.get_expr(eid)
            && let Expr::Ident(ns) = self.ast.get_expr(*obj)
            && ns == "console"
        {
            return match name.as_str() {
                "log" => Some("log"),
                "error" => Some("error"),
                "warn" => Some("warn"),
                // S328 — WHATWG console §1.1.{2,4} — info / debug
                // alias log (stdout) in bun/node behavior. Print
                // routing in `console_print_target` keeps both on
                // the non-stderr branch.
                "info" => Some("info"),
                "debug" => Some("debug"),
                _ => None,
            };
        }
        None
    }

    /// Pick the right print intrinsic for `console.<method>(<arg>)`.
    /// log / info / debug write to stdout; error / warn write to stderr.
    pub(crate) fn console_print_target(&self, method: &str, arg_ty: Type) -> FuncId {
        let to_stderr = matches!(method, "error" | "warn");
        match (arg_ty, to_stderr) {
            (Type::Str, false) => self.intrinsics.str_print,
            (Type::Str, true) => self.intrinsics.str_print_err,
            // V3-18 m1.h.34 — Substr layout differs from Str
            // (parent+offset+len vs inline data). Dedicated
            // substr_print walks parent + offset; pre-fix Substr
            // fell through to the catch-all print_i64 which printed
            // the pointer-as-integer (or nothing for empty), so any
            // `console.log("a-b".split("-")[0])` etc diverged from
            // bun.
            (Type::Substr, false) => self.intrinsics.substr_print,
            (Type::Substr, true) => self.intrinsics.substr_print,
            (Type::F64, false) => self.intrinsics.print_f64,
            (Type::F64, true) => self.intrinsics.print_f64_err,
            (Type::Bool, false) => self.intrinsics.print_bool,
            (Type::Bool, true) => self.intrinsics.print_bool_err,
            // T-10.d.i — Type::Any operand routes through the
            // tag-aware `__torajs_print_any` runtime helper. stderr
            // variant deferred to T-10.d.ii alongside the multi-arg
            // joiner; for v0.4 the boxed-Any path is single-arg-only,
            // and console.error/warn don't yet show up in any
            // conformance fixture that exercises Any operands.
            (Type::Any, false) => self.intrinsics.print_any,
            (Type::Any, true) => self.intrinsics.print_any,
            // T-13.a — Type::Symbol prints `Symbol(<desc>)` via the
            // dedicated runtime helper. stderr variant uses stdout for
            // now (no separate _err helper; matches console.error's
            // partial behavior on rare types).
            (Type::Symbol, _) => self.intrinsics.symbol_print,
            // V3-18 m1.h.12 — `console.log(arr)` array pretty-print.
            // Per element type: I64 / F64 / Bool / Str / Substr; any
            // other elem type (Any / Arr<...> / Obj / Map / Set /
            // Closure / etc) routes through the tag-aware
            // __torajs_print_anyv (Commit 4 wired its Tag::Arr +
            // Tag::DynObj walkers; Commits 5-8 wire the remaining
            // typed Tag walkers). Closes W-O-1 (`const a:any[]=[]`),
            // W-O-3-nested (`console.log(Object.entries(o))`).
            (Type::Arr(arr_id), false) => {
                let elem_ty = self.arr_layouts[arr_id.0 as usize];
                match elem_ty {
                    Type::I64 => self.intrinsics.arr_print_i64,
                    Type::F64 => self.intrinsics.arr_print_f64,
                    Type::Bool => self.intrinsics.arr_print_bool,
                    // V3-18 m1.h.28 — Substr layout differs from Str
                    // (parent + offset + len vs inline data); pick the
                    // matching helper. Pre-fix arr_print_str read
                    // parent-pointer bytes as data and printed garbage.
                    Type::Str => self.intrinsics.arr_print_str,
                    Type::Substr => self.intrinsics.arr_print_substr,
                    // Nested-print substrate trunk Commit 4.
                    _ => self.intrinsics.print_any,
                }
            }
            (Type::Arr(_), true) => self.intrinsics.print_any,
            // Nested-print substrate trunk Commit 4 — typed heap
            // receivers (Type::Obj / Map / Set / Promise / Date /
            // RegExp / Closure / WeakRef / WeakMap / WeakSet /
            // MapIter / ArrIter) route through __torajs_print_anyv,
            // which reads HeapHeader::type_tag and dispatches to its
            // Commit 4 Tag::DynObj walker, or for the remaining
            // tags falls back to `[object]\n` until Commits 5-8
            // wire each typed walker (Date / RegExp / Function in 5,
            // Map / Set / Promise in 6-8). Pre-Commit 4 these all
            // fell through to print_i64 below, which emitted the
            // raw heap pointer as a decimal — the typed-receiver
            // console.log fallback wedge.
            // Commit 7 — Map / Set route through dedicated wrappers
            // because runtime Tag::Map=15 covers BOTH Map and Set
            // heap blocks (no separate Tag::Set). Going through
            // print_any would print Sets as `Map(...)`.
            (Type::Map, _) => self.intrinsics.map_print_outer,
            (Type::Set, _) => self.intrinsics.set_print_outer,
            // Fn-name registry Phase 1 narrow — Type::FnSig is a
            // raw code-section pointer (not a heap object) so it
            // can't go through print_any's NaN-box tag-walker
            // (top16 of __TEXT vaddr is usually nonzero, so
            // `is_cell` returns false → `[unknown-any-tag]`
            // fallthrough). The dedicated outer wrapper emits
            // `[Function]\n` directly; Phase 2 swaps the body for
            // the rodata table binary-search.
            (Type::FnSig(_), _) => self.intrinsics.fn_print_outer,
            (Type::Obj(_), _)
            | (Type::Promise, _)
            | (Type::Date, _)
            | (Type::RegExp, _)
            | (Type::Closure(_), _)
            | (Type::WeakRef, _)
            | (Type::WeakMap, _)
            | (Type::WeakSet, _)
            | (Type::MapIter, _)
            | (Type::ArrIter, _) => self.intrinsics.print_any,
            (_, false) => self.intrinsics.print_i64,
            (_, true) => self.intrinsics.print_i64_err,
        }
    }

    /// `JSON.stringify(value)` — type-aware serializer. Emits SSA for
    /// the static type of `val_op` and returns a fresh Type::Str
    /// operand containing the JSON encoding. Recursive: arrays loop +
    /// dispatch on element type; structs unfold field-by-field at
    /// compile time. Always single-pass — no second walk for length
    /// pre-computation; fragments accumulate via str_concat.
    pub(crate) fn lower_json_stringify(&mut self, val_op: Operand, ty: Type) -> Operand {
        crate::ssa_lower_json_stringify::lower(self, val_op, ty)
    }

    /// Allocate a stack slot of `ty` in the current block. Returns the
    /// alloca's pointer ValueId. Used for `let`-decl locals + parameter
    /// home-slots (see lower_fn).
    pub(crate) fn alloca(&mut self, ty: Type, name: Option<&str>) -> ValueId {
        self.f
            .append_inst(self.cur_block, InstKind::Alloca(ty), Type::Ptr, name)
    }

    /// Allocate in the function's entry block (BlockId(0)) regardless of
    /// where lowering is currently positioned. Needed for slots whose
    /// loads happen on multiple control-flow predecessors that share no
    /// dominator other than entry — e.g. `__pending_break` /
    /// `__pending_continue` flags, where the lazy alloca otherwise lands
    /// in the break-block (which doesn't dominate the finally-tail
    /// fall-through path) and LLVM rejects with "Instruction does not
    /// dominate all uses".
    pub(crate) fn alloca_in_entry(&mut self, ty: Type, name: Option<&str>) -> ValueId {
        self.f
            .append_inst(BlockId(0), InstKind::Alloca(ty), Type::Ptr, name)
    }

    /// LetDecl binding-slot allocation. A refcounted binding is dropped
    /// at scope end, and that drop's `load <slot>` can land in a block
    /// with multiple predecessors sharing no dominator but entry (e.g.
    /// the post-try continuation, reachable from both the try-normal
    /// and catch paths). If the slot were `alloca`'d in whatever block
    /// lowering happened to be in — which a mid-expression block split
    /// (a may-throw call / bigint op) moves forward — that block won't
    /// dominate the drop's load and codegen rejects ("unmapped SSA
    /// value" — the backend maps values in block-insertion order).
    /// Entry-hoisting refcounted slots is the standard LLVM shape (all
    /// allocas in entry; mem2reg promotes them) and removes the whole
    /// fragility class. Copy slots have no scope-end drop, so they keep
    /// the cheaper in-place alloca (no behavior change for them).
    pub(crate) fn binding_slot_alloca(&mut self, ty: Type, name: &str) -> ValueId {
        if ty.is_refcounted() {
            let slot = self.alloca_in_entry(ty, Some(name));
            // T-49b — NULL-init the refcounted slot at entry. Without
            // this, a `const c = <may-throw-expr>` whose RHS throws
            // (e.g. `10n / 0n`) leaves the entry-hoisted slot with
            // stack-uninit bytes; the scope-end / main-exit drop walk
            // then calls `rc_dec` on garbage and SIGSEGVs. NULL is
            // the rc-dec NULL-guard sentinel — drops on it are
            // no-ops, mirroring the OLD LLVM pipeline's behaviour
            // where the LLVM mem2pass turns the alloca into an SSA
            // phi initialized to `null` in the entry block.
            //
            // Cheap: one store per refcounted let / const binding,
            // overwritten by the normal-path assignment. Bool flags
            // already follow this shape (see
            // `alloca_bool_flag_in_entry`).
            self.f.append_void(
                BlockId(0),
                InstKind::Store(Operand::ConstPtrNull, Operand::Value(slot), 0),
            );
            slot
        } else {
            self.alloca(ty, Some(name))
        }
    }

    /// Same as `alloca_in_entry` but also seeds the slot with `false`
    /// (for Bool flags) in the entry block. Without this, the flag is
    /// uninitialized memory on paths that reach the finally tail without
    /// having taken the break/continue branch (e.g. the i=0 iteration
    /// of `for { try { if i===N break } finally { … } }`); the finally
    /// tail's `Load` then sees garbage and may spuriously route through
    /// the break dispatch on the very first pass.
    pub(crate) fn alloca_bool_flag_in_entry(&mut self, name: Option<&str>) -> ValueId {
        let slot = self.alloca_in_entry(Type::Bool, name);
        self.f.append_void(
            BlockId(0),
            InstKind::Store(Operand::ConstBool(false), Operand::Value(slot), 0),
        );
        slot
    }

    /// If `eid` resolves to a non-Copy `Ident(name)` binding, mark that
    /// binding as moved. No-op for Copy types (number/bool/etc) and for
    /// non-Ident expressions (literals, BinOp results, Call results).
    /// Mirrors check.rs's affine consume pass.
    pub(crate) fn consume_if_ident(&mut self, eid: ExprId) {
        if let Expr::Ident(name) = self.ast.get_expr(eid) {
            let name = name.clone();
            if let Some(info) = self.locals.get_mut(&name)
                && !info.ty.is_copy()
            {
                info.moved = true;
            }
        }
    }

    /// Walk the entire expression tree under `eid` and mark every
    /// non-Copy `Expr::Ident(name)` reference as moved. Used at
    /// `Stmt::Return` so the drop walk skips any local whose heap
    /// might be aliased by the returned value (`return helper(f)`
    /// returns the same heap as `f` — dropping `f` before the return
    /// would dangle the pointer the caller is about to receive).
    /// Conservative: marks all non-Copy idents reached, even if not
    /// actually aliased — at the return site this is safe because
    /// the locals are about to go out of scope anyway. Stops at
    /// closure / arrow bodies (their captured names live in a
    /// separate frame).
    pub(crate) fn consume_all_idents_in_return(&mut self, eid: ExprId) {
        let mut stack: Vec<ExprId> = vec![eid];
        let mut visited: std::collections::HashSet<u32> = std::collections::HashSet::new();
        while let Some(id) = stack.pop() {
            if !visited.insert(id.0) {
                continue;
            }
            match self.ast.get_expr(id).clone() {
                Expr::Ident(name) => {
                    if let Some(info) = self.locals.get_mut(&name)
                        && !info.ty.is_copy()
                    {
                        info.moved = true;
                    }
                }
                Expr::BinOp { left, right, .. } => {
                    stack.push(left);
                    stack.push(right);
                }
                Expr::Unary { expr, .. }
                | Expr::TypeOf { expr }
                | Expr::Spread { expr }
                | Expr::InstanceOf { expr, .. } => {
                    stack.push(expr);
                }
                Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => {
                    stack.push(obj);
                }
                Expr::Call { callee, args } => {
                    stack.push(callee);
                    for a in args {
                        stack.push(a);
                    }
                }
                Expr::Assign { target, value } => {
                    stack.push(target);
                    stack.push(value);
                }
                Expr::Index { obj, index } => {
                    stack.push(obj);
                    stack.push(index);
                }
                Expr::Array(els) => {
                    for e in els {
                        stack.push(e);
                    }
                }
                Expr::ObjectLit { fields } => {
                    for (_, e) in fields {
                        stack.push(e);
                    }
                }
                Expr::Ternary {
                    cond,
                    then_branch,
                    else_branch,
                } => {
                    stack.push(cond);
                    stack.push(then_branch);
                    stack.push(else_branch);
                }
                Expr::Nullish { lhs, rhs } => {
                    stack.push(lhs);
                    stack.push(rhs);
                }
                Expr::New { class_name, args } => {
                    /* T-26 — `new WeakRef(target)` / `new WeakMap()`
                     * / `new WeakSet()` borrow their args (or take
                     * none); skip the recurse so the consume walk
                     * doesn't mark bound idents as moved.
                     * P6.1 — `new Map()` is zero-arg; the iterable-
                     * initializer overload (P6.5) will need its own
                     * recurse policy. */
                    if class_name == "WeakRef"
                        || class_name == "WeakMap"
                        || class_name == "WeakSet"
                        || class_name == "Map"
                        || class_name == "Set"
                    {
                        continue;
                    }
                    for e in args {
                        stack.push(e);
                    }
                }
                Expr::Super { args } => {
                    for e in args {
                        stack.push(e);
                    }
                }
                Expr::PostIncr { target, .. } => {
                    stack.push(target);
                }
                _ => {}
            }
        }
    }

    /// Drop a single Operand of non-Copy type. Recurses into struct fields:
    ///
    ///   Str       → call str_drop(val)
    ///   Obj(sid)  → for each non-Copy field at offset i*8:
    ///                  load field, recursively drop its value;
    ///               call obj_drop(val)  // free the outer struct after
    ///                                   // its non-Copy children are gone
    ///
    /// Copy fields don't show up here — they don't own anything heap.
    /// Recursion bottoms out at Str (the leaves) or at Obj with all-Copy
    /// fields (just free, no inner drops). Cycles aren't possible because
    /// our type aliases are declaration-ordered and forward refs are
    /// rejected at the type-decl pass — there's no way to build a
    /// recursive struct.
    /// Walk the `extends` chain from `cname` to decide whether the
    /// class is `Error` itself or a transitive subclass. Used to stamp
    /// FLAG_ERROR on the instance header so the uncaught reporter can
    /// render `name: message`. The hierarchy is acyclic (forward refs
    /// are rejected at the type-decl pass), so the walk terminates.
    pub(crate) fn class_is_error_derived(&self, cname: &str) -> bool {
        if cname == "Error" {
            return true;
        }
        let mut cur = self.ast.class_parents.get(cname).and_then(|p| p.clone());
        while let Some(name) = cur {
            if name == "Error" {
                return true;
            }
            cur = self.ast.class_parents.get(&name).and_then(|p| p.clone());
        }
        false
    }

    /// Phase 2B refcount: write the universal heap header (refcount=1
    /// + type_tag=OBJ + flags=0) at offset 0 of a freshly-alloc'd
    /// object. Lowerer emits this at every ObjectLit alloc site since
    /// `__torajs_obj_alloc` stays a plain malloc (re-used by box / env
    /// paths that don't want a refcount header).
    pub(crate) fn emit_obj_header_init(&mut self, obj_op: Operand) {
        // refcount @ +0 = 1
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::ConstI32(1), obj_op, 0),
        );
        // type_tag @ +4 = OBJ (1)  (i16 stored via i32; high 16 bits are
        // flags @ +6, also 0)
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::ConstI32(1), obj_op, 4),
        );
    }

    /// Clamp an i64 SSA value to [lo, hi] via two `select` SSA-shape
    /// branches. Used by Array helpers that take user-provided indices
    /// (start / end / target) and need to match the C runtime's clamp
    /// semantics for the in-place case.
    pub(crate) fn clamp_i64_to_range(&mut self, v: Operand, lo: Operand, hi: Operand) -> Operand {
        // step 1: max(v, lo)
        let too_low = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(IPred::Slt, v, lo),
            Type::Bool,
            None,
        );
        let lo_slot = self.alloca_in_entry(Type::I64, Some("__clamp_lo"));
        let lo_t = self.f.add_block();
        let lo_f = self.f.add_block();
        let lo_after = self.f.add_block();
        self.f.set_term(
            self.cur_block,
            Terminator::CondBr {
                cond: Operand::Value(too_low),
                then_blk: lo_t,
                else_blk: lo_f,
            },
        );
        self.f
            .append_void(lo_t, InstKind::Store(lo, Operand::Value(lo_slot), 0));
        self.f.set_term(lo_t, Terminator::Br(lo_after));
        self.f
            .append_void(lo_f, InstKind::Store(v, Operand::Value(lo_slot), 0));
        self.f.set_term(lo_f, Terminator::Br(lo_after));
        self.cur_block = lo_after;
        let after_lo = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::I64, Operand::Value(lo_slot), 0),
            Type::I64,
            None,
        );
        // step 2: min(after_lo, hi)
        let too_high = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(IPred::Sgt, Operand::Value(after_lo), hi),
            Type::Bool,
            None,
        );
        let hi_slot = self.alloca_in_entry(Type::I64, Some("__clamp_hi"));
        let hi_t = self.f.add_block();
        let hi_f = self.f.add_block();
        let hi_after = self.f.add_block();
        self.f.set_term(
            self.cur_block,
            Terminator::CondBr {
                cond: Operand::Value(too_high),
                then_blk: hi_t,
                else_blk: hi_f,
            },
        );
        self.f
            .append_void(hi_t, InstKind::Store(hi, Operand::Value(hi_slot), 0));
        self.f.set_term(hi_t, Terminator::Br(hi_after));
        self.f.append_void(
            hi_f,
            InstKind::Store(Operand::Value(after_lo), Operand::Value(hi_slot), 0),
        );
        self.f.set_term(hi_f, Terminator::Br(hi_after));
        self.cur_block = hi_after;
        let v = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::I64, Operand::Value(hi_slot), 0),
            Type::I64,
            None,
        );
        Operand::Value(v)
    }

    /// V3-18 wedge — JS spec relative-index normalisation, per the
    /// pattern that copyWithin / slice / splice / etc. use:
    /// for an integer index `n` against an array length `len`:
    ///   if n < 0: n = max(len + n, 0)
    ///   if n >= len: n = len
    ///   else: n
    /// Emits the `n < 0 ? n + len : n` select via condbr+slot+load
    /// (no SSA Select instruction), then chains through
    /// clamp_i64_to_range for the [0, len] clamp.
    pub(crate) fn relative_to_len(&mut self, v: Operand, len: Operand) -> Operand {
        // ES ToIntegerOrInfinity at the call boundary — every callsite
        // (copyWithin / fill / slice / indexOf / lastIndexOf etc.)
        // feeds a numeric index that must be a signed integer for the
        // inline ICmp/Add/Store chain below. A fractional / NaN / ±∞
        // literal lowers to f64 and would panic backend GPR
        // materialization. coerce_to_i64 const-folds NaN→0 / ±∞→i64::
        // {MAX,MIN} and emits FpToSi for non-const f64 (no-op when
        // already i64).
        let v = self.coerce_to_i64(v);
        let is_neg = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(IPred::Slt, v.clone(), Operand::ConstI64(0)),
            Type::Bool,
            None,
        );
        let plus_len = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(SsaBinOp::Add, v.clone(), len.clone()),
            Type::I64,
            None,
        );
        let eff_slot = self.alloca_in_entry(Type::I64, Some("__rel_eff"));
        let neg_blk = self.f.add_block();
        let pos_blk = self.f.add_block();
        let join = self.f.add_block();
        self.f.set_term(
            self.cur_block,
            Terminator::CondBr {
                cond: Operand::Value(is_neg),
                then_blk: neg_blk,
                else_blk: pos_blk,
            },
        );
        self.f.append_void(
            neg_blk,
            InstKind::Store(Operand::Value(plus_len), Operand::Value(eff_slot), 0),
        );
        self.f.set_term(neg_blk, Terminator::Br(join));
        self.f
            .append_void(pos_blk, InstKind::Store(v, Operand::Value(eff_slot), 0));
        self.f.set_term(pos_blk, Terminator::Br(join));
        self.cur_block = join;
        let effective = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::I64, Operand::Value(eff_slot), 0),
            Type::I64,
            None,
        );
        self.clamp_i64_to_range(Operand::Value(effective), Operand::ConstI64(0), len)
    }

    /// Boundary materialize: take an Array<Substr> and return a fresh
    /// Array<Str> with each element substr_to_owned'd. Drops the
    /// source array (its element-walk dec's parents; the new array's
    /// elements own the bytes outright). Used at fn / closure return
    /// sites where the declared type is Array<Str> but the body
    /// produced Array<Substr> (e.g. closure body `s => s.split("")`).
    pub(crate) fn materialize_arr_substr_to_str(
        &mut self,
        src: Operand,
        declared_ty: Type,
    ) -> Operand {
        let src_len = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::I64, src, ARR_LEN_OFF),
            Type::I64,
            None,
        );
        let dst = self.f.append_inst(
            self.cur_block,
            InstKind::Call(self.intrinsics.arr_alloc, vec![Operand::Value(src_len)]),
            declared_ty,
            None,
        );
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::Value(src_len), Operand::Value(dst), ARR_LEN_OFF),
        );
        // Per-element loop: substr_to_owned each.
        let i_slot = self.alloca(Type::I64, Some("__mat_i"));
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::ConstI64(0), Operand::Value(i_slot), 0),
        );
        let header = self.f.add_block();
        let body = self.f.add_block();
        let after = self.f.add_block();
        self.f.set_term(self.cur_block, Terminator::Br(header));
        self.cur_block = header;
        let i_now = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::I64, Operand::Value(i_slot), 0),
            Type::I64,
            None,
        );
        let cmp = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(IPred::Slt, Operand::Value(i_now), Operand::Value(src_len)),
            Type::Bool,
            None,
        );
        self.f.set_term(
            self.cur_block,
            Terminator::CondBr {
                cond: Operand::Value(cmp),
                then_blk: body,
                else_blk: after,
            },
        );
        self.cur_block = body;
        // T-13.5: src may be shifted (head>0) — use head-aware offset.
        // dst is freshly allocated above so head=0; reuse the raw
        // physical offset (i*8 + ARR_DATA_OFF) for the store.
        let src_off = self.emit_arr_slot_byte_offset(src.clone(), Operand::Value(i_now), 3, false);
        let scaled = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(SsaBinOp::Shl, Operand::Value(i_now), Operand::ConstI64(3)),
            Type::I64,
            None,
        );
        let off = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(
                SsaBinOp::Add,
                Operand::Value(scaled),
                Operand::ConstI64(ARR_DATA_OFF as i64),
            ),
            Type::I64,
            None,
        );
        let substr_v = self.f.append_inst(
            self.cur_block,
            InstKind::LoadDyn(Type::Substr, src, src_off),
            Type::Substr,
            None,
        );
        let owned = self.f.append_inst(
            self.cur_block,
            InstKind::Call(
                self.intrinsics.substr_to_owned,
                vec![Operand::Value(substr_v)],
            ),
            Type::Str,
            None,
        );
        self.f.append_void(
            self.cur_block,
            InstKind::StoreDyn(
                Operand::Value(owned),
                Operand::Value(dst),
                Operand::Value(off),
            ),
        );
        let i_next = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(SsaBinOp::Add, Operand::Value(i_now), Operand::ConstI64(1)),
            Type::I64,
            None,
        );
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::Value(i_next), Operand::Value(i_slot), 0),
        );
        self.f.set_term(self.cur_block, Terminator::Br(header));
        self.cur_block = after;
        // Drop the source Array<Substr> — its element-walk dec's each
        // substr (which dec's parent), then frees the array block.
        let src_arr_substr_ty = self.operand_ty(&src);
        self.emit_drop_value(src, src_arr_substr_ty);
        Operand::Value(dst)
    }

    /// Emit a refcount inc on `op`. Today expands to a single
    /// `Call(intrinsics.rc_inc, vec![op])` — semantically and
    /// instruction-wise equivalent to a direct emit. This helper
    /// is the single retrofit point for the future biased ARC
    /// (owner-thread fast path 0 atomic 增量 + share transition +
    /// atomic 慢路径,详见 `.claude/vision.md` 三-1 节 +
    /// `rules/torajs-design-principles.md` §6.2)。
    ///
    /// **HARD RULE (§6.2):** all refcount inc emit goes through
    /// this helper. Direct `InstKind::Call(intrinsics.rc_inc, ...)`
    /// in lowering code is a §6 violation.
    pub(crate) fn emit_rc_inc(&mut self, op: Operand) {
        let block = self.cur_block;
        self.emit_rc_inc_in(block, op);
    }

    /// Same as [`emit_rc_inc`] but emits into an explicit `block`
    /// instead of `self.cur_block`. Used by control-flow shapes that
    /// build a fresh `then_end` / `else_blk` and need to inc in a
    /// branch tail (e.g. Nullish-coalescing `??`).
    pub(crate) fn emit_rc_inc_in(&mut self, block: BlockId, op: Operand) {
        self.f
            .append_void(block, InstKind::Call(self.intrinsics.rc_inc, vec![op]));
    }

    /// Emit an inline refcount dec on the heap-header pointer `hdr`.
    /// Returns the new refcount value (Type::I32) so the caller can
    /// `ICmp(Eq, _, ConstI32(0))` to dispatch to drop. Mirrors the
    /// existing Bacon-Rajan inline shape: Load i32 @ offset 0 →
    /// `Sub 1` → Store back.
    ///
    /// Future biased ARC swap-point: this helper expands to an
    /// owner-thread check + atomic_rmw fetch_sub for shared objects.
    /// Today equivalent to the raw Load-Sub-Store sequence.
    ///
    /// **HARD RULE (§6.2):** all refcount dec emit goes through
    /// this helper or through the typed drop helpers
    /// (`emit_drop_value` / `intrinsics.{str_drop, arr_drop,
    /// substr_drop, value_drop_heap}`).
    pub(crate) fn emit_rc_dec_inline(&mut self, hdr: Operand) -> Operand {
        let rc_now = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::I32, hdr.clone(), 0),
            Type::I32,
            None,
        );
        let rc_new = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(SsaBinOp::Sub, Operand::Value(rc_now), Operand::ConstI32(1)),
            Type::I32,
            None,
        );
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::Value(rc_new), hdr, 0),
        );
        Operand::Value(rc_new)
    }

    pub(crate) fn emit_drop_value(&mut self, val: Operand, ty: Type) {
        crate::ssa_lower_emit_drop_value::emit(self, val, ty)
    }

    /// Emit drop sequences for every owned non-Copy local in the current
    /// block. Called immediately before terminators that exit the function
    /// (Ret, fall-through). Skips `moved` bindings — those have transferred
    /// ownership elsewhere and the receiver is responsible for the drop.
    pub(crate) fn lower_stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Multi(stmts) => {
                // Compiler-generated sequence — share surrounding scope.
                // No scope push, no drop emission of its own. Each child
                // lowers as if it appeared at the parent site. Used by
                // parse-time desugars (destructuring, possibly others)
                // that need to emit multiple lets without burying them
                // in a child block.
                let mut prev: Option<&Stmt> = None;
                for s in stmts {
                    if !self.try_lower_while_fast(prev, s) {
                        self.lower_stmt(s);
                    }
                    if !self.cur_open() {
                        break;
                    }
                    prev = Some(s);
                }
            }
            Stmt::Block(stmts) => {
                crate::ssa_lower_stmt_block::lower(self, stmts);
            }
            Stmt::LetDecl {
                mutable: _,
                name,
                type_ann,
                init,
                is_var: false,
            } => {
                crate::ssa_lower_stmt_let_decl::lower(self, name, type_ann.as_ref(), *init);
            }
            Stmt::While { cond, body } => {
                lower_while_inner(self, *cond, body, None);
            }
            Stmt::ForOfSplitIter {
                var_name,
                parent,
                sep,
                body,
            } => {
                crate::ssa_lower_stmt_for_of_split_iter::lower(self, var_name, *parent, *sep, body);
            }
            Stmt::ForOf {
                var_name,
                i_ident,
                elem_expr,
                body,
                ..
            } => {
                crate::ssa_lower_stmt_for_of::lower(self, var_name, i_ident, *elem_expr, body);
            }
            Stmt::DoWhile { body, cond } => {
                crate::ssa_lower_stmt_do_while::lower(self, body, *cond);
            }
            Stmt::Switch {
                scrutinee,
                cases,
                default,
            } => {
                crate::ssa_lower_stmt_switch::lower(self, *scrutinee, cases, default);
            }
            Stmt::For {
                init,
                cond,
                step,
                body,
            } => {
                crate::ssa_lower_stmt_for::lower(self, init.as_deref(), *cond, *step, body);
            }
            Stmt::Break => {
                crate::ssa_lower_stmt_break_continue::lower_break(self);
            }
            Stmt::Continue => {
                crate::ssa_lower_stmt_break_continue::lower_continue(self);
            }
            Stmt::Throw(eid) => {
                crate::ssa_lower_stmt_throw::lower(self, *eid);
            }
            Stmt::Try {
                body,
                had_catch,
                catch_param,
                catch_type,
                catch_body,
                finally_body,
            } => {
                crate::ssa_lower_stmt_try::lower(
                    self,
                    body,
                    *had_catch,
                    catch_param.as_ref(),
                    catch_type.as_ref(),
                    catch_body,
                    finally_body.as_ref(),
                );
            }
            Stmt::If {
                cond,
                then_branch,
                else_branch,
            } => {
                crate::ssa_lower_stmt_if::lower(self, *cond, then_branch, else_branch.as_deref());
            }
            Stmt::Return(maybe) => {
                crate::ssa_lower_stmt_return::lower(self, *maybe);
            }
            Stmt::Expr(eid) => {
                // Multi-arg `console.log` per-arg inspect dispatch —
                // shared with lower_top_stmt; without this the legacy
                // `coerce_to_str` joiner below would panic on typed
                // `Arr<T>` args inside try-body / nested block scopes.
                if crate::ssa_lower_console_log_multiarg::try_lower(self, s) {
                    return;
                }
                // Result discarded. Expression may still produce SSA insts as
                // side effects (its own value), e.g. nested Calls.
                let _ = self.lower_expr(*eid);
            }
            Stmt::TypeDecl { .. } => {
                // Pass 0 of `lower()` already registered the alias and
                // interned the layout. Re-encountering during the body
                // walk is a no-op.
            }
            Stmt::ImportDecl { .. } => {
                // K.1 single-file mode: no semantic effect. K.2 will
                // wire this into a cross-file symbol table.
            }
            Stmt::ExportDecl { .. } => {
                // K.1: `unwrap_exports` desugar should have flattened
                // the declaration-form. Bare named-export (`export {
                // ... }`) reaches here and is a no-op in single-file
                // mode.
            }
            other => {
                // Friendly classification for the most common shapes
                // that hit this catch-all so users get a readable
                // message instead of the raw AST debug print.
                let label = match other {
                    Stmt::FnDecl { name, .. } => format!(
                        "nested function declaration `{name}` inside a block / switch (planned: function-statement hoisting, see roadmap)"
                    ),
                    Stmt::ClassDecl { name, .. } => format!(
                        "nested class declaration `{name}` inside a block (planned: same hoisting story as nested functions)"
                    ),
                    _ => format!("statement shape not yet implemented: {other:?}"),
                };
                panic!("{label}");
            }
        }
    }

    /// Intern a string literal and return a Type::Str SSA value pointing at
    /// a fresh heap-allocated `{u64 len; u8 data[]}` copy. The static bytes
    /// live as a `[N x i8]` global (no NUL, len is explicit); `__torajs_str_alloc`
    /// copies them into a heap StrRepr at runtime. Every literal use does
    /// one alloc — caller is responsible for emitting Drop at scope end
    /// (P2.2.b.2 wires that up; this sub-step intentionally leaks one
    /// alloc per literal use, which is fine for one-shot bench programs).
    pub(crate) fn intern_string_literal(&mut self, s: &str) -> ValueId {
        // Phase P-rpn — every string-literal expression resolves to a
        // Str-shaped `StaticStrRef` global (rc_inc / rc_dec / free
        // all no-op via the STATIC_LITERAL flag). Encoding decision
        // happens in `StringLiteral::encode_from_str` (P11.1-S2-a).
        let lit = ssa::StringLiteral::encode_from_str(s);
        let sid = ssa::StringId((self.string_id_base + self.new_strings.len()) as u32);
        self.new_strings.push(lit);
        self.f
            .append_inst(self.cur_block, InstKind::StaticStrRef(sid), Type::Str, None)
    }

    /// T-10.c (v0.4.0) — cheap AST-shape probe for Array literal
    /// heterogeneity. Returns true iff the literal mixes DIFFERENT
    /// static-known kinds (Number vs String vs Bool vs Null among
    /// LITERAL elements only). Non-literal elements (Identifier,
    /// Call, Member, BinOp, ...) are treated as "kind unknown" and
    /// don't trigger the Any path — those route through the regular
    /// homogeneous codegen which already understands them. This
    /// means `[1, 'a', true]` → Any, but `[1, x, 3]` (where x is an
    /// `i64` ident) → regular Array<I64>. Matching the operand types
    /// of mixed expressions to the Any path is T-10.d work.
    pub(crate) fn array_literal_is_heterogeneous(&self, ids: &[ExprId]) -> bool {
        // Recursive — `Unary{Neg, Number(...)}` like `-3.14` keeps the
        // inner Number's kind so `[-3.14, 'x']` correctly flags as
        // heterogeneous (F64-kind vs Str-kind). Same for `+x` /
        // `~bits` if those ever appear inside an Array literal.
        fn classify(ast: &Ast, eid: ExprId) -> Option<u8> {
            match ast.get_expr(eid) {
                // W4 — int and fract literals share the number kind:
                // `[2, 1.5]` is a typed F64-elem array (the width
                // analysis seeds the literal's elem class and the
                // ArrayLit lowering coerces the int members), not an
                // Array<Any>. check.rs agrees (both are TS Number).
                Expr::Number(_) => Some(1),
                Expr::String(_) => Some(3),
                Expr::Bool(_) => Some(4),
                Expr::Null => Some(5),
                // S129-3 — nested Array literal counts as its own
                // kind so `[[1,2], 6]` (array + scalar) classifies
                // as heterogeneous → Array<Any> codegen. Pre-fix
                // nested arrays returned None, leaving the anchor
                // pinned to the scalar's kind; the array slots then
                // got raw-stored as i64 ptrs into a typed Array<T>,
                // breaking arr_flat_any's NaN-box decode. Homogeneous
                // nested literals (`[[1,2],[3,4]]`) still anchor to
                // the same kind = 2 → typed Array<Array<T>>.
                Expr::Array(_) => Some(2),
                Expr::Unary { expr, .. } => classify(ast, *expr),
                _ => None, // unknown kind — fall back to homogeneous path
            }
        }
        let mut anchor: Option<u8> = None;
        for &eid in ids {
            if let Some(k) = classify(self.ast, eid) {
                match anchor {
                    None => anchor = Some(k),
                    Some(a) if a != k => return true,
                    _ => {}
                }
            }
        }
        false
    }

    /// T-10.c (v0.4.0) — emit codegen for a heterogeneous Array
    /// literal. alloc_any(N) sized to fit, then per-element box +
    /// push_any with the matching tag. Returns the (possibly grown)
    /// P0 — box a concrete-typed Operand into the universal Any-box.
    /// Returns an Operand::Value of Type::Ptr that points at a fresh
    /// 24-byte heap struct (header + tag + payload). For ANY_HEAP
    /// values the inner heap pointer's refcount is bumped by the
    /// runtime helper so the box's drop releases its share. Caller
    /// owns the returned box; ssa_lower's regular drop walk frees
    /// it via __torajs_any_box_drop. Also used by lower_array_any
    /// (which encodes the same tag scheme but inlines the call).
    /// P3.2 — `let x: any = { f1: v1, f2: v2 }` lowering. Allocate
    /// a dynobj via `__torajs_dynobj_alloc()`, populate each field
    /// via `dynobj_set`, then box the dynobj ptr as ANY_HEAP=4 so
    /// the slot holds an Any-box pointing at the dynobj. Subsequent
    /// `x.foo` reads/writes route through the dynobj substrate.
    /// Empty `{}` produces a zero-entry dynobj (allocates the header
    /// + initial bucket array but no entries).
    pub(crate) fn lower_dynobj_init(&mut self, eid: ExprId) -> Operand {
        let fields = match self.ast.get_expr(eid).clone() {
            Expr::ObjectLit { fields } => fields,
            _ => panic!("lower_dynobj_init called on non-ObjectLit"),
        };
        // Allocate the dynobj.
        let dynobj = self.f.append_inst(
            self.cur_block,
            InstKind::Call(self.intrinsics.dynobj_alloc, Vec::new()),
            Type::Ptr,
            None,
        );
        // For each (name, value), set into the dynobj. Box value
        // first using the same scheme as box_to_any but inlined.
        for (fname, fval_eid) in fields {
            let v_raw = self.lower_expr(fval_eid);
            self.consume_if_ident(fval_eid);
            let v_ty = self.operand_ty(&v_raw);
            let (tag, val_op): (i64, Operand) = match v_ty {
                Type::I64 | Type::I32 => (2, v_raw),
                Type::F64 => {
                    let bits = self.f.append_inst(
                        self.cur_block,
                        InstKind::BitCastF64ToI64(v_raw),
                        Type::I64,
                        None,
                    );
                    (3, Operand::Value(bits))
                }
                Type::Bool => {
                    let zext = self.f.append_inst(
                        self.cur_block,
                        InstKind::ZExtBoolToI64(v_raw),
                        Type::I64,
                        None,
                    );
                    (1, Operand::Value(zext))
                }
                // P4.0 — Type::Any must be unboxed BEFORE the
                // is_refcounted catch-all (Type::Any is itself
                // refcounted, so the `_ if v_ty.is_refcounted()`
                // arm would otherwise grab the any-box wrapper
                // ptr and store *that* as the bucket value with
                // tag=ANY_HEAP. Reads then return the wrapper ptr
                // instead of the underlying heap object, breaking
                // identity (`{p: inner}.p === inner`) and recursive
                // field access (`outer.p.x`). Forward (tag, val) via
                // any_unbox_tag/_value shims (Step 7c — was inline
                // `Load i64 +8/+16` direct-offset); bucket owns the
                // +1 on val via any_payload_rc_inc when tag == HEAP.
                Type::Any => {
                    let tag_v = self.f.append_inst(
                        self.cur_block,
                        InstKind::Call(self.intrinsics.any_unbox_tag, vec![v_raw.clone()]),
                        Type::I64,
                        None,
                    );
                    let val_v = self.f.append_inst(
                        self.cur_block,
                        InstKind::Call(self.intrinsics.any_unbox_value, vec![v_raw.clone()]),
                        Type::I64,
                        None,
                    );
                    self.f.append_void(
                        self.cur_block,
                        InstKind::Call(
                            self.intrinsics.any_payload_rc_inc,
                            vec![Operand::Value(tag_v), Operand::Value(val_v)],
                        ),
                    );
                    let key_str = self.intern_string_literal(&fname);
                    let slot = self.alloca(Type::Ptr, Some("__dynobj_init_slot"));
                    self.f.append_void(
                        self.cur_block,
                        InstKind::Store(Operand::Value(dynobj), Operand::Value(slot), 0),
                    );
                    self.f.append_void(
                        self.cur_block,
                        InstKind::Call(
                            self.intrinsics.dynobj_set,
                            vec![
                                Operand::Value(slot),
                                Operand::Value(key_str),
                                Operand::Value(tag_v),
                                Operand::Value(val_v),
                            ],
                        ),
                    );
                    continue;
                }
                _ if v_ty.is_refcounted() => {
                    self.emit_rc_inc(v_raw.clone());
                    (4, v_raw)
                }
                Type::Ptr if matches!(v_raw, Operand::ConstPtrNull) => (0, Operand::ConstI64(0)),
                _ => panic!("ssa-lower: dynobj init unsupported field type {v_ty:?}"),
            };
            let key_str = self.intern_string_literal(&fname);
            let slot = self.alloca(Type::Ptr, Some("__dynobj_init_slot"));
            self.f.append_void(
                self.cur_block,
                InstKind::Store(Operand::Value(dynobj), Operand::Value(slot), 0),
            );
            self.f.append_void(
                self.cur_block,
                InstKind::Call(
                    self.intrinsics.dynobj_set,
                    vec![
                        Operand::Value(slot),
                        Operand::Value(key_str),
                        Operand::ConstI64(tag),
                        val_op,
                    ],
                ),
            );
        }
        Operand::Value(dynobj)
    }

    /// P1.5 — `box_to_any` variant that knows the source frontend
    /// type, so it can pick ANY_UNDEF=5 vs ANY_NULL=0 for the
    /// pointer-shaped cases. The tag is the only thing that
    /// distinguishes null from undefined at the runtime level
    /// (both lower to ConstPtrNull); the per-tag rules in
    /// any_typeof / any_to_str / any_to_bool / etc. then preserve
    /// the spec distinction downstream.
    pub(crate) fn box_to_any_from_expr(&mut self, eid: ExprId, val: Operand) -> Operand {
        let is_undef = matches!(
            self.expr_types.get(&eid),
            Some(crate::check::Type::Undefined)
        );
        let val_ty = self.operand_ty(&val);
        if is_undef && matches!(val_ty, Type::Ptr) {
            // ANY_UNDEF=5, payload 0.
            let v = self.f.append_inst(
                self.cur_block,
                InstKind::Call(
                    self.intrinsics.any_box,
                    vec![Operand::ConstI64(5), Operand::ConstI64(0)],
                ),
                Type::Any,
                None,
            );
            return Operand::Value(v);
        }
        // Step 8d — IR-side const ShortStr emit for compile-time short
        // string literals. When boxing a Type::Str whose source expression
        // is a string literal of ≤ SHORT_STR_CAP bytes, bypass the runtime
        // `any_box(4, str_ptr)` call: encode the bytes directly into a
        // NaN-box ShortStr u64 at compile time, then emit
        // IntToPtr(ConstI64(short_u64)) typed as Any. The dead StaticStrRef
        // inst left behind is dropped by LLVM DCE (no side effects);
        // STATIC_LITERAL strings carry a no-op rc_dec path so any leftover
        // scope-end drop is also a no-op at runtime.
        if matches!(val_ty, Type::Str)
            && let Expr::String(s) = self.ast.get_expr(eid)
            && let Some(short_u64) = encode_short_str_literal(s.as_bytes())
        {
            let v = self.f.append_inst(
                self.cur_block,
                InstKind::IntToPtr(Operand::ConstI64(short_u64 as i64)),
                Type::Any,
                None,
            );
            return Operand::Value(v);
        }
        self.box_to_any(val)
    }

    /// Lower an expression to its `(tag, value)` pair, with the same
    /// frontend-type awareness as `box_to_any_from_expr`. Used by sites
    /// that need both the unboxed pair *and* the spec-correct ANY_UNDEF
    /// tag for an `undefined` literal (P6.1 Map.set / has / delete /
    /// get etc.) — plain `box_to_tag_value` would otherwise see only
    /// the SSA-level `Type::Ptr` + `ConstPtrNull` and emit ANY_NULL,
    /// collapsing undefined and null into the same key.
    pub(crate) fn lower_to_tag_value(&mut self, eid: ExprId) -> (Operand, Operand) {
        let is_undef = matches!(
            self.expr_types.get(&eid),
            Some(crate::check::Type::Undefined)
        );
        let val = self.lower_expr(eid);
        let val_ty = self.operand_ty(&val);
        if is_undef && matches!(val_ty, Type::Ptr) {
            return (Operand::ConstI64(5), Operand::ConstI64(0));
        }
        self.box_to_tag_value(val)
    }

    /// Extract `(tag_op, value_op)` for a freshly-lowered value, matching
    /// `box_to_any`'s tag scheme. Used by sites that need the unboxed
    /// pair instead of an Any-box (e.g. dynobj_set / fn_props_set
    /// which take tag + value as separate args). For statically-typed
    /// values the tag is `ConstI64(literal)`; for already-boxed Any
    /// it's a Load extracting the box's runtime tag at +8 — callers
    /// must pass the returned Operand straight through (don't unwrap
    /// to an i64 literal).
    pub(crate) fn box_to_tag_value(&mut self, val: Operand) -> (Operand, Operand) {
        let val_ty = self.operand_ty(&val);
        match val_ty {
            Type::I64 | Type::I32 => (Operand::ConstI64(2), val),
            Type::F64 => {
                let bits = self.f.append_inst(
                    self.cur_block,
                    InstKind::BitCastF64ToI64(val),
                    Type::I64,
                    None,
                );
                (Operand::ConstI64(3), Operand::Value(bits))
            }
            Type::Bool => {
                let zext = self.f.append_inst(
                    self.cur_block,
                    InstKind::ZExtBoolToI64(val),
                    Type::I64,
                    None,
                );
                (Operand::ConstI64(1), Operand::Value(zext))
            }
            // P4.0 — Type::Any must be unboxed BEFORE the
            // is_refcounted catch-all (Type::Any is itself
            // refcounted; would otherwise grab the any-box wrapper
            // ptr and tag=ANY_HEAP, dropping the real tag/value).
            // Step 7c: read via any_unbox_tag/_value shims (was
            // inline `Load i64 +8/+16` direct-offset).
            Type::Any => {
                let tag_v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.any_unbox_tag, vec![val.clone()]),
                    Type::I64,
                    None,
                );
                let val_v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.any_unbox_value, vec![val]),
                    Type::I64,
                    None,
                );
                self.f.append_void(
                    self.cur_block,
                    InstKind::Call(
                        self.intrinsics.any_payload_rc_inc,
                        vec![Operand::Value(tag_v), Operand::Value(val_v)],
                    ),
                );
                (Operand::Value(tag_v), Operand::Value(val_v))
            }
            _ if val_ty.is_refcounted() => {
                self.emit_rc_inc(val.clone());
                (Operand::ConstI64(4), val)
            }
            Type::Ptr if matches!(val, Operand::ConstPtrNull) => {
                (Operand::ConstI64(0), Operand::ConstI64(0))
            }
            other => panic!("ssa-lower: box_to_tag_value type {other:?} not supported"),
        }
    }

    /// T-27 — `f.x = (tag, val)` against a closure. Loads the lazy
    /// props_dynobj at CLOSURE_PROPS_OFF, allocates it on first write,
    /// stores the new ptr back into the closure (it may also resize
    /// later). Then calls dynobj_set against the live ptr through a
    /// stack slot so the resize-aware writeback works.
    pub(crate) fn fn_props_set(
        &mut self,
        closure_op: Operand,
        key: &str,
        tag: Operand,
        val_op: Operand,
    ) {
        let key_str = self.intern_string_literal(key);
        // Load current props ptr (NULL on first write).
        let cur_props = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::Ptr, closure_op.clone(), CLOSURE_PROPS_OFF),
            Type::Ptr,
            None,
        );
        // Branch on NULL: alloc-and-store, or use existing.
        let is_null = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(IPred::Eq, Operand::Value(cur_props), Operand::ConstPtrNull),
            Type::Bool,
            None,
        );
        let alloc_blk = self.f.add_block();
        let after_alloc = self.f.add_block();
        let cb0 = self.cur_block;
        self.f.set_term(
            cb0,
            Terminator::CondBr {
                cond: Operand::Value(is_null),
                then_blk: alloc_blk,
                else_blk: after_alloc,
            },
        );
        // alloc path: dynobj_alloc() → store at CLOSURE_PROPS_OFF.
        self.cur_block = alloc_blk;
        let new_props = self.f.append_inst(
            self.cur_block,
            InstKind::Call(self.intrinsics.dynobj_alloc, vec![]),
            Type::Ptr,
            None,
        );
        self.f.append_void(
            self.cur_block,
            InstKind::Store(
                Operand::Value(new_props),
                closure_op.clone(),
                CLOSURE_PROPS_OFF,
            ),
        );
        let ab = self.cur_block;
        self.f.set_term(ab, Terminator::Br(after_alloc));
        // after_alloc: re-load to get whichever path's value, stash in
        // a stack slot for the resize-aware dynobj_set, set, then write
        // back to closure props.
        self.cur_block = after_alloc;
        let live_props = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::Ptr, closure_op.clone(), CLOSURE_PROPS_OFF),
            Type::Ptr,
            None,
        );
        let slot = self.alloca(Type::Ptr, Some("__fnprops_slot"));
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::Value(live_props), Operand::Value(slot), 0),
        );
        self.f.append_void(
            self.cur_block,
            InstKind::Call(
                self.intrinsics.dynobj_set,
                vec![Operand::Value(slot), Operand::Value(key_str), tag, val_op],
            ),
        );
        // P3.attribute-flag-tracking — fnprops user assign can hit a
        // writable=false existing bucket.
        self.emit_throw_check(None);
        // Writeback resize-aware ptr.
        let new_live = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::Ptr, Operand::Value(slot), 0),
            Type::Ptr,
            None,
        );
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::Value(new_live), closure_op, CLOSURE_PROPS_OFF),
        );
    }

    /// T-27 — `f.x` read against a closure. Loads props_dynobj at
    /// CLOSURE_PROPS_OFF; NULL → ANY_UNDEF box. Otherwise calls
    /// dynobj_get_tag/value with the key and boxes the result.
    pub(crate) fn fn_props_get(&mut self, closure_op: Operand, key: &str) -> Operand {
        let key_str = self.intern_string_literal(key);
        let res_slot = self.alloca_in_entry(Type::Any, Some("__fnprops_get"));
        let props = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::Ptr, closure_op, CLOSURE_PROPS_OFF),
            Type::Ptr,
            None,
        );
        let is_null = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(IPred::Eq, Operand::Value(props), Operand::ConstPtrNull),
            Type::Bool,
            None,
        );
        let null_blk = self.f.add_block();
        let read_blk = self.f.add_block();
        let after = self.f.add_block();
        let cb0 = self.cur_block;
        self.f.set_term(
            cb0,
            Terminator::CondBr {
                cond: Operand::Value(is_null),
                then_blk: null_blk,
                else_blk: read_blk,
            },
        );
        // null path: undef box.
        self.cur_block = null_blk;
        let undef_box = self.f.append_inst(
            self.cur_block,
            InstKind::Call(
                self.intrinsics.any_box,
                vec![Operand::ConstI64(5), Operand::ConstI64(0)],
            ),
            Type::Any,
            None,
        );
        self.f.append_void(
            null_blk,
            InstKind::Store(Operand::Value(undef_box), Operand::Value(res_slot), 0),
        );
        self.f.set_term(null_blk, Terminator::Br(after));
        // read path: dynobj_get_tag/value + box.
        self.cur_block = read_blk;
        let tag = self.f.append_inst(
            self.cur_block,
            InstKind::Call(
                self.intrinsics.dynobj_get_tag,
                vec![Operand::Value(props), Operand::Value(key_str)],
            ),
            Type::I64,
            None,
        );
        let value = self.f.append_inst(
            self.cur_block,
            InstKind::Call(
                self.intrinsics.dynobj_get_value,
                vec![Operand::Value(props), Operand::Value(key_str)],
            ),
            Type::I64,
            None,
        );
        let box_v = crate::ssa_lower_accessor::emit_dynobj_get_result(self, tag, value);
        self.f.append_void(
            self.cur_block,
            InstKind::Store(box_v.clone(), Operand::Value(res_slot), 0),
        );
        let rb = self.cur_block;
        self.f.set_term(rb, Terminator::Br(after));
        self.cur_block = after;
        let r = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::Any, Operand::Value(res_slot), 0),
            Type::Any,
            None,
        );
        Operand::Value(r)
    }

    pub(crate) fn box_to_any(&mut self, val: Operand) -> Operand {
        let val_ty = self.operand_ty(&val);
        let (tag, value_op): (i64, Operand) = match val_ty {
            Type::I64 | Type::I32 => (2, val),
            Type::F64 => {
                let bits = self.f.append_inst(
                    self.cur_block,
                    InstKind::BitCastF64ToI64(val),
                    Type::I64,
                    None,
                );
                (3, Operand::Value(bits))
            }
            Type::Bool => {
                let zext = self.f.append_inst(
                    self.cur_block,
                    InstKind::ZExtBoolToI64(val),
                    Type::I64,
                    None,
                );
                (1, Operand::Value(zext))
            }
            _ if val_ty.is_refcounted() => {
                // Heap-typed value: pass the ptr as i64. The any_box
                // helper bumps its refcount internally so the box's
                // drop balances. ABI-compatible because ptr ↔ i64
                // share the same machine word.
                (4, val)
            }
            Type::Ptr => {
                // P3.2 — distinguish ConstPtrNull (the lowered `null`
                // literal) from a generic Ptr value (e.g. a dynobj
                // alloc result). Pre-P3.2 box_to_any treated all
                // Ptrs as ANY_NULL, which silently dropped dynobj
                // ptrs and made `let x: any = {}; x.foo` always
                // return undefined. Now ConstPtrNull → ANY_NULL=0;
                // any other Ptr → ANY_HEAP=4 with the ptr as value.
                if matches!(val, Operand::ConstPtrNull) {
                    (0, Operand::ConstI64(0))
                } else {
                    (4, val)
                }
            }
            other => panic!("ssa-lower: box_to_any element type {other:?} not supported"),
        };
        let v = self.f.append_inst(
            self.cur_block,
            InstKind::Call(
                self.intrinsics.any_box,
                vec![Operand::ConstI64(tag), value_op],
            ),
            Type::Any,
            None,
        );
        Operand::Value(v)
    }

    /// Inverse of `box_to_any` for heap-payload reads — decode an
    /// AnyBox-encoded `Type::Any` operand back to its boxed-payload
    /// pointer. Emits the `any_unbox_value` shim call (returning the
    /// i64 value field) plus an IntToPtr cast, so callers stay
    /// decoupled from the AnyBox struct layout (Step 7d's NaN-box
    /// switch only has to swap the shim impl).
    pub(crate) fn any_unbox_value_as_ptr(&mut self, obj: Operand) -> ValueId {
        let raw = self.f.append_inst(
            self.cur_block,
            InstKind::Call(self.intrinsics.any_unbox_value, vec![obj]),
            Type::I64,
            None,
        );
        self.f.append_inst(
            self.cur_block,
            InstKind::IntToPtr(Operand::Value(raw)),
            Type::Ptr,
            None,
        )
    }

    /// Step 7d-A — `dynobj_set` / `dynobj_define` may resize +
    /// relocate the underlying heap block (`*obj_slot` updated).
    /// The variable's AnyValue still holds the OLD ptr; if the
    /// receiver was a named Ident, reload the post-resize ptr and
    /// store it back as a fresh NaN-box `AnyValue`. NaN-box Cell
    /// encoding is `ptr as u64` (identical bits — the PtrToInt +
    /// IntToPtr cast is a no-op at LLVM IR; LTO collapses them
    /// into the same SSA value). Non-Ident receivers (e.g.
    /// `arr[i].x = v`) don't have a hoisted slot; the resize-time
    /// dangling is a follow-up patch (no current conformance
    /// fixture exercises it under the 7/8 load factor +
    /// `INITIAL_CAP=8`).
    pub(crate) fn emit_any_dynobj_writeback(
        &mut self,
        obj_ident: &Option<String>,
        dynobj_slot: ValueId,
    ) {
        let Some(name) = obj_ident else {
            return;
        };
        let Some(info) = self.locals.get(name).copied() else {
            return;
        };
        if !matches!(info.ty, Type::Any) {
            return;
        }
        let new_dynobj = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::Ptr, Operand::Value(dynobj_slot), 0),
            Type::Ptr,
            None,
        );
        let new_dynobj_as_i64 = self.f.append_inst(
            self.cur_block,
            InstKind::PtrToInt(Operand::Value(new_dynobj)),
            Type::I64,
            None,
        );
        let new_any = self.f.append_inst(
            self.cur_block,
            InstKind::IntToPtr(Operand::Value(new_dynobj_as_i64)),
            Type::Any,
            None,
        );
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::Value(new_any), Operand::Value(info.slot), 0),
        );
    }

    /// array pointer as Operand::Value.
    pub(crate) fn lower_array_any_literal(&mut self, ids: &[ExprId]) -> Operand {
        // P5.6 — spreads inside Array<Any> literals walk through
        // arr_extend_any (which understands the 16-byte tagged
        // slot layout); non-spread items still push tag/value
        // pairs via arr_push_any. The arr_alloc_any size hint is
        // the literal-count (spreads grow via realloc — same
        // strategy as push's growth on overflow).
        let arr_id = intern_arr_layout(self.arr_layouts, Type::Any);
        let literal_count: i64 = ids
            .iter()
            .filter(|id| !matches!(self.ast.get_expr(**id), Expr::Spread { .. }))
            .count() as i64;
        let mut arr = self.f.append_inst(
            self.cur_block,
            InstKind::Call(
                self.intrinsics.arr_alloc_any,
                vec![Operand::ConstI64(literal_count)],
            ),
            Type::Arr(arr_id),
            None,
        );
        for &eid in ids {
            // P5.6 — spread item routes through arr_extend_any.
            // Inner must lower to Type::Arr(any_arr_id); typed
            // Array<T> spread into Array<Any> needs per-elem box
            // (defer; reject with subset-boundary msg).
            if let Expr::Spread { expr: inner } = self.ast.get_expr(eid) {
                let inner_eid = *inner;
                let mut src_op = self.lower_expr(inner_eid);
                let mut src_ty = self.operand_ty(&src_op);
                // S141 — `[...set]` inside Array<Any> literal: route
                // through the shared Array.from(set) helper to land an
                // Arr<Any> the existing arr_extend_any path can splice.
                if matches!(src_ty, Type::Set) {
                    src_op = crate::ssa_lower_arr_from_set::emit(self, src_op);
                    src_ty = self.operand_ty(&src_op);
                }
                let inner_is_any_arr = match src_ty {
                    Type::Arr(src_arr_id) => {
                        matches!(self.arr_layouts[src_arr_id.0 as usize], Type::Any)
                    }
                    _ => false,
                };
                if !inner_is_any_arr {
                    panic!(
                        "ssa-lower: spread of {src_ty:?} into Array<Any> literal not yet supported (P5.6 subset — Array<Any> spread only; typed-Array spread into Any requires per-elem box, follow-up)"
                    );
                }
                let new_arr = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(
                        self.intrinsics.arr_extend_any,
                        vec![Operand::Value(arr), src_op],
                    ),
                    Type::Arr(arr_id),
                    None,
                );
                arr = new_arr;
                continue;
            }
            // Nested Array literal: recurse so the inner array is also
            // Arr<Any> (per-slot NaN-box). Without this, lower_expr routes
            // through the typed Arr<T> fast path and the outer slot's
            // ANY_HEAP unwrap exposes raw 8-byte int slots that
            // __torajs_arr_print_any decodes as NaN-box AnyValues → deref
            // `1` SIGSEGV on `[[1,2],[3,4]]`. Same root as the LetDecl
            // `let x: any = [...]` arm above.
            if let Expr::Array(inner_ids) = self.ast.get_expr(eid) {
                let inner_eids: Vec<ExprId> = inner_ids.clone();
                let inner_arr = self.lower_array_any_literal(&inner_eids);
                self.emit_rc_inc(inner_arr.clone());
                arr = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(
                        self.intrinsics.arr_push_any,
                        vec![Operand::Value(arr), Operand::ConstI64(4), inner_arr],
                    ),
                    Type::Arr(arr_id),
                    None,
                );
                continue;
            }
            let val = self.lower_expr(eid);
            let val_ty = self.operand_ty(&val);
            // ANY_NULL=0, ANY_BOOL=1, ANY_I64=2, ANY_F64=3, ANY_HEAP=4
            // (matches __TORAJS_ANY_* in runtime_str.c).
            let (tag, value_op): (i64, Operand) = match val_ty {
                Type::I64 | Type::I32 => (2, val),
                Type::F64 => {
                    // T-10.d.ii — pun f64 bits to i64 so push_any
                    // (i64 third param) carries them exactly.
                    // print_any reverses the bitcast at decode time.
                    let bits = self.f.append_inst(
                        self.cur_block,
                        InstKind::BitCastF64ToI64(val),
                        Type::I64,
                        None,
                    );
                    (3, Operand::Value(bits))
                }
                Type::Bool => {
                    let zext = self.f.append_inst(
                        self.cur_block,
                        InstKind::ZExtBoolToI64(val),
                        Type::I64,
                        None,
                    );
                    (1, Operand::Value(zext))
                }
                _ if val_ty.is_refcounted() => {
                    // Heap-typed value: rc_inc to hold an owning ref
                    // for the array slot. push_any's third param is
                    // i64 in the SSA decl; LLVM treats ptr ↔ i64 as
                    // ABI-compatible (same machine word), so passing
                    // the ptr operand directly works at the call site
                    // without an explicit PtrToInt SSA op (which the
                    // current InstKind enum doesn't expose). Drop
                    // walks via __torajs_arr_drop_any when the array
                    // dies.
                    self.emit_rc_inc(val.clone());
                    (4, val)
                }
                Type::Ptr => {
                    // Ptr that's null (Type::Null lowers to ConstPtrNull
                    // → Type::Ptr). Tag as ANY_NULL with value 0.
                    // S127-1: `undefined` literal also lowers to
                    // ConstPtrNull (Type::Ptr). Recover the original
                    // AST shape so the slot tags ANY_UNDEF=5, else
                    // `[undefined]` collapses to `[null]` and
                    // strict-eq / .indexOf(undefined) mis-fires.
                    // Same root as W-D narrow trunk's box_to_any
                    // ConstPtrNull arm (S126-1/-3).
                    if matches!(
                        self.ast.get_expr(eid),
                        Expr::Ident(n) if n == "undefined"
                    ) {
                        (5, Operand::ConstI64(0))
                    } else {
                        (0, Operand::ConstI64(0))
                    }
                }
                other => panic!(
                    "not yet supported: lower_array_any_literal element type {other:?} \
                     (T-10.d will add F64 + boxed-primitive coverage)"
                ),
            };
            arr = self.f.append_inst(
                self.cur_block,
                InstKind::Call(
                    self.intrinsics.arr_push_any,
                    vec![Operand::Value(arr), Operand::ConstI64(tag), value_op],
                ),
                Type::Arr(arr_id),
                None,
            );
        }
        Operand::Value(arr)
    }

    /// v0.3 #4 D-3 — outer wrapper that stamps every Inst emitted
    /// while lowering `eid` with `current_origin = Some(eid)` so
    /// debug-info emission can resolve the source span for DWARF.
    /// Recursive `self.lower_expr(...)` calls re-enter this wrapper
    /// so nested exprs get their own tighter origin scoped to the
    /// inner subtree (RAII-style save/restore on the prev value).
    pub(crate) fn lower_expr(&mut self, eid: ExprId) -> Operand {
        let prev = self.f.current_origin;
        self.f.current_origin = Some(eid);
        let result = self.lower_expr_inner(eid);
        self.f.current_origin = prev;
        result
    }

    fn lower_expr_inner(&mut self, eid: ExprId) -> Operand {
        crate::ssa_lower_expr_inner::lower(self, eid)
    }

    /// Type of the value produced by an operand. For SSA-Value operands this
    /// is the function's value-table lookup; for constants it's implied by
    /// the constant flavor.
    pub(crate) fn operand_ty(&self, op: &Operand) -> Type {
        match op {
            Operand::Value(v) => self.f.value_type(*v),
            Operand::ConstI64(_) => Type::I64,
            Operand::ConstI32(_) => Type::I32,
            Operand::ConstF64(_) => Type::F64,
            Operand::ConstBool(_) => Type::Bool,
            // null is intentionally untyped at this layer — the
            // surrounding context (Store slot type, Call arg type)
            // determines what pointer shape it lands in. Returning Ptr
            // here is the safe default; callers that need a more
            // specific Type::Str / Type::Obj / etc. read it from the
            // sink instead.
            Operand::ConstPtrNull => Type::Ptr,
        }
    }

    /// Widen a Bool / i1 operand to the i64-shaped slot used by uniform
    /// runtime helpers (array push, object field store, hashmap value,
    /// throw_value). Constants are rewritten in place; SSA values go
    /// through an explicit `ZExtBoolToI64` instruction. No-op when the
    /// operand is already i64-shaped.
    pub(crate) fn coerce_bool_to_i64(&mut self, op: Operand) -> Operand {
        match self.operand_ty(&op) {
            Type::Bool => match op {
                Operand::ConstBool(b) => Operand::ConstI64(if b { 1 } else { 0 }),
                Operand::Value(_) => {
                    let v = self.f.append_inst(
                        self.cur_block,
                        InstKind::ZExtBoolToI64(op),
                        Type::I64,
                        None,
                    );
                    Operand::Value(v)
                }
                _ => op,
            },
            _ => op,
        }
    }

    /// Truncate an f64 operand to i64. Mirrors `coerce_to_f64` for the
    /// reverse direction — used at call sites whose runtime intrinsic
    /// expects an integer parameter (Math.imul, Math.clz32) but the
    /// caller may have passed a float literal or a Math.* result.
    /// Constants fold in place; value operands emit `InstKind::FpToSi`.
    pub(crate) fn coerce_to_i64(&mut self, op: Operand) -> Operand {
        match self.operand_ty(&op) {
            Type::I64 => op,
            Type::Bool => self.coerce_bool_to_i64(op),
            Type::F64 => match op {
                // V3-18 m2.d follow-up — JS spec ToInteger:
                //   NaN  → 0
                //   ±Inf → ±Infinity (here represented by i64::MAX /
                //          i64::MIN — preserves sign through any
                //          downstream "from >= len" / "from + len"
                //          clamp logic).
                // Without this, FpToSi on non-finite is poison and
                // downstream loops get stuck or read garbage.
                Operand::ConstF64(n) => {
                    if n.is_nan() {
                        Operand::ConstI64(0)
                    } else if n == f64::INFINITY {
                        Operand::ConstI64(i64::MAX)
                    } else if n == f64::NEG_INFINITY {
                        Operand::ConstI64(i64::MIN)
                    } else {
                        Operand::ConstI64(n as i64)
                    }
                }
                Operand::Value(_) => {
                    let v =
                        self.f
                            .append_inst(self.cur_block, InstKind::FpToSi(op), Type::I64, None);
                    Operand::Value(v)
                }
                _ => op,
            },
            other => panic!("ssa-lower: cannot coerce {other:?} to i64"),
        }
    }

    /// V3-18 m1.h.40 — JS spec §7.1.6 ToInt32 for the bitwise-on-Number
    /// path. Constants fold at compile time (NaN / ±Inf → 0; finite
    /// truncates towards zero); SSA values use FpToSi (matching the
    /// finite-in-i32-range behavior LLVM gives, with NaN / OOB
    /// landing as poison — same as v8 / jsc in practice for the
    /// integer bitwise idioms we exercise).
    pub(crate) fn coerce_f64_to_i64_for_bitwise(&mut self, op: Operand) -> Operand {
        match self.operand_ty(&op) {
            Type::I64 => op,
            Type::Bool => self.coerce_bool_to_i64(op),
            Type::F64 => match op {
                Operand::ConstF64(n) => {
                    if !n.is_finite() {
                        Operand::ConstI64(0)
                    } else {
                        Operand::ConstI64(n as i64)
                    }
                }
                Operand::Value(_) => {
                    let v =
                        self.f
                            .append_inst(self.cur_block, InstKind::FpToSi(op), Type::I64, None);
                    Operand::Value(v)
                }
                _ => op,
            },
            other => panic!("ssa-lower: cannot coerce {other:?} to i64 for bitwise"),
        }
    }

    /// JS spec §7.1.6 ToInt32 normalization on tora's i64 value model:
    /// sign-extend the operand's low 32 bits (`shl 32` + `ashr 32` —
    /// LLVM folds the pair to a single sxtw on arm64). Bitwise/shift
    /// operators apply this to each operand (and to the Shl result,
    /// which can carry past bit 31) so `1 << 31` wraps negative and
    /// `4294967296 | 0` truncates to 0 exactly like v8 / jsc.
    pub(crate) fn emit_to_int32(&mut self, op: Operand) -> Operand {
        match op {
            Operand::ConstI64(n) => Operand::ConstI64(n as i32 as i64),
            _ => {
                let hi = self.bin(SsaBinOp::Shl, op, Operand::ConstI64(32), Type::I64);
                self.bin(SsaBinOp::AShr, hi, Operand::ConstI64(32), Type::I64)
            }
        }
    }

    /// JS spec §13.9 — shift counts take `ToUint32(b) & 31`, which is
    /// exactly the low 5 bits of the i64 operand (two's complement
    /// agrees for negative counts: `-3 & 31 == 29 == ToUint32(-3) & 31`).
    fn emit_shift_count(&mut self, op: Operand) -> Operand {
        match op {
            Operand::ConstI64(n) => Operand::ConstI64(n & 31),
            _ => self.bin(SsaBinOp::And, op, Operand::ConstI64(31), Type::I64),
        }
    }

    /// Shared int32-semantics lowering for the six bitwise/shift
    /// operators on Number (the all-i64 and the mixed-f64 binop paths
    /// both land here; f64 operands first truncate via
    /// `coerce_f64_to_i64_for_bitwise`). Per JS spec §13.9 / §13.12 each
    /// operand is ToInt32-normalized (ToUint32 for `>>>`), the op runs
    /// at 32-bit width, and the result sign-extends back — emitted as
    /// explicit i64 SSA insts so every downstream pass (egraph
    /// const-fold included) stays a plain i64-semantics transform.
    pub(crate) fn lower_bitwise_int32(&mut self, op: AstBinOp, a: Operand, b: Operand) -> Operand {
        let ai = self.coerce_f64_to_i64_for_bitwise(a);
        let bi = self.coerce_f64_to_i64_for_bitwise(b);
        match op {
            // And/Or/Xor of two sign-extended-32 values is itself
            // sign-extended-32 — no post-normalization needed.
            AstBinOp::BitAnd => {
                let a32 = self.emit_to_int32(ai);
                let b32 = self.emit_to_int32(bi);
                self.bin(SsaBinOp::And, a32, b32, Type::I64)
            }
            AstBinOp::BitOr => {
                let a32 = self.emit_to_int32(ai);
                let b32 = self.emit_to_int32(bi);
                self.bin(SsaBinOp::Or, a32, b32, Type::I64)
            }
            AstBinOp::BitXor => {
                let a32 = self.emit_to_int32(ai);
                let b32 = self.emit_to_int32(bi);
                self.bin(SsaBinOp::Xor, a32, b32, Type::I64)
            }
            // `a << b` can carry past bit 31 — re-normalize the result.
            AstBinOp::Shl => {
                let a32 = self.emit_to_int32(ai);
                let cnt = self.emit_shift_count(bi);
                let r = self.bin(SsaBinOp::Shl, a32, cnt, Type::I64);
                self.emit_to_int32(r)
            }
            // ashr of a sign-extended-32 value stays sign-extended-32.
            AstBinOp::Shr => {
                let a32 = self.emit_to_int32(ai);
                let cnt = self.emit_shift_count(bi);
                self.bin(SsaBinOp::AShr, a32, cnt, Type::I64)
            }
            // `>>>` is ToUint32: zero-extend the low 32 bits, then
            // logical-shift — result is in [0, 2^32), non-negative.
            AstBinOp::UShr => {
                let mask32 = self.bin(SsaBinOp::And, ai, Operand::ConstI64(0xFFFF_FFFF), Type::I64);
                let cnt = self.emit_shift_count(bi);
                self.bin(SsaBinOp::LShr, mask32, cnt, Type::I64)
            }
            other => unreachable!("lower_bitwise_int32: non-bitwise op {other:?}"),
        }
    }

    /// Promote an i64 operand to f64. Constants are rewritten in place
    /// (cheaper than emitting a sitofp instruction LLVM would constant-fold
    /// anyway). Value operands emit an explicit InstKind::SiToFp.
    /// W4 — raw-slot intrinsic argument: array slots are 8 raw bytes
    /// and the `__torajs_arr_*` helpers take them as i64. An f64
    /// value must cross as explicit bits — passing an FPR value to an
    /// i64 param is codegen-ambiguous (the baseline tier reads the
    /// wrong register class; LLVM IR type-mismatches).
    pub(crate) fn raw_slot_arg(&mut self, val: Operand) -> Operand {
        if self.operand_ty(&val) != Type::F64 {
            return val;
        }
        match val {
            Operand::ConstF64(x) => Operand::ConstI64(x.to_bits() as i64),
            _ => Operand::Value(self.f.append_inst(
                self.cur_block,
                InstKind::BitCastF64ToI64(val),
                Type::I64,
                None,
            )),
        }
    }

    pub(crate) fn coerce_to_f64(&mut self, op: Operand) -> Operand {
        match self.operand_ty(&op) {
            Type::F64 => op,
            Type::I64 => match op {
                Operand::ConstI64(n) => Operand::ConstF64(n as f64),
                Operand::Value(_) => {
                    let v =
                        self.f
                            .append_inst(self.cur_block, InstKind::SiToFp(op), Type::F64, None);
                    Operand::Value(v)
                }
                _ => op,
            },
            other => panic!("ssa-lower: cannot coerce {other:?} to f64"),
        }
    }

    /// P7.2b — coerce an Any operand to a concrete numeric: JS spec
    /// §7.1.4 ToNumber via the one `__torajs_any_to_number` runtime
    /// helper, then narrowed to `target` (F64 as-is, or I64 via the
    /// existing F64→i64 ToInteger path). Single place for the
    /// Any→number sink so Stmt::Return and Assign can't drift apart
    /// (mirrors coerce_to_bool's `Type::Any => any_to_bool`
    /// precedent). Caller guarantees `operand_ty(op) == Type::Any`
    /// and `target` ∈ {I64, F64}.
    pub(crate) fn coerce_any_to_number(&mut self, op: Operand, target: Type) -> Operand {
        let num = Operand::Value(self.f.append_inst(
            self.cur_block,
            InstKind::Call(self.intrinsics.any_to_number, vec![op]),
            Type::F64,
            None,
        ));
        if target == Type::F64 {
            num
        } else {
            self.coerce_to_i64(num)
        }
    }

    /// Type-aware BinOp lowering. Decision rule:
    ///   - `/` always produces f64. Both operands coerced to f64. (Use `>>`
    ///     or explicit conversion for integer division — see collatz.tora.ts
    ///     for the convention.)
    ///   - Otherwise: if either operand is f64, both coerced to f64 and
    ///     a float-flavored op is emitted (FAdd/FSub/FMul, FCmp).
    ///   - Bitwise ops + Mod stay integer-only; mixing them with f64 is a
    ///     type error (caught at lower-time, not tolerated).
    /// Emit inline byte-by-byte `Str === &[u8]` comparison. Returns a
    /// bool Operand. Walks bytes [0..bytes.len()) of `other`; first
    /// mismatch short-circuits to false. For len=0 just returns
    /// `len(other) == 0`.
    ///
    /// Skips the `__torajs_str_eq` C-runtime fn-call (which lives in
    /// a separately-compiled module so LLVM can't inline it). For tiny
    /// literals (1-2 bytes) this unrolls to a few cycles; for longer
    /// (up to caller-defined cap) LLVM's loop opts often collapse to
    /// a single wide load + cmp.
    /// Compute the byte-data location for a Str / Substr operand,
    /// returned as `(base_ptr, byte_offset_into_base)`. Caller uses
    /// LoadDyn(type, base_ptr, total_offset) where total_offset =
    /// base_byte_offset + per-byte index.
    ///
    /// For OWNED Str: `(self, 16)` — bytes inline at self+16.
    /// For Substr: `(parent_ptr, STR_HDR(16) + offset)` — the parent's
    ///   bytes start at parent+16, view starts at parent+16+offset.
    /// Returns `(base_ptr, base_offset_value_or_const)`.
    pub(crate) fn emit_str_data_base(&mut self, op: Operand, ty: Type) -> (Operand, Operand) {
        match ty {
            Type::Str => (op, Operand::ConstI64(16)),
            Type::Substr => {
                let parent = self.f.append_inst(
                    self.cur_block,
                    InstKind::Load(Type::Ptr, op, 16),
                    Type::Ptr,
                    None,
                );
                let offset = self.f.append_inst(
                    self.cur_block,
                    InstKind::Load(Type::I64, op, 24),
                    Type::I64,
                    None,
                );
                // 16 + offset → byte offset into parent
                let total_off = self.f.append_inst(
                    self.cur_block,
                    InstKind::BinOp(SsaBinOp::Add, Operand::Value(offset), Operand::ConstI64(16)),
                    Type::I64,
                    None,
                );
                (Operand::Value(parent), Operand::Value(total_off))
            }
            other => panic!("emit_str_data_base: unsupported type {other:?}"),
        }
    }

    pub(crate) fn emit_inline_str_eq_bytes(&mut self, other: Operand, bytes: &[u8]) -> Operand {
        // For Substr we still load len at offset 8 (same as Str), but
        // bytes are accessed via (parent_data + offset). Compute the
        // data pointer once per call, then per-byte loads use it.
        let other_ty = self.operand_ty(&other);
        let result_slot = self.alloca_in_entry(Type::Bool, Some("__streq_r"));
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::ConstBool(false), Operand::Value(result_slot), 0),
        );
        let done_blk = self.f.add_block();
        // step 1: len-eq. Str/Substr layout fork lives in the
        // ssa_lower_str sidekick — see load_str_or_substr_length.
        let other_len = match crate::ssa_lower_str::load_str_or_substr_length(self, other, other_ty)
        {
            Operand::Value(v) => v,
            _ => unreachable!("length helper always yields a Value"),
        };
        let len_eq = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(
                IPred::Eq,
                Operand::Value(other_len),
                Operand::ConstI64(bytes.len() as i64),
            ),
            Type::Bool,
            None,
        );
        let cmp_blk = self.f.add_block();
        self.f.set_term(
            self.cur_block,
            Terminator::CondBr {
                cond: Operand::Value(len_eq),
                then_blk: cmp_blk,
                else_blk: done_blk,
            },
        );
        self.cur_block = cmp_blk;
        if bytes.is_empty() {
            // len-eq alone determines truth.
            self.f.append_void(
                self.cur_block,
                InstKind::Store(Operand::ConstBool(true), Operand::Value(result_slot), 0),
            );
            self.f.set_term(self.cur_block, Terminator::Br(done_blk));
        } else {
            // Compute (base_ptr, base_offset) once. For Str: (self, 16) —
            // const-folded immediate. For Substr: 2 loads + 1 add to
            // resolve parent + 16 + view_offset, amortized over chain.
            let (base, base_off) = self.emit_str_data_base(other, other_ty);
            let mut chain: Vec<BlockId> = Vec::with_capacity(bytes.len() + 1);
            chain.push(self.cur_block);
            for _ in 0..bytes.len() {
                chain.push(self.f.add_block());
            }
            for (i, &want_byte) in bytes.iter().enumerate() {
                self.cur_block = chain[i];
                // total_off = base_off + i, then LoadDyn 4 bytes.
                // For Str (base_off = const 16) the add folds; for
                // Substr the add stays but i is small const.
                let off_i = self.f.append_inst(
                    self.cur_block,
                    InstKind::BinOp(SsaBinOp::Add, base_off, Operand::ConstI64(i as i64)),
                    Type::I64,
                    None,
                );
                let byte_v = self.f.append_inst(
                    self.cur_block,
                    InstKind::LoadDyn(Type::I32, base, Operand::Value(off_i)),
                    Type::I32,
                    None,
                );
                let byte_lo = self.f.append_inst(
                    self.cur_block,
                    InstKind::BinOp(
                        SsaBinOp::And,
                        Operand::Value(byte_v),
                        Operand::ConstI32(0xff),
                    ),
                    Type::I32,
                    None,
                );
                let eq = self.f.append_inst(
                    self.cur_block,
                    InstKind::ICmp(
                        IPred::Eq,
                        Operand::Value(byte_lo),
                        Operand::ConstI32(want_byte as i32),
                    ),
                    Type::Bool,
                    None,
                );
                self.f.set_term(
                    self.cur_block,
                    Terminator::CondBr {
                        cond: Operand::Value(eq),
                        then_blk: chain[i + 1],
                        else_blk: done_blk,
                    },
                );
            }
            self.cur_block = chain[bytes.len()];
            self.f.append_void(
                self.cur_block,
                InstKind::Store(Operand::ConstBool(true), Operand::Value(result_slot), 0),
            );
            self.f.set_term(self.cur_block, Terminator::Br(done_blk));
        }
        self.cur_block = done_blk;
        let r = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::Bool, Operand::Value(result_slot), 0),
            Type::Bool,
            None,
        );
        Operand::Value(r)
    }

    /// Perf fast-path for `expr === "literal"` / `expr !== "literal"`
    /// where the literal is short (≤16 bytes). Returns `None` if the
    /// pattern doesn't match (caller falls through to the generic
    /// str_eq path). For switch-on-string the equivalent inline emit
    /// happens directly inside `Stmt::Switch` lowering — see there.
    /// V3-18 m2.e — fold `<prim>.constructor === <Ctor>` /
    /// `<Ctor> === <prim>.constructor` at AST level. Used as a
    /// pre-lower pattern match since tora has no first-class
    /// function ref for namespace ctors (bare `Number` / `String`
    /// etc can't be lowered as a value).
    pub(crate) fn try_fold_constructor_eq(
        &mut self,
        op: AstBinOp,
        left: ExprId,
        right: ExprId,
    ) -> Option<Operand> {
        // Identify a (prim_constructor_member, ctor_ident) pair
        // regardless of which side is which.
        fn prim_type_tag(t: Type) -> Option<&'static str> {
            match t {
                Type::I64 | Type::F64 | Type::I32 => Some("Number"),
                Type::Str | Type::Substr => Some("String"),
                Type::Bool => Some("Boolean"),
                Type::BigInt => Some("BigInt"),
                Type::Symbol => Some("Symbol"),
                Type::Obj(_) => Some("Object"),
                Type::Arr(_) => Some("Array"),
                _ => None,
            }
        }
        let l_expr = self.ast.get_expr(left).clone();
        let r_expr = self.ast.get_expr(right).clone();
        let (member_expr, ctor_name): (Expr, String) = match (l_expr, r_expr) {
            (Expr::Member { obj, name }, Expr::Ident(c)) if name == "constructor" => {
                (self.ast.get_expr(obj).clone(), c)
            }
            (Expr::Ident(c), Expr::Member { obj, name }) if name == "constructor" => {
                (self.ast.get_expr(obj).clone(), c)
            }
            _ => return None,
        };
        // Resolve the receiver's static SSA type.
        let recv_ty = match member_expr {
            Expr::Ident(ref n) => {
                let info = self.locals.get(n)?;
                info.ty
            }
            _ => return None,
        };
        let actual = prim_type_tag(recv_ty)?;
        let matches_ctor = actual == ctor_name.as_str();
        let result = match op {
            AstBinOp::Eq => matches_ctor,
            AstBinOp::Neq => !matches_ctor,
            _ => return None,
        };
        Some(Operand::ConstBool(result))
    }

    pub(crate) fn try_inline_str_eq_with_literal(
        &mut self,
        op: AstBinOp,
        left: ExprId,
        right: ExprId,
    ) -> Option<Operand> {
        let (lit_bytes, other_eid) = match (
            self.ast.get_expr(left).clone(),
            self.ast.get_expr(right).clone(),
        ) {
            (Expr::String(s), _) => (s.into_bytes(), right),
            (_, Expr::String(s)) => (s.into_bytes(), left),
            _ => return None,
        };
        if lit_bytes.len() > 16 {
            return None;
        }
        // P11.1-S2.3 — the inline `str === <literal>` path
        // compares the runtime Str's `length` field against
        // `lit_bytes.len()` (the literal's UTF-8 byte count) and
        // then walks the runtime Str's payload byte-by-byte
        // against the literal. Post-S2 the runtime Str's
        // `length` is a code unit count, not a byte count: for
        // an ASCII-only Latin-1 payload code unit == byte so
        // both numbers + bytes coincide, but for any non-ASCII
        // codepoint they diverge (UTF-16 Str length is half its
        // byte count; the literal's UTF-8 byte length doesn't
        // match the Latin-1 byte length either). Bail out of
        // the inline arm whenever the literal carries any byte
        // > 0x7F so the runtime `__torajs_str_eq` does the
        // encoding-aware compare instead.
        if lit_bytes.iter().any(|&b| b > 0x7F) {
            return None;
        }
        let other = self.lower_expr(other_eid);
        let other_ty = self.operand_ty(&other);
        if other_ty != Type::Str && other_ty != Type::Substr {
            return None;
        }
        let r = self.emit_inline_str_eq_bytes(other, &lit_bytes);
        // For !==, flip via xor.
        if matches!(op, AstBinOp::Neq) {
            let r_v = match r {
                Operand::Value(v) => v,
                _ => unreachable!(),
            };
            let n = self.f.append_inst(
                self.cur_block,
                InstKind::BinOp(SsaBinOp::Xor, Operand::Value(r_v), Operand::ConstBool(true)),
                Type::Bool,
                None,
            );
            Some(Operand::Value(n))
        } else {
            Some(r)
        }
    }

    /// P1.5/P1.8 — peek a binop operand's source ExprId to see if its
    /// frontend type is Type::Undefined. Set by callers that have
    /// the AST in hand (currently the Eq/Neq path in lower_expr).
    /// None means "no info — treat as null per old behavior". The
    /// pair is `(left_id, right_id)`. Cleared after each lower_binop
    /// call so it doesn't leak across unrelated dispatches.
    fn lower_binop(&mut self, op: AstBinOp, a: Operand, b: Operand) -> Operand {
        self.lower_binop_with_ids(op, a, b, None, None)
    }

    pub(crate) fn lower_binop_with_ids(
        &mut self,
        op: AstBinOp,
        a: Operand,
        b: Operand,
        left_id: Option<ExprId>,
        right_id: Option<ExprId>,
    ) -> Operand {
        let saved_left = self.binop_left_undef_id.take();
        let saved_right = self.binop_right_undef_id.take();
        let saved_square = self.binop_mul_square;
        self.binop_mul_square = matches!(op, AstBinOp::Mul)
            && matches!(
                (
                    left_id.map(|e| self.ast.get_expr(e)),
                    right_id.map(|e| self.ast.get_expr(e)),
                ),
                (Some(Expr::Ident(l)), Some(Expr::Ident(r))) if l == r
            );
        self.binop_left_undef_id = left_id.filter(|eid| {
            matches!(
                self.expr_types.get(eid),
                Some(crate::check::Type::Undefined)
            )
        });
        self.binop_right_undef_id = right_id.filter(|eid| {
            matches!(
                self.expr_types.get(eid),
                Some(crate::check::Type::Undefined)
            )
        });
        let r = self.lower_binop_inner(op, a, b);
        self.binop_left_undef_id = saved_left;
        self.binop_right_undef_id = saved_right;
        self.binop_mul_square = saved_square;
        r
    }

    fn lower_binop_inner(&mut self, op: AstBinOp, a: Operand, b: Operand) -> Operand {
        crate::ssa_lower_binop_inner::lower(self, op, a, b)
    }

    // (`lower_logical_and` / `lower_logical_or` live in
    // `ssa_lower_logical.rs`.)

    // (`coerce_to_bool` lives in `ssa_lower_logical.rs`.)

    pub(crate) fn bin(&mut self, op: SsaBinOp, a: Operand, b: Operand, ty: Type) -> Operand {
        let v = self
            .f
            .append_inst(self.cur_block, InstKind::BinOp(op, a, b), ty, None);
        Operand::Value(v)
    }

    pub(crate) fn cmp(&mut self, pred: IPred, a: Operand, b: Operand) -> Operand {
        let v = self
            .f
            .append_inst(self.cur_block, InstKind::ICmp(pred, a, b), Type::Bool, None);
        Operand::Value(v)
    }

    pub(crate) fn fcmp(&mut self, pred: FPred, a: Operand, b: Operand) -> Operand {
        let v = self
            .f
            .append_inst(self.cur_block, InstKind::FCmp(pred, a, b), Type::Bool, None);
        Operand::Value(v)
    }

    pub(crate) fn resolve_callee(&self, eid: ExprId) -> FuncId {
        match self.ast.get_expr(eid) {
            Expr::Ident(name) => {
                // Resolve direct fn calls: callee Ident matches a global
                // FnDecl. Fn-typed locals are handled BEFORE this in
                // `lower_expr`'s Call arm (CallIndirect path).
                match self.fn_table.get(name) {
                    Some(f) => *f,
                    None => panic!("ssa-lower: unknown function `{name}`"),
                }
            }
            // Member call — currently only `Math.<method>` resolves here.
            // `console.log(...)` is handled by the top-level shortcut in
            // `lower_top_stmt`, so it never reaches here as a regular Call.
            Expr::Member { obj, name } => {
                let is_math = matches!(self.ast.get_expr(*obj), Expr::Ident(n) if n == "Math");
                if is_math {
                    return match name.as_str() {
                        "sqrt" => self.intrinsics.math_sqrt,
                        "abs" => self.intrinsics.math_abs,
                        "floor" => self.intrinsics.math_floor,
                        "ceil" => self.intrinsics.math_ceil,
                        "log" => self.intrinsics.math_log,
                        "exp" => self.intrinsics.math_exp,
                        "pow" => self.intrinsics.math_pow,
                        "min" => self.intrinsics.math_min,
                        "max" => self.intrinsics.math_max,
                        "sign" => self.intrinsics.math_sign,
                        "round" => self.intrinsics.math_round,
                        "trunc" => self.intrinsics.math_trunc,
                        "sin" => self.intrinsics.math_sin,
                        "cos" => self.intrinsics.math_cos,
                        "tan" => self.intrinsics.math_tan,
                        "asin" => self.intrinsics.math_asin,
                        "acos" => self.intrinsics.math_acos,
                        "atan" => self.intrinsics.math_atan,
                        "atan2" => self.intrinsics.math_atan2,
                        "log2" => self.intrinsics.math_log2,
                        "log10" => self.intrinsics.math_log10,
                        "cbrt" => self.intrinsics.math_cbrt,
                        "sinh" => self.intrinsics.math_sinh,
                        "cosh" => self.intrinsics.math_cosh,
                        "tanh" => self.intrinsics.math_tanh,
                        "asinh" => self.intrinsics.math_asinh,
                        "acosh" => self.intrinsics.math_acosh,
                        "atanh" => self.intrinsics.math_atanh,
                        "expm1" => self.intrinsics.math_expm1,
                        "log1p" => self.intrinsics.math_log1p,
                        "imul" => self.intrinsics.math_imul,
                        "clz32" => self.intrinsics.math_clz32,
                        "fround" => self.intrinsics.math_fround,
                        "f16round" => self.intrinsics.math_f16round,
                        "sumPrecise" => self.intrinsics.math_sum_precise,
                        "random" => self.intrinsics.math_random,
                        other => {
                            panic!("ssa-lower: unknown Math method `{other}`")
                        }
                    };
                }
                /* v0.2 #2 — Date.<static>. */
                let is_date = matches!(self.ast.get_expr(*obj), Expr::Ident(n) if n == "Date");
                if is_date {
                    return match name.as_str() {
                        "now" => self.intrinsics.date_now_static,
                        "parse" => self.intrinsics.date_parse_iso,
                        "UTC" => self.intrinsics.date_utc_components,
                        other => panic!("ssa-lower: unknown Date static method `{other}`"),
                    };
                }
                /* v0.3 #1 — fs.<method>. */
                let is_fs = matches!(self.ast.get_expr(*obj), Expr::Ident(n) if n == "fs");
                if is_fs {
                    return match name.as_str() {
                        "readFileSync" => self.intrinsics.fs_read_file_sync,
                        "writeFileSync" => self.intrinsics.fs_write_file_sync,
                        "existsSync" => self.intrinsics.fs_exists_sync,
                        "appendFileSync" => self.intrinsics.fs_append_file_sync,
                        "unlinkSync" => self.intrinsics.fs_unlink_sync,
                        "mkdirSync" => self.intrinsics.fs_mkdir_sync,
                        "readdirSync" => self.intrinsics.fs_readdir_sync,
                        other => panic!("ssa-lower: unknown fs method `{other}`"),
                    };
                }
                /* v0.3 #3 — process.<method>. */
                let is_process =
                    matches!(self.ast.get_expr(*obj), Expr::Ident(n) if n == "process");
                if is_process {
                    return match name.as_str() {
                        "exit" => self.intrinsics.process_exit,
                        "cwd" => self.intrinsics.process_cwd,
                        other => panic!("ssa-lower: unknown process method `{other}`"),
                    };
                }
                /* T-03 (v0.3.0) — process.{stdout, stderr}.write(s)
                 * and process.stdin.read(). The receiver here is a
                 * Member, not an Ident, so dispatch on the inner
                 * Member shape. */
                if let Expr::Member {
                    obj: inner_obj,
                    name: inner_name,
                } = self.ast.get_expr(*obj).clone()
                    && matches!(self.ast.get_expr(inner_obj), Expr::Ident(n) if n == "process")
                {
                    return match (inner_name.as_str(), name.as_str()) {
                        ("stdout", "write") => self.intrinsics.process_stdout_write,
                        ("stderr", "write") => self.intrinsics.process_stderr_write,
                        other => panic!(
                            "ssa-lower: unsupported process.{}.{} call",
                            other.0, other.1
                        ),
                    };
                }
                /* v0.3 #2 — Bun.<method>. Aliases to existing intrinsics. */
                let is_bun = matches!(self.ast.get_expr(*obj), Expr::Ident(n) if n == "Bun");
                if is_bun {
                    return match name.as_str() {
                        "write" => self.intrinsics.fs_write_file_sync,
                        other => panic!("ssa-lower: unknown Bun method `{other}`"),
                    };
                }
                panic!("ssa-lower: unsupported member call shape: {name}")
            }
            other => panic!("ssa-lower: unsupported callee form: {other:?}"),
        }
    }

    pub(crate) fn is_math_unary(&self, fid: FuncId) -> bool {
        fid == self.intrinsics.math_sqrt
            || fid == self.intrinsics.math_abs
            || fid == self.intrinsics.math_floor
            || fid == self.intrinsics.math_ceil
            || fid == self.intrinsics.math_log
            || fid == self.intrinsics.math_exp
            || fid == self.intrinsics.math_sign
            || fid == self.intrinsics.math_round
            || fid == self.intrinsics.math_trunc
            || fid == self.intrinsics.math_sin
            || fid == self.intrinsics.math_cos
            || fid == self.intrinsics.math_tan
            || fid == self.intrinsics.math_asin
            || fid == self.intrinsics.math_acos
            || fid == self.intrinsics.math_atan
            || fid == self.intrinsics.math_log2
            || fid == self.intrinsics.math_log10
            || fid == self.intrinsics.math_cbrt
            || fid == self.intrinsics.math_sinh
            || fid == self.intrinsics.math_cosh
            || fid == self.intrinsics.math_tanh
            || fid == self.intrinsics.math_asinh
            || fid == self.intrinsics.math_acosh
            || fid == self.intrinsics.math_atanh
            || fid == self.intrinsics.math_expm1
            || fid == self.intrinsics.math_log1p
            || fid == self.intrinsics.math_fround
            || fid == self.intrinsics.math_f16round
    }

    pub(crate) fn is_math_binary(&self, fid: FuncId) -> bool {
        fid == self.intrinsics.math_pow
            || fid == self.intrinsics.math_min
            || fid == self.intrinsics.math_max
            || fid == self.intrinsics.math_atan2
    }

    /// M6.2 — call a Closure or FnSig value with a list of args. Used
    /// inside Array.map/filter/reduce/forEach loop bodies (and is the
    /// mirror of the existing inline call-via-Closure / call-via-FnSig
    /// dispatch, packaged for re-use).
    /// Look up a sig's param types from a callable type. Returns None for
    /// non-callable types — callers should already have validated.
    fn sig_param_tys(&self, fn_ty: Type) -> Option<Vec<Type>> {
        let sig_id = match fn_ty {
            Type::FnSig(s) | Type::Closure(s) => s,
            _ => return None,
        };
        Some(self.fn_sigs[sig_id.0 as usize].0.clone())
    }

    /// Phase Substr.B — boundary materialization. If the callee expects
    /// `Type::Str` for an arg position and the actual operand is
    /// `Type::Substr`, allocate an owned Str via substr_to_owned and
    /// return the materialized operand; the caller drops it after the
    /// call. Other type pairs pass through unchanged. Returns the
    /// (possibly-rewritten) args plus a list of Str values to drop after
    /// the call returns.
    fn materialize_call_args(
        &mut self,
        fn_ty: Type,
        args: Vec<Operand>,
    ) -> (Vec<Operand>, Vec<Operand>) {
        let Some(param_tys) = self.sig_param_tys(fn_ty) else {
            return (args, Vec::new());
        };
        let mut out = Vec::with_capacity(args.len());
        let mut drops = Vec::new();
        for (i, a) in args.into_iter().enumerate() {
            let actual = self.operand_ty(&a);
            let expected = param_tys.get(i).copied();
            if expected == Some(Type::Str) && actual == Type::Substr {
                let v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.substr_to_owned, vec![a]),
                    Type::Str,
                    None,
                );
                out.push(Operand::Value(v));
                drops.push(Operand::Value(v));
            } else {
                out.push(a);
            }
        }
        (out, drops)
    }

    pub(crate) fn call_fn_value(
        &mut self,
        fn_val: Operand,
        fn_ty: Type,
        args: Vec<Operand>,
    ) -> ValueId {
        let (args, drops) = self.materialize_call_args(fn_ty, args);
        let ret = self.call_fn_value_raw(fn_val, fn_ty, args);
        for d in drops {
            self.emit_drop_value(d, Type::Str);
        }
        ret
    }

    /// v0.6+1 perf checkpoint — devirt variant of `call_fn_value`.
    /// When the callable's underlying FuncId is statically known
    /// (caller resolved Expr::Closure / Expr::Ident at SSA-lower
    /// time), emit a direct `Call(fid, ...)` instead of the env+8
    /// fn_ptr load + CallIndirect dance. LLVM value-prop then sees
    /// a constant call target and can inline the body — a 10M-elem
    /// `xs.map((x) => x + k)` loop devirts every iteration's
    /// closure call so the optimizer can vectorize the lot.
    ///
    /// `fn_val` still threads through for its env-pointer side
    /// effect (Closure args take env_ptr as the first param). For
    /// Type::FnSig (no env), env arg is omitted.
    pub(crate) fn call_fn_value_devirt(
        &mut self,
        known_fid: FuncId,
        fn_val: Operand,
        fn_ty: Type,
        args: Vec<Operand>,
    ) -> ValueId {
        let (args, drops) = self.materialize_call_args(fn_ty, args);
        let ret_ty = match fn_ty {
            Type::Closure(sig_id) | Type::FnSig(sig_id) => self.fn_sigs[sig_id.0 as usize].1,
            other => panic!("call_fn_value_devirt: expected Closure/FnSig, got {other:?}"),
        };
        let mut argv: Vec<Operand> = match fn_ty {
            Type::Closure(_) => {
                /* Closure ABI: first arg is env_ptr, then user args. */
                let mut a = Vec::with_capacity(args.len() + 1);
                a.push(fn_val);
                a.extend(args);
                a
            }
            Type::FnSig(_) => args, // raw fn ptr — no env arg
            _ => unreachable!(),
        };
        let _ = &mut argv;
        let ret = self.f.append_inst(
            self.cur_block,
            InstKind::Call(known_fid, argv),
            ret_ty,
            None,
        );
        for d in drops {
            self.emit_drop_value(d, Type::Str);
        }
        ret
    }

    fn call_fn_value_raw(&mut self, fn_val: Operand, fn_ty: Type, args: Vec<Operand>) -> ValueId {
        match fn_ty {
            Type::Closure(user_sig_id) => {
                let env_ptr = match fn_val {
                    Operand::Value(v) => v,
                    _ => unreachable!("closure value is SSA"),
                };
                let fn_ptr = self.f.append_inst(
                    self.cur_block,
                    InstKind::Load(Type::Ptr, Operand::Value(env_ptr), CLOSURE_FN_ADDR_OFF),
                    Type::Ptr,
                    None,
                );
                let (user_params, ret_ty) = self.fn_sigs[user_sig_id.0 as usize].clone();
                let mut env_first = Vec::with_capacity(user_params.len() + 1);
                env_first.push(Type::Ptr);
                env_first.extend(user_params);
                let env_first_sig = intern_fn_sig(self.fn_sigs, env_first, ret_ty);
                let mut argv = Vec::with_capacity(args.len() + 1);
                argv.push(Operand::Value(env_ptr));
                argv.extend(args);
                self.f.append_inst(
                    self.cur_block,
                    InstKind::CallIndirect(env_first_sig, Operand::Value(fn_ptr), argv),
                    ret_ty,
                    None,
                )
            }
            Type::FnSig(sig_id) => {
                let fn_ptr_val = match fn_val {
                    Operand::Value(v) => v,
                    _ => unreachable!("fnsig value is SSA"),
                };
                let ret_ty = self.fn_sigs[sig_id.0 as usize].1;
                self.f.append_inst(
                    self.cur_block,
                    InstKind::CallIndirect(sig_id, Operand::Value(fn_ptr_val), args),
                    ret_ty,
                    None,
                )
            }
            other => panic!("ssa-lower: call_fn_value: expected Closure or FnSig, got {other:?}"),
        }
    }

    /// Reverse lookup `FuncId → name` via the lowerer's fn_table. Linear
    /// in the table size; used by `emit_throw_check` to consult the
    /// may_throw set (also keyed by name). Module fn count stays in the
    /// double-digits for our cases, so the linear scan is in the noise.
    fn f_name_of(&self, fid: FuncId) -> String {
        self.fn_table
            .iter()
            .find(|(_, v)| **v == fid)
            .map(|(k, _)| k.clone())
            .unwrap_or_default()
    }

    /// True if `fid` is one of the runtime intrinsics declared at the top
    /// of `lower()`. None of these throw, so M4's call-site throw-check
    /// can skip the cond_br after their calls (saves a runtime fn call
    /// per intrinsic invocation in the hot path).
    fn is_intrinsic(&self, fid: FuncId) -> bool {
        let i = &self.intrinsics;
        fid == i.print_i64
            || fid == i.print_f64
            || fid == i.print_bool
            || fid == i.print_i64_err
            || fid == i.print_f64_err
            || fid == i.print_bool_err
            || fid == i.str_print_err
            || fid == i.str_alloc
            || fid == i.str_print
            || fid == i.str_drop
            || fid == i.str_concat
            || fid == i.obj_alloc
            || fid == i.obj_drop_sized
            || fid == i.arr_alloc
            || fid == i.arr_push
            || fid == i.arr_shift
            || fid == i.arr_unshift
            || fid == i.arr_splice
            || fid == i.arr_drop
            || fid == i.arr_reserve
            || fid == i.arr_push_unchecked
            || fid == i.str_slice
            || fid == i.str_char_code_at
            || fid == i.str_code_point_at
            || fid == i.str_starts_with
            || fid == i.str_ends_with
            || fid == i.str_index_of
            || fid == i.str_last_index_of
            || fid == i.str_locale_compare
            || fid == i.str_includes
            || fid == i.str_eq
            || fid == i.str_split
            || fid == i.substr_create
            || fid == i.substr_drop
            || fid == i.substr_char_code_at
            || fid == i.substr_code_point_at
            || fid == i.substr_eq_str
            || fid == i.substr_to_owned
            || fid == i.substr_starts_with
            || fid == i.substr_ends_with
            || fid == i.substr_includes
            || fid == i.substr_index_of
            || fid == i.substr_slice
            || fid == i.substr_substring
            || fid == i.substr_trim
            || fid == i.substr_trim_into
            || fid == i.substr_trim_start
            || fid == i.substr_trim_end
            || fid == i.substr_concat_substr_str
            || fid == i.substr_concat_str_substr
            || fid == i.substr_concat_substr_substr
            || fid == i.arr_from_string
            || fid == i.str_substring
            || fid == i.arr_to_reversed
            || fid == i.arr_with
            || fid == i.arr_join
            || fid == i.arr_join_substr
            || fid == i.math_sqrt
            || fid == i.math_abs
            || fid == i.math_floor
            || fid == i.math_ceil
            || fid == i.math_log
            || fid == i.math_exp
            || fid == i.math_pow
            || fid == i.math_min
            || fid == i.math_max
            || fid == i.math_sign
            || fid == i.math_round
            || fid == i.math_trunc
            || fid == i.math_sin
            || fid == i.math_cos
            || fid == i.math_tan
            || fid == i.math_asin
            || fid == i.math_acos
            || fid == i.math_atan
            || fid == i.math_atan2
            || fid == i.math_log2
            || fid == i.math_log10
            || fid == i.math_cbrt
            || fid == i.math_sinh
            || fid == i.math_cosh
            || fid == i.math_tanh
            || fid == i.math_asinh
            || fid == i.math_acosh
            || fid == i.math_atanh
            || fid == i.math_expm1
            || fid == i.math_log1p
            || fid == i.math_imul
            || fid == i.math_clz32
            || fid == i.math_fround
            || fid == i.math_f16round
            || fid == i.math_random
            || fid == i.json_quote_str
            || fid == i.str_repeat
            || fid == i.str_to_upper
            || fid == i.str_to_lower
            || fid == i.str_trim
            || fid == i.str_trim_start
            || fid == i.str_trim_end
            || fid == i.str_pad_start
            || fid == i.str_pad_end
            || fid == i.str_from_char_code
            || fid == i.str_at
            || fid == i.str_replace
            || fid == i.str_replace_all
            || fid == i.num_to_fixed_f
            || fid == i.num_to_fixed_i
            || fid == i.num_to_string_radix_i
            || fid == i.num_to_string_radix_f
            || fid == i.num_to_exp_f
            || fid == i.num_to_exp_i
            || fid == i.num_to_precision_f
            || fid == i.num_to_precision_i
            || fid == i.arr_flat
            || fid == i.arr_flat_any
            || fid == i.arr_extend_typed_into_any
            || fid == i.arr_concat
            || fid == i.arr_reverse
            || fid == i.arr_fill
            || fid == i.arr_copy_within
            || fid == i.throw_set
            || fid == i.throw_check
            || fid == i.throw_take
    }

    /// M4 — emit the per-call-site throw check. After a user fn returns,
    /// load the throw_active flag; if non-zero, branch to the innermost
    /// active try-block's catch (via `try_stack`) or — if no try is
    /// active in this fn — emit drops + ret a sentinel so the caller's
    /// own throw_check picks it up. Skips entirely for runtime intrinsics
    /// (they never throw).
    pub(crate) fn emit_throw_check(&mut self, target: Option<FuncId>) {
        if let Some(fid) = target {
            if self.is_intrinsic(fid) {
                return;
            }
            // M4.3.b — skip the check entirely if the callee is a
            // verified-non-throwing user fn. fib40 / popcount / gcd /
            // mandelbrot etc. all live here, so the M4.1 5% slowdown
            // is gone for any program that doesn't use try/throw at
            // all (or whose hot fns provably can't reach a throw).
            let callee_name = self.f_name_of(fid);
            if !self.may_throw_fns.contains(&callee_name) {
                return;
            }
        }
        let active = self.f.append_inst(
            self.cur_block,
            InstKind::Call(self.intrinsics.throw_check, vec![]),
            Type::I64,
            None,
        );
        let cmp = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(IPred::Ne, Operand::Value(active), Operand::ConstI64(0)),
            Type::Bool,
            None,
        );
        let normal_blk = self.f.add_block();
        let throw_blk = self.f.add_block();
        let cb = self.cur_block;
        self.f.set_term(
            cb,
            Terminator::CondBr {
                cond: Operand::Value(cmp),
                then_blk: throw_blk,
                else_blk: normal_blk,
            },
        );
        // throw_blk: route to innermost active try's catch, or
        // propagate (drop owned locals + ret sentinel).
        if let Some(catch) = self.try_stack.last().copied() {
            self.f.set_term(throw_blk, Terminator::Br(catch));
        } else if self.is_main_fn {
            // bug-327 C2.5 — the throw escaped every user frame: this
            // is an uncaught exception. Pre-fix main ret'd the I32
            // sentinel 0, so a crashing program exited clean (bun:
            // error report + exit 1). __torajs_uncaught_exit_code
            // reports the pending throw to stderr and yields 1.
            self.cur_block = throw_blk;
            self.emit_drops_for_owned_locals();
            let uncaught_fid = *self
                .fn_table
                .get("__torajs_uncaught_exit_code")
                .expect("__torajs_uncaught_exit_code declared in module setup");
            let code = self.f.append_inst(
                self.cur_block,
                InstKind::Call(uncaught_fid, vec![]),
                Type::I32,
                None,
            );
            let cb2 = self.cur_block;
            self.f
                .set_term(cb2, Terminator::Ret(Some(Operand::Value(code))));
        } else {
            self.cur_block = throw_blk;
            self.emit_drops_for_owned_locals();
            let cb2 = self.cur_block;
            let ret_ty = self.f.ret;
            let term = match ret_ty {
                Type::Void => Terminator::Ret(None),
                Type::F64 => Terminator::Ret(Some(Operand::ConstF64(0.0))),
                Type::I32 => Terminator::Ret(Some(Operand::ConstI32(0))),
                Type::Bool => Terminator::Ret(Some(Operand::ConstBool(false))),
                _ => Terminator::Ret(Some(Operand::ConstI64(0))),
            };
            self.f.set_term(cb2, term);
        }
        self.cur_block = normal_blk;
    }

    /// Look up the callee's return type from the signatures map populated
    /// in pass 1 of `lower`. Defaults to I64 for unknown FuncIds (intrinsics
    /// or forward refs we haven't catalogued yet — print_i64 returns void
    /// and is called via `append_void`, so its callsites never reach here).
    /// Phase H.3.b — set of runtime class tags that satisfy `instanceof
    /// class_name`: `class_name` itself plus every transitively-extending
    /// subclass. Empty if `class_name` isn't a declared class. Same
    /// algorithm as instanceof's lower path, factored out so the
    /// `__dispatch_<M>` interception can reuse it.
    fn compute_descendant_tags(&self, class_name: &str) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        if !self.ast.class_parents.contains_key(class_name) {
            return out;
        }
        for c in self.ast.class_parents.keys() {
            let mut cur = Some(c.clone());
            let mut depth = 0u32;
            while let Some(name) = cur {
                if depth > 64 {
                    break;
                }
                if name == *class_name {
                    if let Some(tag) = self.class_name_to_tag.get(c) {
                        out.push(*tag);
                    }
                    break;
                }
                cur = self.ast.class_parents.get(&name).and_then(|p| p.clone());
                depth += 1;
            }
        }
        out.sort();
        out.dedup();
        out
    }

    pub(crate) fn f_ret_type_hint(&self, fid: FuncId) -> Type {
        self.signatures.get(&fid).copied().unwrap_or(Type::I64)
    }

    /// P11.4 — `for (const c of <str>)` per ES §22.1.5 (String
    /// iterator). Yields one Substr per code point: BMP code units are
    /// 1-cu views, supplementary plane code points combine the
    /// high+low surrogate pair into a single 2-cu view. Loop layout:
    ///
    ///   i = 0; len = s.length
    ///   while i < len:
    ///     cp = __torajs_str_code_point_at(s, i)
    ///     adv = (cp > 0xFFFF) ? 2 : 1
    ///     c = __torajs_substr_create(s, i, adv)
    ///     <body>
    ///     i += adv     (recomputed in step block — no phi for adv)
    ///
    /// The internal `i_ident` shadows-and-restores per the parent
    /// for-of scope discipline. `var_name` binds the per-iter Substr;
    /// it's marked owned (rc=1 from substr_create) so the per-iter
    /// drop fires on `c` only, not on the parent string.
    pub(crate) fn lower_for_of_str(
        &mut self,
        src_op: Operand,
        i_ident: &str,
        var_name: &str,
        body: &Stmt,
    ) {
        crate::ssa_lower_for_of_str::lower(self, src_op, i_ident, var_name, body);
    }

    /// P5.3 Phase B — emit the iterator-protocol `for (let v of obj)`
    /// loop:
    ///   let __it = obj.__sym_Symbol_iterator__()
    ///   while (true) {
    ///     let __step = __it.next()
    ///     if (__step.done) break
    ///     let v = __step.value
    ///     <body>
    ///   }
    ///
    /// `src_op` is the receiver value (Type::Obj(sid)). `iter_fid` is
    /// the resolved `__cm_<src_class>____sym_Symbol_iterator__`. The
    /// returned iter is itself Type::Obj(iter_sid) — we look up its
    /// class via aliases to find `__cm_<iter_class>__next`, then the
    /// returned step struct provides .done / .value via direct field
    /// loads.
    pub(crate) fn lower_for_of_iter_protocol(
        &mut self,
        src_op: Operand,
        iter_fid: FuncId,
        var_name: &str,
        body: &Stmt,
        src_class: &str,
    ) {
        crate::ssa_lower_for_of_iter_protocol::lower_for_of_iter_protocol(
            self, src_op, iter_fid, var_name, body, src_class,
        );
    }

    /// P6.4c — emit `for (let v of src)` for Map / Set / MapIter
    /// receivers. Map's default iter (per spec §23.1.4) is
    /// `.entries()` — each step yields a freshly-alloced `[k, v]`
    /// Array<Any>, so we bind `var_name` as `Type::Arr<Any>` to let
    /// destructuring `for (let [k, v] of m)` work via the existing
    /// parser-side desugar (which generates `__forof_destr[0]` /
    /// `[1]` reads, lowered through the Array<Any> Expr::Index
    /// path). Set's default iter yields elements (Type::Any).
    /// MapIter is borrowed — kind unknown at compile time, so var is
    /// type-erased to Any.
    pub(crate) fn lower_for_of_map_like(
        &mut self,
        src_op: Operand,
        src_ty: Type,
        var_name: &str,
        body: &Stmt,
    ) {
        crate::ssa_lower_for_of_map_like::lower_for_of_map_like(
            self, src_op, src_ty, var_name, body,
        );
    }
}
