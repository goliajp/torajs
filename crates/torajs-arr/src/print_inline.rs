//! `console.log(arr)` no-\n typed walker family — multi-arg joiner
//! substrate for ssa-lower's `lower_top_stmt` multi-arg path.
//!
//! Thin delegates over the shared break/wrap walker in
//! [`crate::print_typed`] (inspect wrap trunk chunk C) — same bun
//! form as the trailing-`'\n'` family in [`crate::print`], minus the
//! newline (the multi-arg console.log joiner splices `' '`
//! separators and one final `'\n'` itself).
//!
//! Each entry resets the line estimate per arg — an approximation of
//! bun's single estimate across all args of one console.log call
//! (matches `__torajs_print_anyv_inline_top`; divergence only at the
//! 80-column wrap edge across args).

use core::ffi::c_void;

use crate::print_typed::{TypedKind, print_typed_top};

/// `console.log(arr: Array<I64>)` inline (no-\n) variant.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_print_i64_inline(arr: *const c_void) {
    unsafe { print_typed_top(arr, TypedKind::I64) }
}

/// `console.log(arr: Array<F64>)` inline (no-\n) variant. JS-spec
/// NaN / Infinity / -Infinity literals, else `%g` shortest-roundtrip
/// per parent family.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_print_f64_inline(arr: *const c_void) {
    unsafe { print_typed_top(arr, TypedKind::F64) }
}

/// `console.log(arr: Array<Bool>)` inline (no-\n) variant. Slots are
/// i64 (0 / non-0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_print_bool_inline(arr: *const c_void) {
    unsafe { print_typed_top(arr, TypedKind::Bool) }
}

/// `console.log(arr: Array<Str>)` inline (no-\n) variant. Each slot
/// is a `*Str` (NULL → `undefined`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_print_str_inline(arr: *const c_void) {
    unsafe { print_typed_top(arr, TypedKind::Str) }
}

/// `console.log(arr: Array<Substr>)` inline (no-\n) variant. Each
/// slot is a `*Substr` — layout differs from Str (parent_ptr +
/// offset instead of inline bytes).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_print_substr_inline(arr: *const c_void) {
    unsafe { print_typed_top(arr, TypedKind::Substr) }
}
