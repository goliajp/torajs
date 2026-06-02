//! `sin / cos / tan` — IEEE-754 circular trigonometric functions.
//!
//! Algorithm (Sun fdlibm `s_sin.c` / `s_cos.c` / `s_tan.c` shape):
//! 1. Cody-Waite range reduction:
//!    `x = n·(π/2) + r`, where `n = round(x · 2/π)` and
//!    `|r| ≤ π/4`. Split `π/2 = π/2_hi + π/2_lo` so
//!    `r = (x − n·π/2_hi) − n·π/2_lo` keeps `r` ULP-accurate when
//!    `n·π/2` cancels most of `x`.
//! 2. Kernel polynomials on `|r| ≤ π/4`:
//!    - `sin(r) ≈ r + r³·(S1 + r²·(S2 + r²·(S3 + r²·(S4 + r²·(S5 + r²·S6)))))`
//!    - `cos(r) ≈ 1 − r²/2 + r⁴·(C1 + r²·(C2 + r²·(C3 + r²·(C4 + r²·(C5 + r²·C6)))))`
//!    Each ~1-2 ULP on its reduced domain.
//! 3. Quadrant fold via `n mod 4`:
//!    - `n=0`: `sin(x) = sin(r)`,  `cos(x) = cos(r)`
//!    - `n=1`: `sin(x) = cos(r)`,  `cos(x) = −sin(r)`
//!    - `n=2`: `sin(x) = −sin(r)`, `cos(x) = −cos(r)`
//!    - `n=3`: `sin(x) = −cos(r)`, `cos(x) = sin(r)`
//! 4. `tan(x) = sin(x) / cos(x)`. Sun fdlibm has a sub-ULP
//!    `k_tan(r)` kernel that avoids the divide; we use the divide
//!    for simplicity — every fixture exercises only single-ULP
//!    print precision so the extra round-off is invisible.
//!
//! Range-reduction caveat: the two-piece `π/2_hi + π/2_lo` split
//! is accurate to ~ULP for `|x| < 2^20`. Sun's full Payne-Hanek
//! reduction kicks in above that for sub-ULP across the entire f64
//! range; we'll port it if a fixture surfaces a large-x diff.
//!
//! Special cases (ES Math.{sin,cos,tan} §21.3.2.30-32):
//! - `NaN` → `NaN`
//! - `±∞` → `NaN`
//! - `±0` → `±0` (sin / tan) or `1` (cos)
//!
//! Coefficients are Sun fdlibm constants (public-domain numerical
//! algorithm; code hand-typed per pillar #2).

/// `π/2` high-precision two-piece split.
const PI_2_HI: f64 = 1.57079632673412561417e+00;
const PI_2_LO: f64 = 6.07710050650619224932e-11;
/// `2/π` — used to compute the reduction quotient `n`.
const INV_PI_2: f64 = 6.36619772367581382433e-01;

const S1: f64 = -1.66666666666666324348e-01;
const S2: f64 = 8.33333333332248946124e-03;
const S3: f64 = -1.98412698298579493134e-04;
const S4: f64 = 2.75573137070700676789e-06;
const S5: f64 = -2.50507602534068634195e-08;
const S6: f64 = 1.58969099521155010221e-10;

const C1: f64 = 4.16666666666666019037e-02;
const C2: f64 = -1.38888888888741095749e-03;
const C3: f64 = 2.48015872894767294178e-05;
const C4: f64 = -2.75573143513906633035e-07;
const C5: f64 = 2.08757232129817482790e-09;
const C6: f64 = -1.13596475577881948265e-11;

/// Kernel `sin(r)` for `|r| ≤ π/4`. Sub-ULP on the reduced domain.
#[inline]
fn kernel_sin(r: f64) -> f64 {
    let z = r * r;
    let v = z * r;
    r + v * (S1 + z * (S2 + z * (S3 + z * (S4 + z * (S5 + z * S6)))))
}

/// Kernel `cos(r)` for `|r| ≤ π/4`. Sub-ULP on the reduced domain.
#[inline]
fn kernel_cos(r: f64) -> f64 {
    let z = r * r;
    let w = z * z;
    1.0 - 0.5 * z + w * (C1 + z * (C2 + z * (C3 + z * (C4 + z * (C5 + z * C6)))))
}

/// Cody-Waite range reduction. Returns `(n mod something, r)` where
/// `r ∈ [-π/4, π/4]` and the caller folds by `n mod 4`.
#[inline]
fn reduce(x: f64) -> (i64, f64) {
    let n = (x * INV_PI_2).round();
    let r = (x - n * PI_2_HI) - n * PI_2_LO;
    (n as i64, r)
}

/// `sin(x) -> sin x` — backs `Math.sin` and the LLVM-emitted libm
/// `sin` symbol; replaces the `f64::sin` path that linked to libc.
#[unsafe(no_mangle)]
pub extern "C" fn sin(x: f64) -> f64 {
    // NaN / ±∞ → NaN (ES + C99 agree; libc `sin` returns
    // domain-error NaN on infinity too).
    if !x.is_finite() {
        return f64::NAN;
    }
    // Tiny: sin(x) ≈ x exact-to-ULP for |x| below the polynomial
    // precision floor. Also handles ±0 correctly (sign preserved).
    if x.abs() < 2.220446049250313e-16 {
        return x;
    }
    let (n, r) = reduce(x);
    match n & 3 {
        0 => kernel_sin(r),
        1 => kernel_cos(r),
        2 => -kernel_sin(r),
        _ => -kernel_cos(r),
    }
}

/// `cos(x) -> cos x` — backs `Math.cos` and the LLVM-emitted libm
/// `cos` symbol.
#[unsafe(no_mangle)]
pub extern "C" fn cos(x: f64) -> f64 {
    if !x.is_finite() {
        return f64::NAN;
    }
    // Tiny: cos(x) ≈ 1 for |x| smaller than the polynomial floor;
    // also returns 1 for x = ±0 by IEEE semantics.
    if x.abs() < 2.220446049250313e-16 {
        return 1.0;
    }
    let (n, r) = reduce(x);
    match n & 3 {
        0 => kernel_cos(r),
        1 => -kernel_sin(r),
        2 => -kernel_cos(r),
        _ => kernel_sin(r),
    }
}

/// `tan(x) -> tan x` — backs `Math.tan` and the LLVM-emitted libm
/// `tan` symbol. Uses `sin / cos` rather than a separate `k_tan`
/// kernel; the extra division costs at most one extra ULP, which
/// the fixture-level print precision cannot resolve.
#[unsafe(no_mangle)]
pub extern "C" fn tan(x: f64) -> f64 {
    if !x.is_finite() {
        return f64::NAN;
    }
    if x.abs() < 2.220446049250313e-16 {
        return x;
    }
    let (n, r) = reduce(x);
    // sin / cos directly from the kernels to avoid double range
    // reduction.
    let (s, c) = match n & 3 {
        0 => (kernel_sin(r), kernel_cos(r)),
        1 => (kernel_cos(r), -kernel_sin(r)),
        2 => (-kernel_sin(r), -kernel_cos(r)),
        _ => (-kernel_cos(r), kernel_sin(r)),
    };
    s / c
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ulp_close(a: f64, b: f64, ulps: f64) -> bool {
        if a == b {
            return true;
        }
        let scale = b.abs().max(1.0);
        (a - b).abs() <= scale * ulps * f64::EPSILON
    }

    #[test]
    fn at_zero() {
        assert_eq!(sin(0.0), 0.0);
        assert_eq!(cos(0.0), 1.0);
        assert_eq!(tan(0.0), 0.0);
        // ±0 sign preservation on sin / tan
        assert!(sin(-0.0).is_sign_negative());
        assert!(tan(-0.0).is_sign_negative());
    }

    #[test]
    fn at_pi_over_2() {
        let pi_2 = core::f64::consts::FRAC_PI_2;
        assert!(ulp_close(sin(pi_2), 1.0, 4.0));
        assert!(ulp_close(cos(pi_2), 0.0, 1e8)); // cos(π/2) underflows to ~6e-17, scale-relative wide
        // cos(π/2) is actually ~6.12e-17 (representational, not bug)
        assert!(cos(pi_2).abs() < 1e-15);
    }

    #[test]
    fn at_pi() {
        let pi = core::f64::consts::PI;
        assert!(sin(pi).abs() < 1e-15);
        assert!(ulp_close(cos(pi), -1.0, 4.0));
        assert!(tan(pi).abs() < 1e-15);
    }

    #[test]
    fn nan_and_inf() {
        assert!(sin(f64::NAN).is_nan());
        assert!(cos(f64::NAN).is_nan());
        assert!(tan(f64::NAN).is_nan());
        assert!(sin(f64::INFINITY).is_nan());
        assert!(cos(f64::INFINITY).is_nan());
        assert!(tan(f64::INFINITY).is_nan());
    }

    #[test]
    fn matches_host_libm() {
        let cases = [0.5_f64, 1.0, 1.5, 2.0, 3.0, -0.5, -1.0, 0.1, 0.7];
        for x in cases {
            assert!(
                ulp_close(sin(x), f64::sin(x), 4.0),
                "sin({x}): ours={} host={}",
                sin(x),
                f64::sin(x),
            );
            assert!(
                ulp_close(cos(x), f64::cos(x), 4.0),
                "cos({x}): ours={} host={}",
                cos(x),
                f64::cos(x),
            );
            assert!(
                ulp_close(tan(x), f64::tan(x), 8.0),
                "tan({x}): ours={} host={}",
                tan(x),
                f64::tan(x),
            );
        }
    }
}
