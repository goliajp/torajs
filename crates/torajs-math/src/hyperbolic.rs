//! `sinh / cosh / tanh / asinh / acosh / atanh` — IEEE-754
//! hyperbolic + inverse-hyperbolic functions.
//!
//! All six are textbook reductions onto [`super::exp`] /
//! [`super::log`]:
//!
//! - `sinh(x) = (eˣ − e⁻ˣ) / 2`
//! - `cosh(x) = (eˣ + e⁻ˣ) / 2`
//! - `tanh(x) = 1 − 2/(e²ˣ + 1)`        (numerically nicer than
//!                                       the bare sinh/cosh ratio
//!                                       — avoids two transcendental
//!                                       evals + a near-1 divide)
//! - `asinh(x) = ln(x + √(x² + 1))`
//! - `acosh(x) = ln(x + √(x² − 1))`      (domain x ≥ 1)
//! - `atanh(x) = ½ · ln((1 + x) / (1 − x))`   (domain |x| < 1)
//!
//! Overflow caps:
//! - `|x| ≥ 710`: `sinh / cosh` saturate to `±∞ / +∞` (eˣ
//!   itself overflows around there).
//! - `|x| ≥ 22`: `tanh` saturates to `±1` (the `e²ˣ + 1` divide is
//!   already ≤ 1 ULP from the limit).
//!
//! Tiny-x linearization (`|x| < 2⁻²⁶`) avoids cancellation in the
//! `eˣ − e⁻ˣ` subtraction:
//! - `sinh(x) ≈ x`, `cosh(x) = 1`, `tanh(x) ≈ x`,
//! - `asinh(x) ≈ x`, `atanh(x) ≈ x`.
//!
//! Special cases (ES Math.{sinh,cosh,tanh,asinh,acosh,atanh}
//! §21.3.2.31-37):
//! - `NaN` → `NaN` for all six.
//! - `±∞`: sinh / tanh / asinh preserve sign; cosh / acosh → `+∞`;
//!   `atanh(±1) = ±∞`; `atanh(|x| > 1) = NaN`.
//! - `±0`: sinh / tanh / asinh / atanh preserve sign; cosh / acosh
//!   trivially `1 / 0`.

use crate::exp::exp;
use crate::log::log;

/// Tiny-x threshold below which the polynomial cancellation in
/// `eˣ − e⁻ˣ` exceeds the linear approximation error.
const TINY: f64 = 1.4901161193847656e-08; // 2^-26

/// Overflow guard for `sinh / cosh` — `e^710` already overflows the
/// f64 range, so anything past this saturates.
const OVERFLOW_X: f64 = 710.0;

/// `sinh(x)` — backs `Math.sinh`.
#[unsafe(no_mangle)]
pub extern "C" fn sinh(x: f64) -> f64 {
    if !x.is_finite() {
        return x; // NaN, ±∞ pass through
    }
    let neg = x.is_sign_negative();
    let abs_x = x.abs();
    if abs_x < TINY {
        return x; // sign preserved
    }
    if abs_x >= OVERFLOW_X {
        return if neg {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        };
    }
    let e = exp(abs_x);
    let s = (e - 1.0 / e) * 0.5;
    if neg { -s } else { s }
}

/// `cosh(x)` — backs `Math.cosh`.
#[unsafe(no_mangle)]
pub extern "C" fn cosh(x: f64) -> f64 {
    if x.is_nan() {
        return x;
    }
    let abs_x = x.abs();
    if abs_x >= OVERFLOW_X {
        return f64::INFINITY;
    }
    if abs_x < TINY {
        return 1.0;
    }
    let e = exp(abs_x);
    (e + 1.0 / e) * 0.5
}

/// `tanh(x)` — backs `Math.tanh`. Uses the
/// `1 − 2/(e²ˣ + 1)` form for numerical stability vs. `sinh/cosh`.
#[unsafe(no_mangle)]
pub extern "C" fn tanh(x: f64) -> f64 {
    if x.is_nan() {
        return x;
    }
    if x == f64::INFINITY {
        return 1.0;
    }
    if x == f64::NEG_INFINITY {
        return -1.0;
    }
    let neg = x.is_sign_negative();
    let abs_x = x.abs();
    if abs_x < TINY {
        return x; // sign preserved
    }
    // |x| ≥ 22 → tanh saturates to ±1 within ULP.
    if abs_x >= 22.0 {
        return if neg { -1.0 } else { 1.0 };
    }
    let e2 = exp(2.0 * abs_x);
    let t = 1.0 - 2.0 / (e2 + 1.0);
    if neg { -t } else { t }
}

/// `asinh(x) = ln(x + √(x² + 1))` — backs `Math.asinh`.
#[unsafe(no_mangle)]
pub extern "C" fn asinh(x: f64) -> f64 {
    if !x.is_finite() {
        return x; // NaN / ±∞ propagate
    }
    let abs_x = x.abs();
    if abs_x < TINY {
        return x; // sign preserved
    }
    let neg = x.is_sign_negative();
    // For |x| ≫ 1 the `x + √(x² + 1) → 2x` simplification keeps
    // precision; the bare form would lose bits when `x²` rounds.
    let r = if abs_x > 1e9 {
        log(2.0 * abs_x)
    } else {
        log(abs_x + (abs_x * abs_x + 1.0).sqrt())
    };
    if neg { -r } else { r }
}

/// `acosh(x) = ln(x + √(x² − 1))` — backs `Math.acosh`. Domain
/// `x ≥ 1`; `acosh(x < 1) = NaN`.
#[unsafe(no_mangle)]
pub extern "C" fn acosh(x: f64) -> f64 {
    if x.is_nan() {
        return x;
    }
    if x < 1.0 {
        return f64::NAN;
    }
    if x == 1.0 {
        return 0.0;
    }
    if x == f64::INFINITY {
        return f64::INFINITY;
    }
    if x > 1e9 {
        log(2.0 * x)
    } else {
        log(x + (x * x - 1.0).sqrt())
    }
}

/// `atanh(x) = ½ · ln((1 + x) / (1 − x))` — backs `Math.atanh`.
/// Domain `|x| < 1`; `atanh(±1) = ±∞`; `|x| > 1` → NaN.
#[unsafe(no_mangle)]
pub extern "C" fn atanh(x: f64) -> f64 {
    if x.is_nan() {
        return x;
    }
    let abs_x = x.abs();
    if abs_x > 1.0 {
        return f64::NAN;
    }
    if abs_x == 1.0 {
        return if x.is_sign_negative() {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        };
    }
    if abs_x < TINY {
        return x;
    }
    0.5 * log((1.0 + x) / (1.0 - x))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ulp_close(a: f64, b: f64, ulps: f64) -> bool {
        if a == b {
            return true;
        }
        if a.is_nan() && b.is_nan() {
            return true;
        }
        let scale = b.abs().max(1.0);
        (a - b).abs() <= scale * ulps * f64::EPSILON
    }

    #[test]
    fn at_zero() {
        assert_eq!(sinh(0.0), 0.0);
        assert_eq!(cosh(0.0), 1.0);
        assert_eq!(tanh(0.0), 0.0);
        assert_eq!(asinh(0.0), 0.0);
        assert_eq!(acosh(1.0), 0.0);
        assert_eq!(atanh(0.0), 0.0);
        // sign preservation on ±0
        assert!(sinh(-0.0).is_sign_negative());
        assert!(tanh(-0.0).is_sign_negative());
        assert!(asinh(-0.0).is_sign_negative());
        assert!(atanh(-0.0).is_sign_negative());
    }

    #[test]
    fn nan_and_inf() {
        assert!(sinh(f64::NAN).is_nan());
        assert!(cosh(f64::NAN).is_nan());
        assert!(tanh(f64::NAN).is_nan());
        assert!(asinh(f64::NAN).is_nan());
        assert!(acosh(f64::NAN).is_nan());
        assert!(atanh(f64::NAN).is_nan());

        assert_eq!(sinh(f64::INFINITY), f64::INFINITY);
        assert_eq!(cosh(f64::INFINITY), f64::INFINITY);
        assert_eq!(tanh(f64::INFINITY), 1.0);
        assert_eq!(tanh(f64::NEG_INFINITY), -1.0);
        assert_eq!(asinh(f64::INFINITY), f64::INFINITY);
        assert_eq!(acosh(f64::INFINITY), f64::INFINITY);

        // atanh(±1) = ±∞
        assert_eq!(atanh(1.0), f64::INFINITY);
        assert_eq!(atanh(-1.0), f64::NEG_INFINITY);
        // out of domain
        assert!(atanh(1.5).is_nan());
        assert!(acosh(0.5).is_nan());
    }

    #[test]
    fn overflow() {
        // `sinh(710) ≈ e^710/2 ≈ 1.12e308` still fits f64; overflow
        // really kicks in around 710.48 (our cap is at 710.0, which
        // is conservative — close to but just inside the boundary).
        assert_eq!(sinh(750.0), f64::INFINITY);
        assert_eq!(sinh(-750.0), f64::NEG_INFINITY);
        assert_eq!(cosh(750.0), f64::INFINITY);
        assert_eq!(tanh(50.0), 1.0); // saturates
    }

    #[test]
    fn matches_host_libm() {
        let cases = [0.5_f64, 1.0, 1.5, 2.0, 5.0, -1.0, 10.0];
        for x in cases {
            assert!(
                ulp_close(sinh(x), f64::sinh(x), 8.0),
                "sinh({x}): ours={} host={}",
                sinh(x),
                f64::sinh(x),
            );
            assert!(
                ulp_close(cosh(x), f64::cosh(x), 8.0),
                "cosh({x}): ours={} host={}",
                cosh(x),
                f64::cosh(x),
            );
            assert!(
                ulp_close(tanh(x), f64::tanh(x), 8.0),
                "tanh({x}): ours={} host={}",
                tanh(x),
                f64::tanh(x),
            );
        }
    }

    #[test]
    fn inverse_round_trip() {
        // sinh ∘ asinh ≈ identity within ULP on small x.
        for x in [0.1_f64, 0.5, 1.0, 2.0, -1.0] {
            let r = sinh(asinh(x));
            assert!(ulp_close(r, x, 16.0), "sinh(asinh({x})) = {r}");
        }
        // cosh ∘ acosh ≈ identity for x ≥ 1.
        for x in [1.5_f64, 2.0, 5.0] {
            let r = cosh(acosh(x));
            assert!(ulp_close(r, x, 16.0), "cosh(acosh({x})) = {r}");
        }
    }
}
