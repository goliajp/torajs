//! Number primitives + Math namespace for the torajs AOT TypeScript
//! runtime.
//!
//! Layer-2 substrate of the architecture rewrite
//! (`docs/architecture-rewrite.md` P3.2). Provides:
//!
//! - [`math`] — Math intrinsics (`__torajs_math_sqrt` /
//!   `__torajs_math_abs` / ...). Each is a thin `extern "C"`
//!   wrapper over Rust's `f64::X(self)` methods, which libc-link
//!   the same libm operations the IR-emitted versions did. Bit-
//!   for-bit identical output, single fn call from the IR's
//!   perspective.
//!
//! ## Sub-step matrix
//!
//! | Phase    | Adds                                                       |
//! |----------|------------------------------------------------------------|
//! | P3.2-a   | Scaffold + Math.sqrt (single-fn pipeline verify)           |
//! | P3.2-b   | Bulk port remaining 22 Math intrinsics + helper cleanup    |
//! | P3.2-c   | ToNumber (any.toNumber + cross-cutting num coerce)         |
//!
//! ## Why `std`, not `no_std`
//!
//! Same reason as [`torajs-rc`] / [`torajs-str`] / etc.: cargo's
//! `cargo test` + dual `crate-type = ["rlib", "staticlib"]` +
//! `no_std` combination triggers a precompiled-core panic-strategy
//! mismatch that has no clean fix on stable. `std` staticlibs link
//! cleanly at `tr build` time (cc + LLVM-LTO tolerates std symbol
//! overlap between Rust-emitted .a's).

pub mod math;

// Re-export the small surface the rest of the workspace (and the
// FFI consumers) reach for most often.
pub use math::__torajs_math_sqrt;
