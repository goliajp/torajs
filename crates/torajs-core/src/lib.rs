//! v0.3 #6 Graduation — torajs-core crate.
//!
//! The compiler library: lex → parse → desugar → typecheck → SSA
//! lower → inkwell-emit. Public modules let downstream callers
//! (`torajs-cli`, the conformance and bench harnesses) drive any
//! sub-stage of the pipeline directly.
//!
//! Depends on `torajs-runtime` for the C source files that get
//! embedded into every `tr build` artifact (string/array helpers,
//! regex/Date engines, ...) and on `torajs-rc` for the Layer-1
//! refcount staticlib that ssa_inkwell links into every user
//! binary alongside those C object files.

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
/// `ssa_inkwell::compile()` writes each entry to a per-build temp
/// `.a` file and appends every path to the cc link command.
/// Adding a new sub-crate is a single line here + a single line
/// in `build.rs`'s `STATICLIBS` list — no other change needed for
/// the link wiring to pick it up.
pub const TORAJS_STATICLIBS: &[(&str, &[u8])] = &[
    (
        "libtorajs_rc.a",
        include_bytes!(env!("TORAJS_RC_STATICLIB_PATH")),
    ),
    (
        "libtorajs_anyvalue.a",
        include_bytes!(env!("TORAJS_ANYVALUE_STATICLIB_PATH")),
    ),
];

pub mod ast;
pub mod check;
pub mod formatter;
pub mod lexer;
pub mod linter;
pub mod modules;
pub mod parser;
pub mod ssa;
pub mod ssa_inkwell;
pub mod ssa_lower;
