//! `extern "C"` wrappers over [`crate::format`]'s pure-Rust cores
//! (chunk 766 extraction — format.rs had drifted past the 500-line
//! file limit; the B-0 two-layer split puts the thin FFI shells in
//! their own file). Symbols / signatures unchanged; each shell is
//! range-gate + `alloc_str` only.

use crate::format::{
    special_value, to_exp_f, to_exp_i, to_fixed_f, to_fixed_i, to_locale_f, to_locale_i,
    to_precision_f, to_precision_i,
};
use crate::str_bridge::alloc_str;

unsafe extern "C" {
    /// Cross-tier — torajs-throw. Records a pending RangeError via
    /// TLS; returns normally so the caller's `emit_throw_check`
    /// after the call site propagates the throw.
    fn __torajs_throw_range_error(msg: *const u8);
}

/// `n.toFixed(digits)` for f64 receivers.
///
/// ES §22.1.3.32 step 3 — `digits` must be in `[0, 100]`; otherwise
/// `RangeError` is thrown. Pre-fix tr's helper clamped silently via
/// `digits.clamp(0, 20)` so `(1.5).toFixed(-1)` returned `"2"`
/// instead of throwing. Throws propagate non-locally via TLS — the
/// SSA arm calls `emit_throw_check(None)` after every toFixed Call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_num_to_fixed_f(n: f64, digits: i64) -> *mut u8 {
    if !(0..=100).contains(&digits) {
        unsafe {
            __torajs_throw_range_error(b"toFixed() argument must be between 0 and 100\0".as_ptr());
        }
        return alloc_str(b"");
    }
    alloc_str(&to_fixed_f(n, digits))
}

/// `n.toFixed(digits)` for i64 receivers. Same RangeError gate as
/// the f64 variant.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_num_to_fixed_i(n: i64, digits: i64) -> *mut u8 {
    if !(0..=100).contains(&digits) {
        unsafe {
            __torajs_throw_range_error(b"toFixed() argument must be between 0 and 100\0".as_ptr());
        }
        return alloc_str(b"");
    }
    alloc_str(&to_fixed_i(n, digits))
}

/// `n.toExponential(digits)` for f64 receivers.
///
/// ES §22.1.3.5 step 3 — `digits` must be in `[0, 100]`; otherwise
/// `RangeError`. The SSA arm passes `i64::MIN` as the no-arg
/// sentinel (unreachable as a user literal), routing to the
/// shortest-form `{:e}` path via the pure-Rust `to_exp_f` core's
/// `digits < 0` branch. Every other negative / oversized value
/// reaches the RangeError gate. SSA-side `emit_throw_check(None)`
/// propagates the pending throw.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_num_to_exp_f(n: f64, digits: i64) -> *mut u8 {
    // ES §21.1.3.3 step 4 — "If x is not finite, return
    // Number::toString(x, 10)". This precedes step 5's range check
    // on `f`, so `NaN.toExponential(Infinity)` returns "NaN" and
    // `(Infinity).toExponential(1000)` returns "Infinity" without
    // hitting the RangeError gate below. Only the finite-x path
    // enters the range check.
    if let Some(s) = special_value(n) {
        return alloc_str(&s);
    }
    if digits != i64::MIN && !(0..=100).contains(&digits) {
        unsafe {
            __torajs_throw_range_error(
                b"toExponential() argument must be between 0 and 100\0".as_ptr(),
            );
        }
        return alloc_str(b"");
    }
    alloc_str(&to_exp_f(n, digits))
}

/// `n.toExponential(digits)` for i64 receivers. Same RangeError
/// gate as the f64 variant.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_num_to_exp_i(n: i64, digits: i64) -> *mut u8 {
    if digits != i64::MIN && !(0..=100).contains(&digits) {
        unsafe {
            __torajs_throw_range_error(
                b"toExponential() argument must be between 0 and 100\0".as_ptr(),
            );
        }
        return alloc_str(b"");
    }
    alloc_str(&to_exp_i(n, digits))
}

/// `n.toPrecision(digits)` for f64 receivers.
///
/// ES §22.1.3.32 step 3 — `precision` must be in `[1, 100]`;
/// otherwise `RangeError`. SSA-side `emit_throw_check(None)`
/// propagates the pending throw. The no-arg `toPrecision()` form
/// short-circuits in `ssa_lower.rs` to plain `Number.toString`
/// before reaching this entry point, so the gate doesn't need a
/// sentinel carve-out.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_num_to_precision_f(n: f64, digits: i64) -> *mut u8 {
    // ES §21.1.3.5 step 4 — "If x is not finite, return
    // Number::toString(x, 10)". Mirrors the `toExponential` fix
    // above: NaN / ±Infinity short-circuit before the step-5
    // precision-range gate, so `NaN.toPrecision(1000)` returns "NaN"
    // and `(Infinity).toPrecision(1000)` returns "Infinity".
    if let Some(s) = special_value(n) {
        return alloc_str(&s);
    }
    if !(1..=100).contains(&digits) {
        unsafe {
            __torajs_throw_range_error(
                b"toPrecision() argument must be between 1 and 100\0".as_ptr(),
            );
        }
        return alloc_str(b"");
    }
    alloc_str(&to_precision_f(n, digits))
}

/// `n.toPrecision(digits)` for i64 receivers. Same RangeError gate
/// as the f64 variant.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_num_to_precision_i(n: i64, digits: i64) -> *mut u8 {
    if !(1..=100).contains(&digits) {
        unsafe {
            __torajs_throw_range_error(
                b"toPrecision() argument must be between 1 and 100\0".as_ptr(),
            );
        }
        return alloc_str(b"");
    }
    alloc_str(&to_precision_i(n, digits))
}

/// `n.toLocaleString()` en-US default for f64 receivers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_num_to_locale_f(n: f64) -> *mut u8 {
    alloc_str(&to_locale_f(n))
}

/// `n.toLocaleString()` en-US default for i64 receivers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_num_to_locale_i(n: i64) -> *mut u8 {
    alloc_str(&to_locale_i(n))
}
