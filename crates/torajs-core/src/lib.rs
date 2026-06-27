//! v0.3 #6 Graduation — torajs-core crate.
//!
//! The compiler library: lex → parse → desugar → typecheck → SSA
//! lower. Downstream AOT codegen + Mach-O emit + linking lives in
//! torajs-codegen → torajs-obj → torajs-link. Public modules let
//! downstream callers (`torajs-cli`, the conformance and bench
//! harnesses) drive any sub-stage of the pipeline directly.
//!
//! Depends on `torajs-rc` (and the other Layer-1+ sub-crates via
//! build.rs env vars) for the staticlibs that torajs-link bakes
//! into every `tr build` user binary.

/// Embedded staticlib bytes for every Layer-1+ Rust sub-crate
/// that contributes `__torajs_*` symbols to the final `tr build`
/// user binary. Each entry is the bytes of `lib<name>.a` as
/// produced by `cargo build -p <crate>` in the active profile.
///
/// `include_bytes!` resolves at THIS crate's compile time, which
/// cargo's dep graph guarantees runs AFTER every sub-crate finishes
/// building (and thus AFTER each `lib<name>.a` is fully written
/// to `target/<profile>/`). Reading the path from a build-script-
/// emitted env var (`TORAJS_<NAME>_STATICLIB_PATH`) instead of via
/// an OUT_DIR copy avoids the build.rs race where the script can
/// run BEFORE the staticlib artifact is finalized — embedding a
/// stale copy into `tr`. See `build.rs` for the full rationale.
///
/// `torajs-link` writes each entry to a per-build temp `.a` file and
/// hands the paths to the in-house Mach-O linker. Adding a new
/// sub-crate is a single line here + a single line in `build.rs`'s
/// `STATICLIBS` list — no other change needed for the link wiring to
/// pick it up.
pub const TORAJS_STATICLIBS: &[(&str, &[u8])] = &[
    (
        "libtorajs_syscall.a",
        include_bytes!(env!("TORAJS_SYSCALL_STATICLIB_PATH")),
    ),
    (
        "libtorajs_io.a",
        include_bytes!(env!("TORAJS_IO_STATICLIB_PATH")),
    ),
    (
        "libtorajs_fmt.a",
        include_bytes!(env!("TORAJS_FMT_STATICLIB_PATH")),
    ),
    (
        "libtorajs_mem.a",
        include_bytes!(env!("TORAJS_MEM_STATICLIB_PATH")),
    ),
    (
        "libtorajs_mmalloc.a",
        include_bytes!(env!("TORAJS_MMALLOC_STATICLIB_PATH")),
    ),
    (
        "libtorajs_rc.a",
        include_bytes!(env!("TORAJS_RC_STATICLIB_PATH")),
    ),
    (
        "libtorajs_anyvalue.a",
        include_bytes!(env!("TORAJS_ANYVALUE_STATICLIB_PATH")),
    ),
    (
        "libtorajs_throw.a",
        include_bytes!(env!("TORAJS_THROW_STATICLIB_PATH")),
    ),
    (
        "libtorajs_str.a",
        include_bytes!(env!("TORAJS_STR_STATICLIB_PATH")),
    ),
    (
        "libtorajs_num.a",
        include_bytes!(env!("TORAJS_NUM_STATICLIB_PATH")),
    ),
    (
        "libtorajs_math.a",
        include_bytes!(env!("TORAJS_MATH_STATICLIB_PATH")),
    ),
    (
        "libtorajs_bigint.a",
        include_bytes!(env!("TORAJS_BIGINT_STATICLIB_PATH")),
    ),
    (
        "libtorajs_arr.a",
        include_bytes!(env!("TORAJS_ARR_STATICLIB_PATH")),
    ),
    (
        "libtorajs_dynobj.a",
        include_bytes!(env!("TORAJS_DYNOBJ_STATICLIB_PATH")),
    ),
    (
        "libtorajs_collections.a",
        include_bytes!(env!("TORAJS_COLLECTIONS_STATICLIB_PATH")),
    ),
    (
        "libtorajs_weak.a",
        include_bytes!(env!("TORAJS_WEAK_STATICLIB_PATH")),
    ),
    (
        "libtorajs_cycle.a",
        include_bytes!(env!("TORAJS_CYCLE_STATICLIB_PATH")),
    ),
    (
        "libtorajs_microtask.a",
        include_bytes!(env!("TORAJS_MICROTASK_STATICLIB_PATH")),
    ),
    (
        "libtorajs_promise.a",
        include_bytes!(env!("TORAJS_PROMISE_STATICLIB_PATH")),
    ),
    (
        "libtorajs_regex.a",
        include_bytes!(env!("TORAJS_REGEX_STATICLIB_PATH")),
    ),
    (
        "libtorajs_fetch.a",
        include_bytes!(env!("TORAJS_FETCH_STATICLIB_PATH")),
    ),
    (
        "libtorajs_date.a",
        include_bytes!(env!("TORAJS_DATE_STATICLIB_PATH")),
    ),
    (
        "libtorajs_capture_box.a",
        include_bytes!(env!("TORAJS_CAPTURE_BOX_STATICLIB_PATH")),
    ),
    (
        "libtorajs_fs.a",
        include_bytes!(env!("TORAJS_FS_STATICLIB_PATH")),
    ),
    (
        "libtorajs_meta.a",
        include_bytes!(env!("TORAJS_META_STATICLIB_PATH")),
    ),
    (
        "libtorajs_structmeta.a",
        include_bytes!(env!("TORAJS_STRUCTMETA_STATICLIB_PATH")),
    ),
    (
        "libtorajs_process.a",
        include_bytes!(env!("TORAJS_PROCESS_STATICLIB_PATH")),
    ),
    (
        "libtorajs_panic.a",
        include_bytes!(env!("TORAJS_PANIC_STATICLIB_PATH")),
    ),
    (
        "libtorajs_panic_runtime.a",
        include_bytes!(env!("TORAJS_PANIC_RUNTIME_STATICLIB_PATH")),
    ),
    (
        "libtorajs_value_drop.a",
        include_bytes!(env!("TORAJS_VALUE_DROP_STATICLIB_PATH")),
    ),
    (
        "libtorajs_abort.a",
        include_bytes!(env!("TORAJS_ABORT_STATICLIB_PATH")),
    ),
    (
        "libtorajs_print.a",
        include_bytes!(env!("TORAJS_PRINT_STATICLIB_PATH")),
    ),
    (
        "libtorajs_fnname.a",
        include_bytes!(env!("TORAJS_FNNAME_STATICLIB_PATH")),
    ),
];

/// Compiler-source fingerprint emitted by build.rs (hash of
/// `src/ssa_lower.rs` / `src/check.rs` / `src/parser.rs` /
/// `src/lexer.rs` / `src/ast.rs` / `src/modules.rs` / `src/ssa.rs`).
/// Used by the per-fixture `.o` cache (B-1 phase 2): substrate ships
/// don't touch these `.rs` files → fingerprint stable across ships →
/// `.o` cache stays warm even though tr binary mtime changes.
/// Compiler-logic ships (touching any file above) flip the fingerprint
/// and invalidate the cache — correct semantics.
pub const TORAJS_COMPILER_REV: &str = env!("TORAJS_COMPILER_REV");

pub mod ast;
pub mod ast_closure_param_tag;
pub mod ast_refs;
pub mod ast_throw_info;
pub mod check;
pub(crate) mod check_assign_ident;
pub(crate) mod check_assign_target;
pub(crate) mod check_assignable;
pub(crate) mod check_stmt_fn_decl;
pub(crate) mod check_stmt_for;
pub(crate) mod check_stmt_for_of;
pub(crate) mod check_stmt_for_of_split;
pub(crate) mod check_stmt_if;
pub(crate) mod check_stmt_let_decl;
pub(crate) mod check_stmt_misc;
pub(crate) mod check_stmt_return;
pub(crate) mod check_stmt_throw;
pub(crate) mod check_stmt_try;
pub(crate) mod check_stmt_while;
pub(crate) mod check_type_ann;
pub(crate) mod check_type_ann_substitute;
pub(crate) mod check_type_of_array;
pub(crate) mod check_type_of_binop;
pub(crate) mod check_type_of_fn;
pub(crate) mod check_type_of_ident;
pub(crate) mod check_type_of_misc;
pub(crate) mod check_type_of_new;
pub(crate) mod check_type_of_object_lit;
pub(crate) mod check_type_of_ternary;
pub(crate) mod check_type_of_unary;
pub(crate) mod check_typevar;
pub(crate) mod cm_demote;
pub mod formatter;
pub mod lexer;
pub mod linter;
pub mod modules;
pub mod num_width;
pub mod parser;
pub mod short_str_encode;
pub mod ssa;
pub mod ssa_lower;
pub(crate) mod ssa_lower_accessor;
pub(crate) mod ssa_lower_anon_stamp;
pub(crate) mod ssa_lower_any_cast;
pub(crate) mod ssa_lower_any_member;
pub(crate) mod ssa_lower_arr_any_fill;
pub(crate) mod ssa_lower_arr_any_push;
pub(crate) mod ssa_lower_arr_from_set;
pub(crate) mod ssa_lower_arr_mutators;
pub(crate) mod ssa_lower_array;
pub(crate) mod ssa_lower_assign_ident;
pub(crate) mod ssa_lower_assign_member;
pub(crate) mod ssa_lower_binop;
pub(crate) mod ssa_lower_binop_loose_eq;
pub(crate) mod ssa_lower_binop_null_undef;
pub mod ssa_lower_body_returns_closure;
pub(crate) mod ssa_lower_call;
pub(crate) mod ssa_lower_call_arr_flat_map;
pub(crate) mod ssa_lower_call_arr_ho;
pub(crate) mod ssa_lower_call_arr_ho_loop;
pub(crate) mod ssa_lower_call_arr_iter_ctor;
pub(crate) mod ssa_lower_call_arr_predicate;
pub(crate) mod ssa_lower_call_arr_push;
pub(crate) mod ssa_lower_call_array_from;
pub(crate) mod ssa_lower_call_array_is_array;
pub(crate) mod ssa_lower_call_array_of;
pub(crate) mod ssa_lower_call_bare_globals;
pub(crate) mod ssa_lower_call_bigint_as_int_n;
pub(crate) mod ssa_lower_call_bigint_ctor;
pub(crate) mod ssa_lower_call_bun_runtime;
pub(crate) mod ssa_lower_call_class_synth;
pub(crate) mod ssa_lower_call_closure_local;
pub(crate) mod ssa_lower_call_coercion;
pub(crate) mod ssa_lower_call_console;
pub(crate) mod ssa_lower_call_date_methods;
pub(crate) mod ssa_lower_call_date_parse_undef_fold;
pub(crate) mod ssa_lower_call_date_utc_pad;
pub(crate) mod ssa_lower_call_fn_indirect;
pub(crate) mod ssa_lower_call_fs_promises;
pub(crate) mod ssa_lower_call_has_own;
pub(crate) mod ssa_lower_call_in_op;
pub(crate) mod ssa_lower_call_iter_next;
pub(crate) mod ssa_lower_call_json_stringify;
pub(crate) mod ssa_lower_call_map_dispatch;
pub(crate) mod ssa_lower_call_math_binary_undef_fold;
pub(crate) mod ssa_lower_call_math_hypot;
pub(crate) mod ssa_lower_call_math_min_max;
pub(crate) mod ssa_lower_call_math_unary_undef_fold;
pub(crate) mod ssa_lower_call_namespace_obj_methods;
pub(crate) mod ssa_lower_call_number_methods;
pub(crate) mod ssa_lower_call_number_namespace;
pub(crate) mod ssa_lower_call_object_assign;
pub(crate) mod ssa_lower_call_object_entries;
pub(crate) mod ssa_lower_call_object_get_property_descriptor;
pub(crate) mod ssa_lower_call_object_get_prototype_of;
pub(crate) mod ssa_lower_call_object_integrity;
pub(crate) mod ssa_lower_call_object_is;
pub(crate) mod ssa_lower_call_object_keys;
pub(crate) mod ssa_lower_call_object_values;
pub(crate) mod ssa_lower_call_promise_static;
pub(crate) mod ssa_lower_call_reflect_get;
pub(crate) mod ssa_lower_call_regex_methods;
pub(crate) mod ssa_lower_call_set_dispatch;
pub(crate) mod ssa_lower_call_sibling_class_dispatch;
pub(crate) mod ssa_lower_call_str_regex_methods;
pub(crate) mod ssa_lower_call_string_from_char_code;
pub(crate) mod ssa_lower_call_struct_method_dispatch;
pub(crate) mod ssa_lower_call_symbol_ctor;
pub(crate) mod ssa_lower_call_symbol_registry;
pub(crate) mod ssa_lower_call_terminal;
pub(crate) mod ssa_lower_call_universal_methods;
pub(crate) mod ssa_lower_call_vtable_dispatch;
pub(crate) mod ssa_lower_call_weakref_collections;
pub(crate) mod ssa_lower_closure;
pub mod ssa_lower_closure_captures;
pub(crate) mod ssa_lower_console_log_multiarg;
pub(crate) mod ssa_lower_container_width;
pub mod ssa_lower_deque_escape;
pub(crate) mod ssa_lower_drops;
pub(crate) mod ssa_lower_ident;
pub(crate) mod ssa_lower_index;
pub(crate) mod ssa_lower_index_assign;
pub(crate) mod ssa_lower_instanceof;
pub(crate) mod ssa_lower_intrinsics_arr;
pub(crate) mod ssa_lower_intrinsics_arr_any;
pub(crate) mod ssa_lower_intrinsics_date;
pub(crate) mod ssa_lower_intrinsics_fs;
pub(crate) mod ssa_lower_intrinsics_num;
pub(crate) mod ssa_lower_intrinsics_obj_capture;
pub(crate) mod ssa_lower_intrinsics_object;
pub(crate) mod ssa_lower_intrinsics_print_str;
pub(crate) mod ssa_lower_intrinsics_process;
pub(crate) mod ssa_lower_intrinsics_regex;
pub(crate) mod ssa_lower_intrinsics_str_a;
pub(crate) mod ssa_lower_intrinsics_str_b;
pub(crate) mod ssa_lower_json_parse;
pub(crate) mod ssa_lower_lit;
pub(crate) mod ssa_lower_logical;
pub mod ssa_lower_main_exit;
pub(crate) mod ssa_lower_member;
pub(crate) mod ssa_lower_member_builtin_namespace;
pub(crate) mod ssa_lower_member_fn_intro;
pub(crate) mod ssa_lower_member_obj_field;
pub(crate) mod ssa_lower_member_process;
pub(crate) mod ssa_lower_member_promise_value;
pub(crate) mod ssa_lower_member_props_read;
pub(crate) mod ssa_lower_member_regexp_props;
pub(crate) mod ssa_lower_member_symbol_wellknown;
pub(crate) mod ssa_lower_member_typed_props;
pub(crate) mod ssa_lower_member_web_runtime;
pub(crate) mod ssa_lower_new;
pub(crate) mod ssa_lower_new_arr_init;
pub(crate) mod ssa_lower_nullish;
pub mod ssa_lower_obj_escape;
pub(crate) mod ssa_lower_object_define;
pub(crate) mod ssa_lower_object_lit;
pub(crate) mod ssa_lower_objlit_layout;
pub(crate) mod ssa_lower_optchain;
pub(crate) mod ssa_lower_optchain_arm;
pub(crate) mod ssa_lower_parse_fn_type;
pub(crate) mod ssa_lower_parse_type;
pub(crate) mod ssa_lower_post_incr;
pub mod ssa_lower_process_on;
pub(crate) mod ssa_lower_promise_chain;
pub(crate) mod ssa_lower_promise_thunk;
pub mod ssa_lower_push_loop_detect;
pub(crate) mod ssa_lower_regex_bake;
pub(crate) mod ssa_lower_splice;
pub mod ssa_lower_str;
pub(crate) mod ssa_lower_str_arr_at;
pub(crate) mod ssa_lower_str_arr_copy_within;
pub(crate) mod ssa_lower_str_arr_fill;
pub(crate) mod ssa_lower_str_arr_index_coerce;
pub(crate) mod ssa_lower_str_arr_index_eq;
pub(crate) mod ssa_lower_str_arr_index_search;
pub(crate) mod ssa_lower_str_arr_inplace;
pub(crate) mod ssa_lower_str_arr_join_flat;
pub(crate) mod ssa_lower_str_arr_slice;
pub(crate) mod ssa_lower_str_arr_sort;
pub(crate) mod ssa_lower_str_charat_wedges;
pub(crate) mod ssa_lower_str_concat_dispatch;
pub(crate) mod ssa_lower_str_short_circuits;
pub(crate) mod ssa_lower_str_str_argv;
pub(crate) mod ssa_lower_str_str_defaults;
pub(crate) mod ssa_lower_str_str_dispatch;
pub(crate) mod ssa_lower_str_str_intrinsic;
pub(crate) mod ssa_lower_str_str_split;
pub(crate) mod ssa_lower_str_str_trailing;
pub(crate) mod ssa_lower_str_substr_dispatch;
pub mod ssa_lower_substr_trim_into;
pub(crate) mod ssa_lower_ternary;
pub mod ssa_lower_toplevel_globals;
pub(crate) mod ssa_lower_tospliced;
pub(crate) mod ssa_lower_typeof;
pub(crate) mod ssa_lower_unary;
pub mod ssa_lower_while_push_fast;
