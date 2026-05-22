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

/// `libtorajs_rc.a` bytes — the Layer-1 staticlib produced by
/// `crates/torajs-rc`'s `crate-type = ["staticlib"]` artifact.
///
/// `build.rs` copies the `.a` from `target/<profile>/` into the
/// build script's `OUT_DIR` so `include_bytes!` can resolve it at
/// compile time. `ssa_inkwell::compile()` writes these bytes to a
/// per-build temp file and appends the path to the link command,
/// which is how `__torajs_rc_inc` / `__torajs_rc_dec` /
/// `HeapHeader`-aware drop dispatch end up resolved in the final
/// AOT user binary now that those symbols are no longer defined
/// in `runtime_str.c`.
pub const TORAJS_RC_STATICLIB: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/libtorajs_rc.a"));

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
