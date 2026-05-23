//! Math namespace intrinsics — `Math.sqrt(x)` / `Math.abs(x)` /
//! `Math.pow(x, y)` / etc.
//!
//! Each extern fn is a thin wrapper over Rust's `f64::X(self)` /
//! `f64::X(self, other)` methods. Rust stdlib delegates to libm at
//! the same call site the IR-emitted versions used (the
//! `define_math_unary` / `define_math_binary` builders in
//! `ssa_inkwell` emitted single libm calls; both helpers + their
//! 27 dispatch arms deleted at P3.2-b ship 2026-05-23).
//!
//! ## libm-equivalent symbol map
//!
//! | torajs fn                | Rust f64 method | libm symbol |
//! |---|---|---|
//! | `__torajs_math_sqrt`     | `x.sqrt()`      | `sqrt`      |
//! | `__torajs_math_abs`      | `x.abs()`       | `fabs`      |
//! | `__torajs_math_floor`    | `x.floor()`     | `floor`     |
//! | `__torajs_math_ceil`     | `x.ceil()`      | `ceil`      |
//! | `__torajs_math_round`    | `x.round()`     | `round`     |
//! | `__torajs_math_trunc`    | `x.trunc()`     | `trunc`     |
//! | `__torajs_math_cbrt`     | `x.cbrt()`      | `cbrt`      |
//! | `__torajs_math_exp`      | `x.exp()`       | `exp`       |
//! | `__torajs_math_expm1`    | `x.exp_m1()`    | `expm1`     |
//! | `__torajs_math_log`      | `x.ln()`        | `log`       |
//! | `__torajs_math_log2`     | `x.log2()`      | `log2`      |
//! | `__torajs_math_log10`    | `x.log10()`     | `log10`     |
//! | `__torajs_math_log1p`    | `x.ln_1p()`     | `log1p`     |
//! | `__torajs_math_sin`      | `x.sin()`       | `sin`       |
//! | `__torajs_math_cos`      | `x.cos()`       | `cos`       |
//! | `__torajs_math_tan`      | `x.tan()`       | `tan`       |
//! | `__torajs_math_asin`     | `x.asin()`      | `asin`      |
//! | `__torajs_math_acos`     | `x.acos()`      | `acos`      |
//! | `__torajs_math_atan`     | `x.atan()`      | `atan`      |
//! | `__torajs_math_sinh`     | `x.sinh()`      | `sinh`      |
//! | `__torajs_math_cosh`     | `x.cosh()`      | `cosh`      |
//! | `__torajs_math_tanh`     | `x.tanh()`      | `tanh`      |
//! | `__torajs_math_asinh`    | `x.asinh()`     | `asinh`     |
//! | `__torajs_math_acosh`    | `x.acosh()`     | `acosh`     |
//! | `__torajs_math_atanh`    | `x.atanh()`     | `atanh`     |
//! | `__torajs_math_pow`      | `x.powf(y)`     | `pow`       |
//! | `__torajs_math_min`      | `x.min(y)`      | `fmin`      |
//! | `__torajs_math_max`      | `x.max(y)`      | `fmax`      |
//! | `__torajs_math_atan2`    | `y.atan2(x)`    | `atan2`     |
//!
//! `min` / `max` use C `fmin` / `fmax` semantics (NaN treated as
//! sentinel — non-NaN arg returned). JS spec actually says
//! `Math.min(NaN, 5) === NaN`; the pre-port IR-emitted version
//! used libm and conformance was green — we preserve the libm
//! semantics bit-for-bit. Spec-correctness wedge belongs in a
//! later task (after the rewrite stabilizes).

// ============================================================
// Unary — f64 → f64
// ============================================================

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_sqrt(x: f64) -> f64 {
    x.sqrt()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_abs(x: f64) -> f64 {
    x.abs()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_floor(x: f64) -> f64 {
    x.floor()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_ceil(x: f64) -> f64 {
    x.ceil()
}
/// `Math.round(x)` — **JS spec semantics** (round half toward +∞),
/// NOT libc `round` (round half away from zero).
///
/// `Math.round(2.5)` === `3` (both agree); `Math.round(-2.5)` === `-2`
/// (spec) vs `-3` (libc). The pre-port C impl in `runtime_str.c` was
/// specifically `floor(x + 0.5)` to preserve the spec behavior; we
/// replicate that bit-for-bit here so the port doesn't silently
/// change negative-half rounding.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_round(x: f64) -> f64 {
    (x + 0.5).floor()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_trunc(x: f64) -> f64 {
    x.trunc()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_cbrt(x: f64) -> f64 {
    x.cbrt()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_exp(x: f64) -> f64 {
    x.exp()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_expm1(x: f64) -> f64 {
    x.exp_m1()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_log(x: f64) -> f64 {
    x.ln()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_log2(x: f64) -> f64 {
    x.log2()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_log10(x: f64) -> f64 {
    x.log10()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_log1p(x: f64) -> f64 {
    x.ln_1p()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_sin(x: f64) -> f64 {
    x.sin()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_cos(x: f64) -> f64 {
    x.cos()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_tan(x: f64) -> f64 {
    x.tan()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_asin(x: f64) -> f64 {
    x.asin()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_acos(x: f64) -> f64 {
    x.acos()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_atan(x: f64) -> f64 {
    x.atan()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_sinh(x: f64) -> f64 {
    x.sinh()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_cosh(x: f64) -> f64 {
    x.cosh()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_tanh(x: f64) -> f64 {
    x.tanh()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_asinh(x: f64) -> f64 {
    x.asinh()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_acosh(x: f64) -> f64 {
    x.acosh()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_atanh(x: f64) -> f64 {
    x.atanh()
}

// ============================================================
// Binary — f64 × f64 → f64
// ============================================================

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_pow(x: f64, y: f64) -> f64 {
    x.powf(y)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_min(x: f64, y: f64) -> f64 {
    x.min(y)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_max(x: f64, y: f64) -> f64 {
    x.max(y)
}

/// **Arg order**: ES `Math.atan2(y, x)` — i.e. `y` is first.
/// Matches `f64::atan2(self=y, other=x)` Rust convention.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_atan2(y: f64, x: f64) -> f64 {
    y.atan2(x)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unary_basic() {
        assert_eq!(unsafe { __torajs_math_sqrt(4.0) }, 2.0);
        assert_eq!(unsafe { __torajs_math_abs(-7.5) }, 7.5);
        assert_eq!(unsafe { __torajs_math_floor(3.7) }, 3.0);
        assert_eq!(unsafe { __torajs_math_ceil(3.2) }, 4.0);
        assert_eq!(unsafe { __torajs_math_round(2.5) }, 3.0);
        assert_eq!(unsafe { __torajs_math_trunc(3.7) }, 3.0);
        assert_eq!(unsafe { __torajs_math_cbrt(27.0) }, 3.0);
        assert_eq!(unsafe { __torajs_math_exp(0.0) }, 1.0);
        assert_eq!(unsafe { __torajs_math_log(1.0) }, 0.0);
        assert_eq!(unsafe { __torajs_math_log2(8.0) }, 3.0);
        assert_eq!(unsafe { __torajs_math_log10(1000.0) }, 3.0);
    }

    #[test]
    fn trig_basic() {
        assert_eq!(unsafe { __torajs_math_sin(0.0) }, 0.0);
        assert_eq!(unsafe { __torajs_math_cos(0.0) }, 1.0);
        assert_eq!(unsafe { __torajs_math_tan(0.0) }, 0.0);
        assert_eq!(unsafe { __torajs_math_asin(0.0) }, 0.0);
        assert!((unsafe { __torajs_math_acos(0.0) } - core::f64::consts::FRAC_PI_2).abs() < 1e-15);
        assert_eq!(unsafe { __torajs_math_atan(0.0) }, 0.0);
        assert_eq!(unsafe { __torajs_math_sinh(0.0) }, 0.0);
        assert_eq!(unsafe { __torajs_math_cosh(0.0) }, 1.0);
        assert_eq!(unsafe { __torajs_math_tanh(0.0) }, 0.0);
        assert_eq!(unsafe { __torajs_math_asinh(0.0) }, 0.0);
        assert_eq!(unsafe { __torajs_math_acosh(1.0) }, 0.0);
        assert_eq!(unsafe { __torajs_math_atanh(0.0) }, 0.0);
    }

    #[test]
    fn binary_basic() {
        assert_eq!(unsafe { __torajs_math_pow(2.0, 10.0) }, 1024.0);
        assert_eq!(unsafe { __torajs_math_min(3.0, 5.0) }, 3.0);
        assert_eq!(unsafe { __torajs_math_max(3.0, 5.0) }, 5.0);
        assert_eq!(unsafe { __torajs_math_atan2(0.0, 1.0) }, 0.0);
    }

    #[test]
    fn nan_paths_via_libm() {
        // sqrt of negative → NaN
        assert!(unsafe { __torajs_math_sqrt(-1.0) }.is_nan());
        // acos of out-of-range → NaN
        assert!(unsafe { __torajs_math_acos(2.0) }.is_nan());
        // log of negative → NaN
        assert!(unsafe { __torajs_math_log(-1.0) }.is_nan());
        // expm1 of 0 → 0
        assert_eq!(unsafe { __torajs_math_expm1(0.0) }, 0.0);
    }
}
