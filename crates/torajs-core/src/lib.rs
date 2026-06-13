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
pub(crate) mod check_assignable;
pub(crate) mod check_type_ann;
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
pub(crate) mod ssa_lower_any_cast;
pub(crate) mod ssa_lower_arr_mutators;
pub mod ssa_lower_body_returns_closure;
pub mod ssa_lower_closure_captures;
pub(crate) mod ssa_lower_container_width;
pub mod ssa_lower_deque_escape;
pub(crate) mod ssa_lower_drops;
pub(crate) mod ssa_lower_index_assign;
pub(crate) mod ssa_lower_json_parse;
pub(crate) mod ssa_lower_logical;
pub mod ssa_lower_main_exit;
pub mod ssa_lower_obj_escape;
pub(crate) mod ssa_lower_objlit_layout;
pub(crate) mod ssa_lower_parse_type;
pub mod ssa_lower_process_on;
pub(crate) mod ssa_lower_promise_chain;
pub(crate) mod ssa_lower_promise_thunk;
pub mod ssa_lower_push_loop_detect;
pub mod ssa_lower_str;
pub mod ssa_lower_substr_trim_into;
pub mod ssa_lower_toplevel_globals;
pub(crate) mod ssa_lower_unary;
pub mod ssa_lower_while_push_fast;
