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

pub mod format;
pub mod layout;
pub mod math;
pub mod parse;
pub mod predicates;
pub mod str_bridge;
pub mod tostring;

// __torajs_str_alloc_pooled is provided by libtorajs_str.a at
// `tr build` link time. For cargo unit tests of torajs-num
// (which don't link torajs-str), provide a panicking stub so the
// test binary still links — torajs-num's unit tests only exercise
// pure-Rust cores (Vec<u8>-returning fns), never the extern
// wrappers that allocate Str.
#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_alloc_pooled(_len: u64) -> *mut u8 {
    panic!(
        "torajs-num unit-test stub: __torajs_str_alloc_pooled should not be called from cargo test paths"
    );
}

// Re-export — keep this list tight; the extern "C" symbols are
// resolved at link time by ssa_inkwell-emitted IR regardless.
pub use format::{
    __torajs_num_to_exp_f, __torajs_num_to_exp_i, __torajs_num_to_fixed_f, __torajs_num_to_fixed_i,
    __torajs_num_to_precision_f, __torajs_num_to_precision_i,
};
pub use math::{
    __torajs_math_abs, __torajs_math_acos, __torajs_math_acosh, __torajs_math_asin,
    __torajs_math_asinh, __torajs_math_atan, __torajs_math_atan2, __torajs_math_atanh,
    __torajs_math_cbrt, __torajs_math_ceil, __torajs_math_cos, __torajs_math_cosh,
    __torajs_math_exp, __torajs_math_expm1, __torajs_math_floor, __torajs_math_log,
    __torajs_math_log1p, __torajs_math_log2, __torajs_math_log10, __torajs_math_max,
    __torajs_math_min, __torajs_math_pow, __torajs_math_round, __torajs_math_sin,
    __torajs_math_sinh, __torajs_math_sqrt, __torajs_math_tan, __torajs_math_tanh,
    __torajs_math_trunc,
};
pub use parse::{__torajs_num_parse_float, __torajs_num_parse_int};
pub use predicates::{
    __torajs_num_is_finite_f, __torajs_num_is_finite_i, __torajs_num_is_integer_f,
    __torajs_num_is_integer_i, __torajs_num_is_nan_f, __torajs_num_is_nan_i,
    __torajs_num_is_safe_integer_f, __torajs_num_is_safe_integer_i,
};
pub use tostring::{__torajs_num_to_string_radix_f, __torajs_num_to_string_radix_i};
