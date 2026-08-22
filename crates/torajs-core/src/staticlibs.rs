//! Embedded staticlib bytes + compiler-source fingerprint, both
//! emitted by `build.rs` env vars and consumed at compile time.
//! Split out of `lib.rs` so the crate root stays a pure module
//! scaffold; `lib.rs` re-exports both constants so the caller
//! path (`torajs_core::TORAJS_STATICLIBS`) is unchanged.

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
    // The allocator override has to be searched first. Every
    // `libtorajs_*.a` bundles its own copy of `core`, and that copy
    // carries the default `__rust_alloc -> __rdl_alloc` shim; the
    // merge is first-archive-wins (ld64 search order), so whichever
    // archive leads the list decides which allocator the whole user
    // binary gets. This crate is the one holding
    // `#[global_allocator] static GLOBAL: TorajsAllocator`, and it
    // only wins from the front. It also owns `rust_eh_personality` /
    // `__rust_start_panic` / the alloc-error handler, so front is
    // where it belonged anyway.
    //
    // It used to sit near the end, which is why user binaries were
    // silently routing every Rust-side Vec/Box/String to libc malloc
    // instead of the mmalloc TLAB — visible only in a leaf-symbol
    // profile as `_xzm_xzone_malloc` / `_xzm_free` frames under
    // `__torajs_jsb_new`, and confirmed by `__rust_alloc: b
    // __rdl_alloc` in the shipped binary. The crate's own comment
    // asserted "this staticlib resolves first"; with the in-house
    // linker that was never true.
    (
        "libtorajs_panic_runtime.a",
        include_bytes!(env!("TORAJS_PANIC_RUNTIME_STATICLIB_PATH")),
    ),
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
        "libtorajs_value_drop.a",
        include_bytes!(env!("TORAJS_VALUE_DROP_STATICLIB_PATH")),
    ),
    (
        "libtorajs_buffer.a",
        include_bytes!(env!("TORAJS_BUFFER_STATICLIB_PATH")),
    ),
    (
        "libtorajs_wrapper.a",
        include_bytes!(env!("TORAJS_WRAPPER_STATICLIB_PATH")),
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
