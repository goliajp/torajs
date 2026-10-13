//! The embedded runtime image + compiler-source fingerprint, both
//! derived from `build.rs` env vars at compile time. Split out of
//! `lib.rs` so the crate root stays a pure module scaffold; `lib.rs`
//! re-exports both constants.

/// The runtime image: every Layer-1+ Rust sub-crate's `lib<name>.a`
/// (as produced by `cargo build -p <crate>` in the active profile),
/// preprocessed by `torajs_rtimage::image::build_reachable` into the
/// flat layout the linker reads without parsing — only the members a
/// program can pull, the merged first-archive-wins symbol index
/// sorted.
///
/// The proc macro runs inside THIS crate's compilation, which cargo's
/// dep graph schedules AFTER every sub-crate finishes building (and
/// thus AFTER each `lib<name>.a` is fully written to
/// `target/<profile>/`). Reading each path from a build-script-emitted
/// env var (`TORAJS_<NAME>_STATICLIB_PATH`) instead of an OUT_DIR copy
/// avoids the build.rs race where the script can run BEFORE the
/// staticlib artifact is finalized — embedding a stale copy into `tr`.
/// See `build.rs` for the full rationale. The expansion also names
/// every archive in rustc's dep-info, so a sub-crate edit rebuilds
/// this crate exactly as the direct `include_bytes!` did before.
///
/// Adding a new sub-crate is a single line here + a single line in
/// `build.rs`'s `STATICLIBS` list — no other change needed for the
/// link wiring to pick it up.
///
/// The allocator override has to be searched first. Every
/// `libtorajs_*.a` bundles its own copy of `core`, and that copy
/// carries the default `__rust_alloc -> __rdl_alloc` shim; the merge
/// is first-archive-wins (ld64 search order), so whichever archive
/// leads the list decides which allocator the whole user binary gets.
/// `torajs-panic-runtime` is the crate holding `#[global_allocator]
/// static GLOBAL: TorajsAllocator`, and it only wins from the front.
/// It also owns `rust_eh_personality` / `__rust_start_panic` / the
/// alloc-error handler, so front is where it belonged anyway.
///
/// It used to sit near the end, which is why user binaries were
/// silently routing every Rust-side Vec/Box/String to libc malloc
/// instead of the mmalloc TLAB — visible only in a leaf-symbol profile
/// as `_xzm_xzone_malloc` / `_xzm_free` frames under
/// `__torajs_jsb_new`, and confirmed by `__rust_alloc: b __rdl_alloc`
/// in the shipped binary. The crate's own comment asserted "this
/// staticlib resolves first"; with the in-house linker that was never
/// true.
#[cfg(feature = "runtime-image")]
pub static TORAJS_RUNTIME_IMAGE: &[u8] =
    crate::runtime_staticlib_envs!(torajs_rtimage_macro::runtime_image);

/// Expands to `$m!($($arg,)* "TORAJS_<NAME>_STATICLIB_PATH", ...)` with
/// every runtime staticlib in link search order, so each image built
/// from the archives reads them in the one order documented above.
#[macro_export]
macro_rules! runtime_staticlib_envs {
    ($($m:ident)::+ $(, $arg:literal)*) => {
        $($m)::+!(
            $($arg,)*
            "TORAJS_PANIC_RUNTIME_STATICLIB_PATH",
            "TORAJS_SYSCALL_STATICLIB_PATH",
            "TORAJS_IO_STATICLIB_PATH",
            "TORAJS_FMT_STATICLIB_PATH",
            "TORAJS_MEM_STATICLIB_PATH",
            "TORAJS_MMALLOC_STATICLIB_PATH",
            "TORAJS_RC_STATICLIB_PATH",
            "TORAJS_ANYVALUE_STATICLIB_PATH",
            "TORAJS_DISPATCH_STATICLIB_PATH",
            "TORAJS_THROW_STATICLIB_PATH",
            "TORAJS_STR_STATICLIB_PATH",
            "TORAJS_NUM_STATICLIB_PATH",
            "TORAJS_MATH_STATICLIB_PATH",
            "TORAJS_BIGINT_STATICLIB_PATH",
            "TORAJS_ARR_STATICLIB_PATH",
            "TORAJS_DYNOBJ_STATICLIB_PATH",
            "TORAJS_COLLECTIONS_STATICLIB_PATH",
            "TORAJS_WEAK_STATICLIB_PATH",
            "TORAJS_CYCLE_STATICLIB_PATH",
            "TORAJS_MICROTASK_STATICLIB_PATH",
            "TORAJS_PROMISE_STATICLIB_PATH",
            "TORAJS_REGEX_STATICLIB_PATH",
            "TORAJS_FETCH_STATICLIB_PATH",
            "TORAJS_DATE_STATICLIB_PATH",
            "TORAJS_CAPTURE_BOX_STATICLIB_PATH",
            "TORAJS_FS_STATICLIB_PATH",
            "TORAJS_META_STATICLIB_PATH",
            "TORAJS_STRUCTMETA_STATICLIB_PATH",
            "TORAJS_PROCESS_STATICLIB_PATH",
            "TORAJS_PANIC_STATICLIB_PATH",
            "TORAJS_VALUE_DROP_STATICLIB_PATH",
            "TORAJS_BUFFER_STATICLIB_PATH",
            "TORAJS_WRAPPER_STATICLIB_PATH",
            "TORAJS_ABORT_STATICLIB_PATH",
            "TORAJS_PRINT_STATICLIB_PATH",
            "TORAJS_FNNAME_STATICLIB_PATH",
        )
    };
}

/// Compiler-source fingerprint emitted by build.rs (hash of
/// `src/ssa_lower.rs` / `src/check.rs` / `src/parser.rs` /
/// `src/lexer.rs` / `src/ast.rs` / `src/modules.rs` / `src/ssa.rs`).
/// Used by the per-fixture `.o` cache (B-1 phase 2): substrate ships
/// don't touch these `.rs` files → fingerprint stable across ships →
/// `.o` cache stays warm even though tr binary mtime changes.
/// Compiler-logic ships (touching any file above) flip the fingerprint
/// and invalidate the cache — correct semantics.
pub const TORAJS_COMPILER_REV: &str = env!("TORAJS_COMPILER_REV");
