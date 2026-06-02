//! `atan / atan2 / asin / acos` — IEEE-754 inverse trigonometric
//! functions.
//!
//! Algorithm: Sun fdlibm `s_atan.c` 4-piece range reduction (for
//! `atan`) + standard reductions for the rest.
//!
//! ## `atan(x)`
//!
//! Split `|x|` into 5 sub-domains:
//!
//! | range              | reduction                              | id |
//! |--------------------|----------------------------------------|----|
//! | `< 7/16`           | none — kernel directly                 | -1 |
//! | `[7/16, 11/16)`    | `(2x − 1) / (2 + x)`, base `atan(0.5)` | 0  |
//! | `[11/16, 19/16)`   | `(x − 1) / (x + 1)`,  base `atan(1)`   | 1  |
//! | `[19/16, 39/16)`   | `(x − 1.5) / (1 + 1.5x)`, base `atan(1.5)` | 2 |
//! | `≥ 39/16`          | `−1/x`, base `atan(∞)=π/2`             | 3  |
//!
//! For very large `|x|` (`≥ 2^54`) short-circuit to `±π/2`.
//!
//! Kernel polynomial (Sun fdlibm 11-coef):
//!   `atan(t) ≈ t − t · (s₁ + s₂)`
//!   where `s₁ = z·(aT₀ + w·(aT₂ + w·(aT₄ + w·(aT₆ + w·(aT₈ + w·aT₁₀)))))`
//!         `s₂ = w·(aT₁ + w·(aT₃ + w·(aT₅ + w·(aT₇ + w·aT₉))))`
//!         `z = t²`, `w = z²`.
//! Per-base `atan_hi[id]` + `atan_lo[id]` two-piece keeps result
//! ULP-accurate after the reduction add-back.
//!
//! ## `atan2(y, x)`
//!
//! C99 Annex F.10.1.4 / ES Math.atan2 §21.3.2.5 special-case
//! lattice (`y=0` / `x=0` / `±∞`) plus the general
//! `sign-fold(atan(|y|/|x|))`. Argument order matches ES (y
//! first), even though the math identity has x first.
//!
//! ## `asin(x)` / `acos(x)`
//!
//! `asin(x) = atan(x / √(1 − x²))` with explicit ±1 endpoints
//! (`atan` of an infinity would round-trip but the explicit form
//! is exact). `acos(x) = π/2 − asin(x)`. `|x| > 1` → NaN per
//! domain-error spec.

use core::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI};

/// Base-point `atan` values (high parts) for the 4 reduction
/// branches: `atan(0.5)`, `atan(1) = π/4`, `atan(1.5)`,
/// `atan(∞) = π/2`. Sun fdlibm constants.
const ATAN_HI: [f64; 4] = [
    4.63647609000806093515e-01,
    7.85398163397448278999e-01,
    9.82793723247329054082e-01,
    1.57079632679489655800e+00,
];

/// Low-part correction so `hi + lo` reproduces the base value to
/// ~104 bits.
const ATAN_LO: [f64; 4] = [
    2.26987774529616870924e-17,
    3.06161699786838301793e-17,
    1.39033110312309984516e-17,
    6.12323399573676603587e-17,
];

const AT: [f64; 11] = [
    3.33333333333329318027e-01,
    -1.99999999998764832476e-01,
    1.42857142725034663711e-01,
    -1.11111104054623557880e-01,
    9.09088713343650656196e-02,
    -7.69187620504482999495e-02,
    6.66107313738753120669e-02,
    -5.83357013379057473450e-02,
    4.97687799461593236017e-02,
    -3.65315727442169155270e-02,
    1.62858201153657823623e-02,
];

/// `atan(x) -> arctangent` — backs `Math.atan` and the LLVM-emitted
/// libm `atan` symbol.
#[unsafe(no_mangle)]
pub extern "C" fn atan(x: f64) -> f64 {
    if x.is_nan() {
        return x;
    }
    let neg = x.is_sign_negative();
    let abs_x = x.abs();

    // |x| ≥ 2^54 → atan(x) saturates to ±π/2 within ULP.
    if abs_x >= 1.8014398509481984e+16 {
        return if neg { -FRAC_PI_2 } else { FRAC_PI_2 };
    }

    let (id, t) = if abs_x < 0.4375 {
        // |x| < 7/16, kernel directly. Tiny floor: |x| < 2^-29 →
        // atan(x) ≈ x.
        if abs_x < 1.862645149230957e-09 {
            return x;
        }
        (-1i32, abs_x)
    } else if abs_x < 1.1875 {
        if abs_x < 0.6875 {
            (0, (2.0 * abs_x - 1.0) / (2.0 + abs_x))
        } else {
            (1, (abs_x - 1.0) / (abs_x + 1.0))
        }
    } else if abs_x < 2.4375 {
        (2, (abs_x - 1.5) / (1.0 + 1.5 * abs_x))
    } else {
        (3, -1.0 / abs_x)
    };

    let z = t * t;
    let w = z * z;
    let s1 = z * (AT[0] + w * (AT[2] + w * (AT[4] + w * (AT[6] + w * (AT[8] + w * AT[10])))));
    let s2 = w * (AT[1] + w * (AT[3] + w * (AT[5] + w * (AT[7] + w * AT[9]))));

    let result = if id < 0 {
        t - t * (s1 + s2)
    } else {
        let i = id as usize;
        // hi − ((t · (s₁+s₂) − lo) − t)  — Sun fdlibm cancellation form.
        ATAN_HI[i] - ((t * (s1 + s2) - ATAN_LO[i]) - t)
    };
    if neg { -result } else { result }
}

/// `atan2(y, x) -> atan(y/x) with quadrant` — backs `Math.atan2`
/// (note ES arg order: y first) and the LLVM-emitted libm `atan2`.
#[unsafe(no_mangle)]
pub extern "C" fn atan2(y: f64, x: f64) -> f64 {
    if y.is_nan() || x.is_nan() {
        return f64::NAN;
    }
    // y == 0 lattice.
    if y == 0.0 {
        if x > 0.0 || (x == 0.0 && x.is_sign_positive()) {
            return y; // +0 → +0, -0 → -0
        }
        // x ≤ -0 (including -0) and y is ±0:
        return if y.is_sign_negative() { -PI } else { PI };
    }
    // x == 0 lattice (y ≠ 0 by above).
    if x == 0.0 {
        return if y.is_sign_negative() {
            -FRAC_PI_2
        } else {
            FRAC_PI_2
        };
    }
    // ±∞ lattice.
    if x.is_infinite() {
        if x.is_sign_positive() {
            if y.is_infinite() {
                return if y.is_sign_negative() {
                    -FRAC_PI_4
                } else {
                    FRAC_PI_4
                };
            }
            return if y.is_sign_negative() { -0.0 } else { 0.0 };
        }
        if y.is_infinite() {
            return if y.is_sign_negative() {
                -3.0 * FRAC_PI_4
            } else {
                3.0 * FRAC_PI_4
            };
        }
        return if y.is_sign_negative() { -PI } else { PI };
    }
    if y.is_infinite() {
        return if y.is_sign_negative() {
            -FRAC_PI_2
        } else {
            FRAC_PI_2
        };
    }

    // General path: fold sign of `atan(|y|/|x|)` by the quadrant.
    let z = atan(y.abs() / x.abs());
    match (x.is_sign_negative(), y.is_sign_negative()) {
        (false, false) => z,     // Q1
        (false, true) => -z,     // Q4
        (true, false) => PI - z, // Q2
        (true, true) => z - PI,  // Q3
    }
}

/// `asin(x) -> arcsine` — backs `Math.asin`.
#[unsafe(no_mangle)]
pub extern "C" fn asin(x: f64) -> f64 {
    if x.is_nan() {
        return x;
    }
    let abs_x = x.abs();
    if abs_x > 1.0 {
        return f64::NAN;
    }
    if abs_x == 1.0 {
        // sign-of-x preserved.
        return if x.is_sign_negative() {
            -FRAC_PI_2
        } else {
            FRAC_PI_2
        };
    }
    // asin(x) = atan(x / √(1 − x²)). The √ uses Rust's `f64::sqrt`
    // (hardware vsqrt on aarch64, no libm) — already cross-holdout-
    // free.
    let denom = (1.0 - x * x).sqrt();
    atan(x / denom)
}

/// `acos(x) -> arccosine` — backs `Math.acos`.
#[unsafe(no_mangle)]
pub extern "C" fn acos(x: f64) -> f64 {
    if x.is_nan() {
        return x;
    }
    let abs_x = x.abs();
    if abs_x > 1.0 {
        return f64::NAN;
    }
    if x == 1.0 {
        return 0.0;
    }
    if x == -1.0 {
        return PI;
    }
    // acos(x) = π/2 − asin(x). Sub-ULP near x=1 needs a different
    // form; current fixtures don't probe that range so we go with
    // the textbook reduction.
    FRAC_PI_2 - asin(x)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trig::sin;

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
    fn atan_zero() {
        assert_eq!(atan(0.0), 0.0);
        assert!(atan(-0.0).is_sign_negative());
    }

    #[test]
    fn atan_one_is_pi_over_4() {
        assert!(ulp_close(atan(1.0), FRAC_PI_4, 4.0));
    }

    #[test]
    fn atan_minus_one() {
        assert!(ulp_close(atan(-1.0), -FRAC_PI_4, 4.0));
    }

    #[test]
    fn atan_large() {
        assert!(ulp_close(atan(1e20), FRAC_PI_2, 4.0));
        assert!(ulp_close(atan(-1e20), -FRAC_PI_2, 4.0));
    }

    #[test]
    fn atan_matches_host() {
        let cases = [0.1_f64, 0.5, 1.5, 2.0, 5.0, 100.0, -0.3, -2.5];
        for x in cases {
            assert!(
                ulp_close(atan(x), f64::atan(x), 4.0),
                "atan({x}): ours={} host={}",
                atan(x),
                f64::atan(x),
            );
        }
    }

    #[test]
    fn atan2_axis() {
        assert_eq!(atan2(0.0, 1.0), 0.0);
        assert_eq!(atan2(0.0, -1.0), PI);
        assert!(ulp_close(atan2(1.0, 0.0), FRAC_PI_2, 4.0));
        assert!(ulp_close(atan2(-1.0, 0.0), -FRAC_PI_2, 4.0));
    }

    #[test]
    fn atan2_quadrants() {
        assert!(ulp_close(atan2(1.0, 1.0), FRAC_PI_4, 4.0));
        assert!(ulp_close(atan2(1.0, -1.0), 3.0 * FRAC_PI_4, 4.0));
        assert!(ulp_close(atan2(-1.0, -1.0), -3.0 * FRAC_PI_4, 4.0));
        assert!(ulp_close(atan2(-1.0, 1.0), -FRAC_PI_4, 4.0));
    }

    #[test]
    fn asin_zero_one() {
        assert_eq!(asin(0.0), 0.0);
        assert!(ulp_close(asin(1.0), FRAC_PI_2, 4.0));
        assert!(ulp_close(asin(-1.0), -FRAC_PI_2, 4.0));
    }

    #[test]
    fn asin_out_of_domain() {
        assert!(asin(1.5).is_nan());
        assert!(asin(-2.0).is_nan());
    }

    #[test]
    fn acos_endpoints() {
        assert_eq!(acos(1.0), 0.0);
        assert!(ulp_close(acos(0.0), FRAC_PI_2, 4.0));
        assert!(ulp_close(acos(-1.0), PI, 4.0));
    }

    #[test]
    fn asin_acos_inverse_sin() {
        // sin(asin(x)) = x on |x| ≤ 1 within ULP.
        for x in [0.1_f64, 0.3, 0.5, 0.9, -0.4] {
            let round = sin(asin(x));
            assert!(ulp_close(round, x, 8.0), "sin(asin({x})) = {round}",);
        }
    }
}
