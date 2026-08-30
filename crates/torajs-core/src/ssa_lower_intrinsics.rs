// CARVE-OUT: pure runtime-intrinsic FuncId lookup table — refactor blocked by
// rustc requiring a single struct definition site for all field access patterns
// used across 100+ callers. Splitting into sub-structs would require a
// nested-struct or trait-impl-per-arm design — not worth the invasive refactor
// for what is fundamentally a flat data table.

//! `Intrinsics` — flat table of `FuncId` slots for every backend-
//! provided runtime entry point extracted from `ssa_lower.rs` chunk
//! 407 — Path A.3-batch28.
//!
//! Populated during `lower()`'s Pass 1 by `declare_intrinsic` (see
//! `ssa_lower_synthesize_main.rs`); read at every SSA emit site that
//! needs to call into the C runtime (arr / str / num / math / json
//! / substr / bigint / throw / rc / etc). Field docs preserved
//! verbatim from the source.

use crate::ssa;
use crate::ssa::FuncId;

/// FuncIds of every backend-provided runtime entry point. Threaded through
/// every lowering site that needs to emit a runtime call. Single struct so
/// adding a new intrinsic later (e.g. `__torajs_str_concat` for P2.2.c)
/// only touches one type signature.
// `#[allow(dead_code)]` — the Intrinsics struct is a flat FuncId
// lookup table populated by `declare_intrinsic` for every runtime
// entry point tora may ever emit a call to; individual fields are
// read via `self.intrinsics.<name>` at the per-shape SSA emit
// sites. Some slots correspond to intrinsics that are declared
// (may_throw / signature registration) but only read from a subset
// of siblings, so rustc's dead_code lint flags them here despite
// them being part of the contract with the runtime crate. The
// CARVE-OUT header above scopes the whole file as a data table;
// this attribute makes that stance mechanical.
#[allow(dead_code)]
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
    /// r505 — newline-less twins for multi-arg `console.log`.
    pub(crate) print_i64_inline: FuncId,
    pub(crate) print_f64_inline: FuncId,
    pub(crate) print_bool_inline: FuncId,
    pub(crate) str_print_inline: FuncId,
    pub(crate) substr_print_inline: FuncId,
    pub(crate) str_alloc: FuncId,
    pub(crate) str_print: FuncId,
    pub(crate) str_drop: FuncId,
    pub(crate) str_concat: FuncId,
    /// Declared for `torajs-egraph`'s `str_append` pass, which
    /// rewrites `concat` + `drop-left` pairs onto it; the lowering
    /// itself never emits a call here.
    pub(crate) str_append: FuncId,
    /// Declared for `torajs-egraph`'s `concat_num_fuse` pass
    /// (S1-A2 attack B1) — fused `a + String(n)`; lowering never
    /// emits these directly.
    pub(crate) str_concat_i64: FuncId,
    pub(crate) str_concat_f64: FuncId,
    /// Phase B refcount — `__torajs_rc_inc(ptr)` increments the heap
    /// header's refcount (NULL passes through). Emitted at every
    /// slot-copy / shared-ownership site for non-Copy heap values.
    pub(crate) rc_inc: FuncId,
    pub(crate) obj_alloc: FuncId,
    pub(crate) capture_box_alloc: FuncId,
    pub(crate) capture_box_inc: FuncId,
    pub(crate) capture_box_drop: FuncId,
    pub(crate) capture_box_drop_heap: FuncId,
    pub(crate) capture_box_drop_any: FuncId,
    pub(crate) obj_drop_sized: FuncId,
    pub(crate) value_drop_heap: FuncId,
    pub(crate) cycle_unbuffer: FuncId,
    pub(crate) closure_drop_props_slow: FuncId,
    pub(crate) closure_unbuffer_slow: FuncId,
    /// r502 — a struct instance's expando legs behind link seams.
    pub(crate) obj_props_drop_slow: FuncId,
    pub(crate) obj_buffer_slow: FuncId,
    pub(crate) obj_unbuffer_slow: FuncId,
    pub(crate) arr_alloc: FuncId,
    pub(crate) arr_mark_kind: FuncId,
    pub(crate) arr_push: FuncId,
    pub(crate) arr_push_non_deque: FuncId,
    pub(crate) arr_shift: FuncId,
    pub(crate) arr_unshift: FuncId,
    pub(crate) arr_splice: FuncId,
    pub(crate) arr_splice_items: FuncId,
    pub(crate) arr_species_guard: FuncId,
    pub(crate) arr_len_write_guard: FuncId,
    pub(crate) arr_drop: FuncId,
    pub(crate) arr_drop_scalar: FuncId,
    pub(crate) arr_reserve: FuncId,
    pub(crate) arr_push_unchecked: FuncId,
    pub(crate) arr_extend_unchecked: FuncId,
    pub(crate) arr_slice: FuncId,
    pub(crate) arr_sort_cb: FuncId,
    pub(crate) str_repeat: FuncId,
    pub(crate) str_anchor: FuncId,
    pub(crate) str_fontcolor: FuncId,
    pub(crate) str_fontsize: FuncId,
    pub(crate) str_link: FuncId,
    pub(crate) str_big: FuncId,
    pub(crate) str_blink: FuncId,
    pub(crate) str_bold: FuncId,
    pub(crate) str_fixed: FuncId,
    pub(crate) str_italics: FuncId,
    pub(crate) str_small: FuncId,
    pub(crate) str_strike: FuncId,
    pub(crate) str_sub: FuncId,
    pub(crate) str_sup: FuncId,
    pub(crate) str_to_upper: FuncId,
    pub(crate) str_to_lower: FuncId,
    pub(crate) str_trim: FuncId,
    pub(crate) str_trim_start: FuncId,
    pub(crate) str_trim_end: FuncId,
    pub(crate) str_pad_start: FuncId,
    pub(crate) str_pad_end: FuncId,
    pub(crate) str_from_char_code: FuncId,
    pub(crate) str_from_char_code_f64: FuncId,
    pub(crate) str_from_code_point: FuncId,
    pub(crate) str_from_code_point_f64: FuncId,
    pub(crate) string_raw: FuncId,
    pub(crate) str_normalize: FuncId,
    pub(crate) str_to_locale_upper: FuncId,
    pub(crate) str_to_locale_lower: FuncId,
    pub(crate) str_locale_case_arr: FuncId,
    pub(crate) str_at: FuncId,
    pub(crate) str_replace: FuncId,
    pub(crate) str_replace_all: FuncId,
    pub(crate) str_replace_fn: FuncId,
    pub(crate) str_replace_all_fn: FuncId,
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
    pub(crate) num_to_index: FuncId,
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
    /// RFC 20260716 刀 13 — typed-Str `.hasOwnProperty(key)` per
    /// ES §22.1.4 String Exotic Object.
    pub(crate) str_prop_has: FuncId,
    /// RFC 20260716 刀 20 — typed-Str `.propertyIsEnumerable(key)`
    /// per ES §22.1.4 (index chars enumerable, `length` NOT).
    pub(crate) str_prop_enumerable: FuncId,
    pub(crate) str_null_check: FuncId,
    pub(crate) str_is_nullish: FuncId,
    pub(crate) str_is_undef: FuncId,
    pub(crate) is_undef_cell: FuncId,
    pub(crate) heap_nullish_check: FuncId,
    pub(crate) str_sort_cmp: FuncId,
    pub(crate) str_sort_undef_pre: FuncId,
    pub(crate) str_split: FuncId,
    pub(crate) str_split_no_sep: FuncId,
    pub(crate) str_split_any_sep: FuncId,
    pub(crate) str_split_any_sep_lim: FuncId,
    /// Phase Substr.A — substring view runtime helpers.
    pub(crate) substr_create: FuncId,
    pub(crate) substr_drop: FuncId,
    pub(crate) arr_drop_str_elems: FuncId,
    pub(crate) arr_substr_adopt_copied: FuncId,
    pub(crate) arr_substr_materialize_owned: FuncId,
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
    pub(crate) str_index_view: FuncId,
    pub(crate) substr_index_view: FuncId,
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
    /// `new RegExp(pattern, flags)` entry — wraps `regex_compile` and
    /// records a catchable `SyntaxError` on the TLS pending-throw
    /// slot when the parser rejects the pattern. See the declare site
    /// for the full contract (`emit_throw_check_owned` in
    /// `lower_regexp` follows the call). Literal `/pat/flags` still
    /// calls the plain `regex_compile` — literal-time throw is L3b.
    pub(crate) regex_compile_or_throw: FuncId,
    pub(crate) regex_compile_inplace: FuncId,
    pub(crate) regex_compile_any: FuncId,
    pub(crate) regexp_from_any: FuncId,
    pub(crate) regexp_drop_if_coerced: FuncId,
    pub(crate) str_replace_any_pattern: FuncId,
    pub(crate) str_replace_any_repl: FuncId,
    pub(crate) str_replace_any_pattern_fn: FuncId,
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
    /// §22.1.3.{13,20} step 3 custom-matcher legs (Any-typed
    /// pattern; wk-symbol index operand picks match / search).
    pub(crate) any_str_symbol_try: FuncId,
    pub(crate) regex_replace: FuncId,
    pub(crate) regex_replace_all: FuncId,
    /// P9.5-A1 — fn-callback variants of `s.replace(re, fn)` /
    /// `s.replaceAll(re, fn)`. 3rd arg is the closure env block (env+8
    /// = lifted body's fn_addr). Runtime invokes (env, match_str) ->
    /// ret_str per match.
    pub(crate) regex_replace_fn: FuncId,
    pub(crate) regex_replace_all_fn: FuncId,
    pub(crate) regex_split: FuncId,
    /// Chunk 800 — `s.search(re)` per ES §22.1.3.19: UTF-16 match
    /// index or -1; lastIndex untouched.
    pub(crate) regex_search: FuncId,
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
    /// §21.4.2.1 step 4 — `new Date(<runtime value>)`; the anyvalue
    /// kernel runs the no-hint ToPrimitive + parse/ToNumber split.
    pub(crate) date_from_value: FuncId,
    pub(crate) date_drop: FuncId,
    pub(crate) date_now_static: FuncId,
    pub(crate) date_get_time: FuncId,
    pub(crate) date_to_iso_string: FuncId,
    pub(crate) date_to_json: FuncId,
    pub(crate) date_set_time: FuncId,
    pub(crate) date_get_year: FuncId,
    pub(crate) date_set_year: FuncId,
    pub(crate) date_to_gmt_string: FuncId,
    pub(crate) date_to_date_string: FuncId,
    pub(crate) date_to_time_string: FuncId,
    pub(crate) date_to_string: FuncId,
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
    pub(crate) date_set_utc_full_year: FuncId,
    pub(crate) date_set_utc_month: FuncId,
    pub(crate) date_set_utc_date: FuncId,
    pub(crate) date_set_utc_hours: FuncId,
    pub(crate) date_set_utc_minutes: FuncId,
    pub(crate) date_set_utc_seconds: FuncId,
    pub(crate) date_set_utc_milliseconds: FuncId,
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
    /// RFC 20260708-closure-argv-face — bulk any-append over a raw
    /// argv window (`__torajs_arguments` materializer half 2).
    pub(crate) arr_any_push: FuncId,
    /// §10.4.4 — the FLAG_ARR_ARGUMENTS stamp on the fresh
    /// `__torajs_arguments` cell.
    pub(crate) arr_mark_arguments: FuncId,
    /// §10.4.4.6 step 21 — the strict `arguments.callee` read.
    pub(crate) arguments_callee: FuncId,
    pub(crate) arr_push_any: FuncId,
    pub(crate) arr_mark_last_hole: FuncId,
    pub(crate) arr_any_to_locale_string: FuncId,
    pub(crate) arr_typed_to_locale_string: FuncId,
    pub(crate) arr_any_index_of: FuncId,
    pub(crate) arr_any_last_index_of: FuncId,
    pub(crate) arr_unshift_any: FuncId,
    pub(crate) arr_fill_any: FuncId,
    pub(crate) arr_extend_any: FuncId,
    pub(crate) arr_any_slice: FuncId,
    pub(crate) arr_any_to_reversed: FuncId,
    pub(crate) arr_has_index: FuncId,
    pub(crate) arr_set_any: FuncId,
    pub(crate) arr_set_any_grow: FuncId,
    pub(crate) arr_typed_set_grow: FuncId,
    pub(crate) arr_index_revive_idx: FuncId,
    pub(crate) arr_index_check_store: FuncId,
    pub(crate) arr_null_check: FuncId,
    pub(crate) arr_get_any_tag: FuncId,
    pub(crate) arr_get_any_boxed: FuncId,
    pub(crate) arr_get_any_owned: FuncId,
    pub(crate) arr_get_any_value: FuncId,
    pub(crate) arr_any_pop: FuncId,
    pub(crate) arr_any_shift: FuncId,
    pub(crate) dynobj_alloc: FuncId,
    pub(crate) dynobj_mark_null_proto: FuncId,
    pub(crate) get_builtin_prototype: FuncId,
    pub(crate) instanceof_class_any_tag: FuncId,
    pub(crate) instanceof_builtin_any_tag: FuncId,
    pub(crate) instanceof_fn_value: FuncId,
    pub(crate) instanceof_dynamic: FuncId,
    pub(crate) instanceof_object_any: FuncId,
    pub(crate) instanceof_generic_any: FuncId,
    pub(crate) instanceof_generic_tag: FuncId,
    pub(crate) in_op_any_num: FuncId,
    pub(crate) in_op_any_str: FuncId,
    pub(crate) any_is_arr: FuncId,
    pub(crate) fnprops_set: FuncId,
    pub(crate) fnprops_get_tag: FuncId,
    pub(crate) fnprops_get_value: FuncId,
    pub(crate) fnprops_bind_cell: FuncId,
    pub(crate) arrprops_set: FuncId,
    pub(crate) arr_define: FuncId,
    pub(crate) arr_keys_only_of: FuncId,
    pub(crate) arrprops_get_tag: FuncId,
    pub(crate) arrprops_get_value: FuncId,
    pub(crate) arr_member_value: FuncId,
    pub(crate) dynobj_get_tag: FuncId,
    pub(crate) dynobj_get_value: FuncId,
    pub(crate) dynobj_set: FuncId,
    /// RFC 20260825-inject-narrow-define 刀 3 — the accessor-free
    /// object-literal init set (fresh-receiver narrow kernel).
    pub(crate) dynobj_set_fresh: FuncId,
    pub(crate) dynobj_set_fresh_dup: FuncId,
    pub(crate) dynobj_define: FuncId,
    pub(crate) dynobj_define_from_desc: FuncId,
    pub(crate) dynobj_define_from_desc_soft: FuncId,
    pub(crate) reflect_apply: FuncId,
    pub(crate) reflect_construct: FuncId,
    pub(crate) dynobj_spread_from: FuncId,
    pub(crate) accessor_pair_new: FuncId,
    pub(crate) accessor_invoke_getter: FuncId,
    pub(crate) get_property_descriptor: FuncId,
    pub(crate) module_ns_finalize: FuncId,
    pub(crate) throw_typeerror_if_not_object: FuncId,
    pub(crate) throw_typeerror_if_not_desc_object: FuncId,
    pub(crate) throw_typeerror_if_props_nullish: FuncId,
    pub(crate) throw_typeerror_if_props_null_only: FuncId,
    pub(crate) define_props_source_gate: FuncId,
    pub(crate) object_create_check_proto: FuncId,
    pub(crate) object_create_link_proto: FuncId,
    pub(crate) anyv_set_prototype_of: FuncId,
    pub(crate) anyv_proto_member_set: FuncId,
    pub(crate) dynobj_define_properties_from: FuncId,
    pub(crate) arr_throw_reduce_empty: FuncId,
    pub(crate) arr_throw_reduce_right_empty: FuncId,
    /// ES §10.1.9 — writing a property that has a [[Get]] but no
    /// [[Set]] is a strict-mode `TypeError`. `Object.assign` sees this
    /// at compile time off the target's layout (`__getter_v` with no
    /// `__setter_v` half) and emits the 0-arg throw unconditionally.
    pub(crate) throw_readonly_assign: FuncId,
    pub(crate) arr_length_descriptor: FuncId,
    pub(crate) str_length_descriptor: FuncId,
    pub(crate) arr_index_strs: FuncId,
    pub(crate) str_index_strs: FuncId,
    pub(crate) arr_keys_only: FuncId,
    pub(crate) str_keys_only: FuncId,
    /* RC-4 F1c — runtime chooser for keys/gOPN on struct receivers
     * that may have been dynobj-converted by defineProperty. */
    pub(crate) obj_own_keys: FuncId,
    pub(crate) anyv_own_keys: FuncId,
    pub(crate) anyv_own_keys_all: FuncId,
    pub(crate) anyv_forin_keys: FuncId,
    pub(crate) anyv_own_symbols: FuncId,
    pub(crate) anyv_from_entries: FuncId,
    pub(crate) object_group_by: FuncId,
    pub(crate) map_group_by: FuncId,
    pub(crate) anyv_assign: FuncId,
    pub(crate) str_to_char_arr: FuncId,
    pub(crate) arr_entries_by_tag: FuncId,
    pub(crate) str_entries: FuncId,
    pub(crate) anyv_struct_keys: FuncId,
    pub(crate) anyv_own_values: FuncId,
    pub(crate) anyv_own_entries: FuncId,
    pub(crate) obj_key_is_nonenumerable: FuncId,
    pub(crate) str_index_descriptor: FuncId,
    pub(crate) anyv_prevent_extensions: FuncId,
    pub(crate) anyv_is_extensible: FuncId,
    pub(crate) anyv_seal: FuncId,
    pub(crate) anyv_is_sealed: FuncId,
    pub(crate) anyv_freeze: FuncId,
    pub(crate) dynobj_has: FuncId,
    pub(crate) dynobj_delete: FuncId,
    pub(crate) arr_drop_any: FuncId,
    pub(crate) any_box: FuncId,
    pub(crate) anyv_box_str_slot: FuncId,
    pub(crate) anyv_box_substr_slot: FuncId,
    pub(crate) anyv_str_slot_tag: FuncId,
    pub(crate) anyv_str_slot_value: FuncId,
    pub(crate) anyv_substr_slot_tag: FuncId,
    pub(crate) anyv_substr_slot_value: FuncId,
    pub(crate) any_payload_rc_inc: FuncId,
    pub(crate) anyv_retain: FuncId,
    pub(crate) proto_register: FuncId,
    pub(crate) proto_link_fresh: FuncId,
    pub(crate) register_native_error: FuncId,
    pub(crate) proto_get: FuncId,
    pub(crate) closure_install_gen_proto: FuncId,
    pub(crate) builtin_ctor_value: FuncId,
    pub(crate) ns_static_cell: FuncId,
    pub(crate) anyv_json_stringify: FuncId,
    pub(crate) anyv_json_stringify_gap: FuncId,
    pub(crate) anyv_json_gap_str: FuncId,
    pub(crate) json_stringify_full: FuncId,
    pub(crate) json_raw_json: FuncId,
    pub(crate) json_is_raw_json: FuncId,
    pub(crate) json_parse_any: FuncId,
    pub(crate) json_parse_reviver: FuncId,
    pub(crate) json_indent: FuncId,
    pub(crate) json_colon: FuncId,
    pub(crate) json_close_indent: FuncId,
    pub(crate) class_register: FuncId,
    pub(crate) error_proto_install: FuncId,
    pub(crate) error_is_error: FuncId,
    pub(crate) static_method_define: FuncId,
    pub(crate) static_field_define: FuncId,
    pub(crate) class_cell_raw: FuncId,
    pub(crate) proto_cell_raw: FuncId,
    pub(crate) class_accessor_define: FuncId,
    pub(crate) class_static_accessor_define: FuncId,
    pub(crate) class_get: FuncId,
    pub(crate) get_proto_of_any: FuncId,
    pub(crate) proto_member_get: FuncId,
    pub(crate) error_to_string: FuncId,
    pub(crate) error_tostring_dispatch: FuncId,
    pub(crate) error_message_present: FuncId,
    pub(crate) error_message_get: FuncId,
    pub(crate) error_name_present: FuncId,
    pub(crate) error_name_get: FuncId,
    pub(crate) error_install_cause: FuncId,
    pub(crate) any_prop_enumerable: FuncId,
    pub(crate) get_property_descriptors: FuncId,
    pub(crate) ctor_no_super_throw: FuncId,
    pub(crate) throw_reference_error_name: FuncId,
    pub(crate) class_computed_method_define: FuncId,
    pub(crate) class_computed_accessor_define: FuncId,
    pub(crate) class_source_register: FuncId,
    pub(crate) ctor_mark_arr_species: FuncId,
    pub(crate) arr_subclass_alloc: FuncId,
    pub(crate) arr_subclass_super_len: FuncId,
    pub(crate) arr_subclass_super_elems: FuncId,
    pub(crate) number_wrapper_subclass_alloc: FuncId,
    pub(crate) string_wrapper_subclass_alloc: FuncId,
    pub(crate) boolean_wrapper_subclass_alloc: FuncId,
    pub(crate) function_subclass_alloc: FuncId,
    pub(crate) map_subclass_alloc: FuncId,
    pub(crate) set_subclass_alloc: FuncId,
    pub(crate) promise_subclass_alloc: FuncId,
    pub(crate) regex_subclass_alloc: FuncId,
    pub(crate) weakmap_subclass_alloc: FuncId,
    pub(crate) weakset_subclass_alloc: FuncId,
    pub(crate) date_subclass_alloc: FuncId,
    pub(crate) weakmap_subclass_super: FuncId,
    pub(crate) weakset_subclass_super: FuncId,
    pub(crate) date_subclass_super: FuncId,
    pub(crate) date_subclass_super_components: FuncId,
    pub(crate) number_wrapper_subclass_super: FuncId,
    pub(crate) map_subclass_super: FuncId,
    pub(crate) set_subclass_super: FuncId,
    pub(crate) promise_subclass_super: FuncId,
    pub(crate) regex_subclass_super: FuncId,
    pub(crate) regex_subclass_super_flags: FuncId,
    pub(crate) string_wrapper_subclass_super: FuncId,
    pub(crate) boolean_wrapper_subclass_super: FuncId,
    pub(crate) typedarray_subclass_alloc: FuncId,
    pub(crate) typedarray_subclass_super: FuncId,
    pub(crate) genfn_proto: FuncId,
    pub(crate) genfn_chain: FuncId,
    /// RFC 20260730-iterator-global 刀 1 — stripped-heir prototype
    /// chain writer (`class C extends Iterator {}`).
    pub(crate) proto_chain_builtin: FuncId,
    /// RFC 20260730-iterator-global 刀 1 — §7.3.22 proto-chain walk
    /// backing `v instanceof Iterator`.
    pub(crate) instanceof_builtin_proto: FuncId,
    /// RFC 20260730-iterator-global 刀 1 — §27.1.3.1 abstract-ctor
    /// TypeError kernel behind `new Iterator()`.
    pub(crate) iterator_ctor_throw: FuncId,
    /// RFC 20260823-proxy-substrate 刀 1 — §10.5.14 ProxyCreate.
    pub(crate) proxy_create: FuncId,
    /// §25.1.4.1 `new ArrayBuffer(length, options)`.
    pub(crate) arraybuffer_create: FuncId,
    /// §25.1.5.1 `ArrayBuffer.isView(arg)`.
    pub(crate) arraybuffer_is_view: FuncId,
    /// §23.2.5.1 `new <TypedArray>(…)`.
    pub(crate) typedarray_create: FuncId,
    /// §23.2 `x instanceof <T>` — the element-kind test.
    pub(crate) typedarray_is_kind: FuncId,
    /// §25.3.2 `new DataView(buffer, byteOffset, byteLength)` (刀 7).
    pub(crate) dataview_create: FuncId,
    /// RFC 20260823-proxy-substrate 刀 3 — §28.2.2.1 Proxy.revocable.
    pub(crate) proxy_revocable: FuncId,
    /// RFC 20260730-iterator-global 刀 4 — §27.1.6.2 `Iterator.from`
    /// (GetIteratorFlattenable + pass-through-or-wrap).
    pub(crate) iterator_from: FuncId,
    /// RFC 20260730-iterator-global 刀 5a — `Iterator.concat`
    /// (eager iterability check + lazy kind-CONCAT cell).
    pub(crate) iterator_concat: FuncId,
    /// RFC 20260730-iterator-global 刀 5b — `Iterator.zip`
    /// (eager opens + lazy kind-ZIP cell).
    pub(crate) iterator_zip: FuncId,
    /// RFC 20260730-iterator-global 刀 5c — `Iterator.zipKeyed`.
    pub(crate) iterator_zip_keyed: FuncId,
    pub(crate) dstr_close_pending: FuncId,
    pub(crate) dstr_park_pending: FuncId,
    pub(crate) dstr_drain_rest: FuncId,
    pub(crate) any_typeof: FuncId,
    pub(crate) any_to_bool: FuncId,
    pub(crate) any_to_number: FuncId,
    pub(crate) any_to_bigint: FuncId,
    pub(crate) any_to_object: FuncId,
    pub(crate) any_add: FuncId,
    pub(crate) any_arith: FuncId,
    /// RFC 20260716 刀 7 — Any bitwise/shift dispatch.
    pub(crate) any_bitwise: FuncId,
    /// RFC 20260716 刀 7 — Any unary `~` dispatch.
    pub(crate) any_bitnot: FuncId,
    /// `x++` / `x--` over an `any` slot (§13.4.4 / §13.4.5).
    pub(crate) any_incr_slot: FuncId,
    /// Value-shaped ToNumeric + step twin — the any-member update
    /// lane's GetV → step → member-set composition (§13.4.4.1).
    pub(crate) anyv_incr_value: FuncId,
    /// Literal-default substitution kernel (S2.39 / S3.8) — answers
    /// the value unless undefined, else boxes the baked Number/Bool
    /// default (§10.2.11 explicit-undefined binding).
    pub(crate) anyv_or_default: FuncId,
    pub(crate) any_compare: FuncId,
    pub(crate) any_strict_eq: FuncId,
    /// SameValueZero pair variant (§7.2.9) — includes-with-any-needle.
    pub(crate) any_svz: FuncId,
    pub(crate) any_any_strict_eq: FuncId,
    pub(crate) any_any_loose_eq: FuncId,
    pub(crate) any_index_get: FuncId,
    pub(crate) any_index_get_keyed: FuncId,
    pub(crate) anyv_to_property_key: FuncId,
    pub(crate) anyv_property_key_drop: FuncId,
    pub(crate) any_index_set_keyed: FuncId,
    pub(crate) any_index_set: FuncId,
    pub(crate) any_length_get: FuncId,
    pub(crate) any_name_get: FuncId,
    pub(crate) any_size_get: FuncId,
    pub(crate) any_regexp_prop: FuncId,
    pub(crate) any_member_get_tag: FuncId,
    /// Brand-gated tag channel for `__priv_`-named member reads —
    /// own-face miss throws TypeError (§7.3.31 PrivateGet).
    pub(crate) any_member_get_priv_tag: FuncId,
    /// §13.10.1 ergonomic brand check (`#x in o`).
    pub(crate) any_member_priv_has: FuncId,
    /// §7.3.32 PrivateSet write twin — undeclared brand throws.
    pub(crate) any_member_set_priv: FuncId,
    /// RFC 20260820-ctor-return-override — §10.2.2 step 13. The pick
    /// hands back an OWNED box; the carry borrows.
    pub(crate) ctor_ret_value: FuncId,
    pub(crate) ctor_ret_carry: FuncId,
    pub(crate) any_member_get_value: FuncId,
    pub(crate) any_accessor_get: FuncId,
    pub(crate) any_member_set: FuncId,
    pub(crate) any_iter_next: FuncId,
    /// `for await (v of <any>)` — the async-iterate drive.
    pub(crate) any_iter_next_await: FuncId,
    /// `Array.from`'s entry to the same walk — §23.1.2.1 step 3
    /// takes the array-like branch where every other consumer
    /// throws.
    pub(crate) any_iter_next_array_like: FuncId,
    pub(crate) any_iter_close: FuncId,
    pub(crate) iter_close_value: FuncId,
    pub(crate) any_call: FuncId,
    pub(crate) accessor_face_from_any: FuncId,
    pub(crate) closure_call_variadic: FuncId,
    pub(crate) closure_call_with_this: FuncId,
    pub(crate) template_object_begin: FuncId,
    pub(crate) template_object_str: FuncId,
    pub(crate) template_object_end: FuncId,
    pub(crate) anyv_to_callable_cell: FuncId,
    pub(crate) ctorany_register: FuncId,
    pub(crate) super_call_value: FuncId,
    /// §13.3.7 read off a Super Reference (base, key, receiver).
    pub(crate) super_prop_get: FuncId,
    /// §9.1.9 write through a Super Reference (base, key, value, receiver).
    pub(crate) super_prop_set: FuncId,
    /// §13.3.6 call off a Super Reference (base, key, receiver, args).
    pub(crate) super_prop_call: FuncId,
    pub(crate) heritage_check: FuncId,
    pub(crate) any_method_call: FuncId,
    pub(crate) any_method_call_opt: FuncId,
    pub(crate) any_index_method_call: FuncId,
    pub(crate) any_method_probe: FuncId,
    pub(crate) any_prop_delete: FuncId,
    pub(crate) any_prop_delete_soft: FuncId,
    pub(crate) any_member_set_soft: FuncId,
    pub(crate) any_member_set_with_receiver: FuncId,
    pub(crate) any_member_get_with_receiver: FuncId,
    pub(crate) reflect_set_prototype_of: FuncId,
    pub(crate) regexp_escape_any: FuncId,
    pub(crate) any_prop_has: FuncId,
    pub(crate) any_has_property: FuncId,
    pub(crate) arr_forin_key_live: FuncId,
    pub(crate) builtin_proto_method_value: FuncId,
    pub(crate) builtin_method_cell_tagged: FuncId,
    pub(crate) any_unbox_tag: FuncId,
    pub(crate) any_unbox_value: FuncId,
    pub(crate) any_cell_ptr: FuncId,
    pub(crate) any_unbox_value_owned: FuncId,
    pub(crate) any_unbox_settle: FuncId,
    pub(crate) any_box_drop: FuncId,
    pub(crate) class_cell_release: FuncId,
    pub(crate) any_box_rc_inc: FuncId,
    /// S-NEW 刀 2 — class object → boxed factory adapter registration.
    pub(crate) ctor_register: FuncId,
    /// S-NEW 刀 2 — `new <runtime value>(args…)`.
    pub(crate) construct: FuncId,
    /// S-NEW 刀 3 — §7.2.4 IsConstructor as a predicate.
    pub(crate) is_constructor: FuncId,
    pub(crate) print_any: FuncId,
    pub(crate) print_any_inline_top: FuncId,
    pub(crate) io_putc_out: FuncId,
    pub(crate) io_sink_to_stderr: FuncId,
    pub(crate) io_sink_to_stdout: FuncId,
    pub(crate) arr_print_i64_inline: FuncId,
    pub(crate) arr_print_f64_inline: FuncId,
    pub(crate) arr_print_bool_inline: FuncId,
    pub(crate) arr_print_str_inline: FuncId,
    pub(crate) arr_print_substr_inline: FuncId,
    pub(crate) map_print_outer: FuncId,
    pub(crate) set_print_outer: FuncId,
    pub(crate) fn_print_outer: FuncId,
    pub(crate) fnsig_to_str: FuncId,
    /// Chunk 798 — typed-tier `.name` registry rewire: FnSig
    /// receiver (raw fn body vaddr → registry name Str, miss `""`).
    pub(crate) fn_name_str: FuncId,
    pub(crate) fn_source_str: FuncId,
    pub(crate) closure_source_str: FuncId,
    /// RFC 20260721 刀 9 — plain-fn `.prototype` materialization.
    pub(crate) closure_prototype_any: FuncId,
    /// RFC 20260721 刀 4 — flavor-keyed closure `.constructor`.
    pub(crate) closure_ctor_value: FuncId,
    /// Chunk 798 — typed-tier `.name` registry rewire: Closure
    /// receiver (cell → method/bound/registry name chain).
    pub(crate) closure_name_str: FuncId,
    pub(crate) any_to_str: FuncId,
    pub(crate) any_sort_undef_pre: FuncId,
    pub(crate) any_to_str_prim: FuncId,
    pub(crate) any_to_str_box: FuncId,
    pub(crate) any_to_display_str: FuncId,
    pub(crate) obj_freeze: FuncId,
    pub(crate) obj_freeze_any: FuncId,
    pub(crate) obj_is_frozen: FuncId,
    pub(crate) obj_is_frozen_any: FuncId,
    pub(crate) obj_check_not_frozen: FuncId,
    /// RFC 20260806-declared-field-redefine — the typed field-store
    /// guard that asks about writability as well as frozen.
    pub(crate) obj_check_field_writable: FuncId,
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
    /// §27.2.4.7 step 2 any-lane probe: pass a boxed %Promise%
    /// through, else fulfilled_heap + REPR_ANY (kernel-side stamp).
    pub(crate) promise_resolve_any: FuncId,
    /// §27.2.4 static-slot patch consult pair (rotation 448).
    pub(crate) promise_ctor_patched: FuncId,
    pub(crate) promise_patched_result: FuncId,
    /// RFC 20260720-anylane-promise-methods knife 1 — stamp the
    /// cell's `value_repr` at a mint site whose static arg type
    /// fixes the storage form.
    pub(crate) promise_stamp_repr: FuncId,
    pub(crate) promise_drop: FuncId,
    pub(crate) promise_get_value: FuncId,
    pub(crate) promise_get_value_as: FuncId,
    pub(crate) anyv_await: FuncId,
    pub(crate) promise_then_simple: FuncId,
    pub(crate) promise_then_passthrough: FuncId,
    pub(crate) promise_then2: FuncId,
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
    pub(crate) promise_all_dyn: FuncId,
    pub(crate) promise_race_dyn: FuncId,
    pub(crate) promise_any_dyn: FuncId,
    pub(crate) promise_allsettled_dyn: FuncId,
    pub(crate) promise_all_keyed_dyn: FuncId,
    pub(crate) promise_allsettled_keyed_dyn: FuncId,
    pub(crate) array_from_async_dyn: FuncId,
    pub(crate) array_from_async_map_dyn: FuncId,
    pub(crate) promise_with_resolvers: FuncId,
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
    pub(crate) bigint_ctor_any: FuncId,
    pub(crate) bigint_to_number: FuncId,
    pub(crate) number_ctor_any: FuncId,
    pub(crate) any_unary_neg: FuncId,
    pub(crate) bigint_clone: FuncId,
    pub(crate) bigint_neg: FuncId,
    pub(crate) bigint_cmp: FuncId,
    pub(crate) bigint_is_nonzero: FuncId,
    pub(crate) bigint_to_string: FuncId,
    pub(crate) bigint_to_string_radix: FuncId,
    pub(crate) bigint_to_locale_string: FuncId,
    pub(crate) bigint_as_int_n: FuncId,
    pub(crate) bigint_as_uint_n: FuncId,
    pub(crate) bigint_drop_rc: FuncId,
    /// RFC 20260716 刀 2 — `__torajs_number_wrapper_new(val: f64) ->
    /// *mut u8` (torajs-wrapper). Emits from `ssa_lower_new`'s Number
    /// arm when the arg was primitive-coerced to f64. Result is an
    /// Any-boxed heap pointer carrying `Tag::NumberWrapper`.
    pub(crate) number_wrapper_new: FuncId,
    /// RFC 20260716 刀 2b — `__torajs_string_wrapper_new(cell: *mut u8)
    /// -> *mut u8` (torajs-wrapper). **Transfer semantics** — the
    /// caller's owned `+1` on `cell` is consumed. Result is an
    /// Any-boxed heap pointer carrying `Tag::StringWrapper`.
    pub(crate) string_wrapper_new: FuncId,
    /// RFC 20260716 刀 2c — `__torajs_boolean_wrapper_new(val: u8) ->
    /// *mut u8` (torajs-wrapper). Result is an Any-boxed heap pointer
    /// carrying `Tag::BooleanWrapper`.
    pub(crate) boolean_wrapper_new: FuncId,
    pub(crate) weakref_create: FuncId,
    pub(crate) weakref_deref_any: FuncId,
    pub(crate) weakref_drop: FuncId,
    pub(crate) weakref_target_dying: FuncId,
    pub(crate) collection_init_from_iterable: FuncId,
    pub(crate) collection_adder_resolve: FuncId,
    pub(crate) collection_add_static: FuncId,
    pub(crate) weakmap_create: FuncId,
    pub(crate) weakmap_set: FuncId,
    pub(crate) weakmap_get: FuncId,
    pub(crate) weakmap_get_or_insert: FuncId,
    pub(crate) weakmap_get_or_insert_computed: FuncId,
    pub(crate) weakmap_has: FuncId,
    pub(crate) weakmap_delete: FuncId,
    pub(crate) weakmap_drop: FuncId,
    /* RC-4 F2 — weak-key classification (ES CanBeHeldWeakly):
     * `from_any` reads illegal keys as absent (NULL), `or_throw`
     * records a pending TypeError for set/add. */
    pub(crate) weak_key_from_any: FuncId,
    pub(crate) weak_key_from_any_or_throw: FuncId,
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
    /* §24.2.1.2 GetSetRecord kernels — the typed lowering's route
     * for a non-Set setops argument (set_like declare group). */
    pub(crate) set_relation_setlike: FuncId,
    pub(crate) set_setop_setlike: FuncId,
    /* §13.3.8.1 spread-call kernels — dynamic-argc call over a
     * materialized Array<Any> argument list (spread declare group). */
    pub(crate) any_call_spread: FuncId,
    pub(crate) any_method_call_spread: FuncId,
    pub(crate) anyv_construct_spread: FuncId,
    pub(crate) map_clone: FuncId,
    pub(crate) map_set: FuncId,
    pub(crate) map_get: FuncId,
    pub(crate) map_get_or_insert: FuncId,
    pub(crate) map_get_or_insert_computed: FuncId,
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
    pub(crate) symbol_for_any: FuncId,
    pub(crate) symbol_key_for_any: FuncId,
    pub(crate) symbol_iterator: FuncId,
    pub(crate) symbol_async_iterator: FuncId,
    pub(crate) symbol_to_primitive: FuncId,
    pub(crate) symbol_well_known: FuncId,
    pub(crate) object_is_f64: FuncId,
    pub(crate) split_iter_init: FuncId,
    pub(crate) split_iter_next: FuncId,
    pub(crate) split_iter_drop: FuncId,
    pub(crate) arr_from_string: FuncId,
    pub(crate) str_substring: FuncId,
    pub(crate) str_substr: FuncId,
    pub(crate) arr_set_length_validate: FuncId,
    pub(crate) arr_set_length_truncate_scalar: FuncId,
    pub(crate) arr_set_length_any: FuncId,
    pub(crate) arr_to_reversed: FuncId,
    pub(crate) arr_with: FuncId,
    pub(crate) arr_flat_runtime_depth: FuncId,
    pub(crate) arr_oob_throw: FuncId,
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
    pub(crate) str_uri_encode: FuncId,
    pub(crate) str_uri_decode: FuncId,
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
    /// RFC 20260801-ns-object-value — Math namespace object value.
    pub(crate) ns_object_math: FuncId,
    /// RFC 20260812-console-sink knife 4 — console namespace object
    /// value (the WHATWG §1.1 five-logger singleton).
    pub(crate) ns_object_console: FuncId,
    /// RFC 20260801-ns-object-value (JSON extension) — JSON
    /// namespace object value.
    pub(crate) ns_object_json: FuncId,
    /// RFC 20260801-ns-object-value (Reflect extension) — Reflect
    /// namespace object value.
    pub(crate) ns_object_reflect: FuncId,
    /// §19.2.1 — the global `eval` cell as a value.
    pub(crate) global_eval_value: FuncId,
    /// RFC 20260807-global-object G2 — globalThis singleton value.
    pub(crate) globalthis_object: FuncId,
    pub(crate) json_quote_str: FuncId,
    pub(crate) json_quote_str_top: FuncId,
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
    pub(crate) jsb_begin_field: FuncId,
    pub(crate) jsb_push_field_str: FuncId,
    pub(crate) jsb_stringify_shape: FuncId,
    pub(crate) json_obj_sep: FuncId,
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
