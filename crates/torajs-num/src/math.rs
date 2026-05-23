//! Math namespace intrinsics — `Math.sqrt(x)` / `Math.abs(x)` /
//! `Math.pow(x, y)` / etc.
//!
//! Each extern fn is a thin wrapper over Rust's `f64::X(self)` /
//! `f64::X(self, other)` methods. Rust stdlib delegates to libm at
//! the same call site the IR-emitted versions used (the
//! `define_math_unary` / `define_math_binary` builders in
//! `ssa_inkwell` emitted single libm calls).
//!
//! ## P3.2-a (this commit)
//!
//! Only `__torajs_math_sqrt` ported as a single-fn pipeline verify.
//! P3.2-b ports the remaining ~22 (abs / floor / ceil / log / exp /
//! round / trunc / sin / cos / tan / asin / acos / atan / sinh /
//! cosh / tanh / asinh / acosh / atanh / cbrt / expm1 / log10 /
//! log1p / log2 + binary pow / min / max / atan2). Each is a single
//! `extern "C" fn` line.

/// `Math.sqrt(x)` — square root via Rust stdlib (libm `sqrt`).
/// Returns NaN for `x < 0.0` (matches both ES §21.3.2.32 and the
/// IR-emitted version's libm dispatch).
///
/// Port of `ssa_inkwell::define_math_unary("__torajs_math_sqrt",
/// "sqrt")` (P3.2-a, 2026-05-23).
///
/// # Safety
///
/// Pure fn; the `unsafe` is required only for the FFI ABI shape.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_sqrt(x: f64) -> f64 {
    x.sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqrt_basic() {
        assert_eq!(unsafe { __torajs_math_sqrt(0.0) }, 0.0);
        assert_eq!(unsafe { __torajs_math_sqrt(1.0) }, 1.0);
        assert_eq!(unsafe { __torajs_math_sqrt(4.0) }, 2.0);
        assert_eq!(unsafe { __torajs_math_sqrt(9.0) }, 3.0);
    }

    #[test]
    fn sqrt_fractional() {
        let r = unsafe { __torajs_math_sqrt(2.0) };
        assert!((r - 1.4142135623730951).abs() < 1e-15);
    }

    #[test]
    fn sqrt_negative_yields_nan() {
        assert!(unsafe { __torajs_math_sqrt(-1.0) }.is_nan());
    }

    #[test]
    fn sqrt_infinity_is_infinity() {
        assert!(unsafe { __torajs_math_sqrt(f64::INFINITY) }.is_infinite());
    }
}
