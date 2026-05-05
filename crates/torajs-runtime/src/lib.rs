//! v0.3 #6 Graduation — torajs-runtime crate.
//!
//! Holds the C source files that get embedded into every `tr build`
//! artifact. The compiler crate (`torajs-core`) consumes [`SOURCES`]
//! to write them into the per-build temp dir and `cc -c` each one,
//! linking the resulting objects into the final binary.
//!
//! Splitting the runtime into its own crate locks the C-side ABI
//! (universal heap header, refcount intrinsics, string/array
//! helpers, regex/Date engines) so the compiler can evolve without
//! accidentally breaking what's been embedded into already-shipped
//! binaries. Future room: a `build.rs` that pre-compiles into a
//! static `libtorajs_runtime.a` for cross-compile and link-time
//! deduplication; for now we keep the compile-on-emit shape that
//! `tr build` already had.

/// String / Array / Object / Number / JSON helpers + universal heap
/// header + ARC intrinsics (`__torajs_rc_inc` / `__torajs_rc_dec`)
/// + the small-Str and Array LIFO pools + libc panic backtrace.
pub const RUNTIME_STR_C: &str = include_str!("runtime_str.c");

/// v0.2 #1 — regex matching engine.
pub const RUNTIME_REGEX_C: &str = include_str!("runtime_regex.c");

/// v0.2 #2 — Date class implementation.
pub const RUNTIME_DATE_C: &str = include_str!("runtime_date.c");

/// All C runtime translation units in (filename, contents) form, in
/// the order they should be written + cc'd. Filename is the basename
/// the compiler should write into the per-build temp directory.
pub const SOURCES: &[(&str, &str)] = &[
    ("runtime_str.c", RUNTIME_STR_C),
    ("runtime_regex.c", RUNTIME_REGEX_C),
    ("runtime_date.c", RUNTIME_DATE_C),
];
