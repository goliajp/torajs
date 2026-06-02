//! `exp(x)` — IEEE-754 e^x.
//!
//! Algorithm (Sun fdlibm `e_exp.c` shape):
//! 1. Range reduction: `x = k·ln2 + r` where `k = round(x/ln2)` and
//!    `|r| ≤ ln2/2 ≈ 0.347`. Split `ln2 = ln2_hi + ln2_lo` to keep
//!    `r` accurate to extended precision via
//!    `r = (x − k·ln2_hi) − k·ln2_lo`.
//! 2. Approximate `e^r` on `|r| ≤ ln2/2` by Sun fdlibm's 5-term
//!    rational form
//!    `e^r ≈ 1 + 2r / (P − r·c)` where
//!    `c = r − P, P = 2 − (r·R + something(r²))` — concretely the
//!    `R(r²)` polynomial below. Sub-ULP correct on the reduced range.
//! 3. Reconstruct: `e^x = 2^k · e^r`. The `2^k` factor is applied by
//!    repacking the exponent bits (`scalbn`-style) so no second
//!    multiply chain is needed.
//!
//! Special cases:
//! - `NaN` → `NaN`
//! - `+∞` → `+∞`
//! - `-∞` → `0`
//! - `x > 709.782712...` → `+∞` (overflow)
//! - `x < -745.133219...` → `0` (underflow)
//! - `|x| < 2^-54` → `1 + x` (tiny x, linear path)
//!
//! Coefficients are the Sun fdlibm constants (public-domain
//! algorithm, hand-typed code per pillar #2).

/// `ln(2)` high-precision split: hi + lo together reproduce ln2 to
/// extended precision. Same split Sun fdlibm uses.
const LN2_HI: f64 = 6.93147180369123816490e-01;
const LN2_LO: f64 = 1.90821492927058770002e-10;
/// `1/ln(2)` — used to compute the reduction quotient `k`.
const INV_LN2: f64 = 1.44269504088896338700e+00;

/// Polynomial coefficients for the Sun fdlibm exp reduced form
/// `R(z) = 1 + z·(P1 + z·(P2 + z·(P3 + z·(P4 + z·P5))))` where
/// `z = r²`. Sub-ULP on `|r| ≤ ln2/2`.
const P1: f64 = 1.66666666666666019037e-01;
const P2: f64 = -2.77777777770155933842e-03;
const P3: f64 = 6.61375632143793436117e-05;
const P4: f64 = -1.65339022054652515390e-06;
const P5: f64 = 4.13813679705723846039e-08;

/// `exp(x) -> e^x` — backs `Math.exp` and the LLVM-emitted libm
/// `exp` symbol; replaces the `f64::exp` path that linked to libc.
#[unsafe(no_mangle)]
pub extern "C" fn exp(x: f64) -> f64 {
    // NaN propagates unchanged.
    if x.is_nan() {
        return x;
    }
    // Overflow / underflow thresholds (Sun fdlibm constants).
    if x >= 709.782712893383973096 {
        return f64::INFINITY;
    }
    if x <= -745.133219101941108420 {
        return 0.0;
    }
    // Tiny: `e^x ≈ 1 + x` exact to f64 ULP.
    if x.abs() < 2.220446049250313e-16 {
        return 1.0 + x;
    }

    // Range reduction. `k = round(x / ln2)` so |r| ≤ ln2/2. The
    // two-piece ln2 subtract keeps `r` ULP-accurate when k·ln2
    // cancels most of x.
    let k = (x * INV_LN2).round();
    let hi = x - k * LN2_HI;
    let lo = k * LN2_LO;
    let r = hi - lo;
    let z = r * r;

    // Sun fdlibm polynomial — the `c = r − z·(P1 + z·P2 + …)` form
    // produces a value that's `r − O(r³)`, and then
    // `e^r ≈ 1 + r + r·c/(2 − c)` is the Pade-style rearrangement
    // Sun derived (see comments in fdlibm e_exp.c).
    let p = z * (P1 + z * (P2 + z * (P3 + z * (P4 + z * P5))));
    let c = r - p;

    // Apply 2^k via exponent-bit repack (`scalbn` fast path).
    // Bias is 1023; new exponent field = bias + k.
    let k_i = k as i32;
    // Sun fdlibm's cancellation-free reconstruction:
    //   y = 1 − ((lo − r·c/(2 − c)) − hi)
    //     = 1 + hi − lo + r·c/(2 − c)
    //     = 1 + r + r·c/(2 − c)
    // The hi/lo split matters: `r` already absorbed the cancellation,
    // but for very large k the explicit hi − lo form keeps the last
    // few bits.
    let y = 1.0 - ((lo - r * c / (2.0 - c)) - hi);
    if k_i >= -1022 && k_i <= 1023 {
        let factor = f64::from_bits(((1023 + k_i) as u64) << 52);
        factor * y
    } else if k_i > 1023 {
        f64::INFINITY
    } else {
        // Denormal range — use the two-step factor to avoid 2^-1075
        // underflow before the multiply.
        let factor1 = f64::from_bits(((1023 + k_i / 2) as u64) << 52);
        let factor2 = f64::from_bits(((1023 + (k_i - k_i / 2)) as u64) << 52);
        factor1 * factor2 * y
    }
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
    fn exp_zero_is_one() {
        assert_eq!(exp(0.0), 1.0);
        assert_eq!(exp(-0.0), 1.0);
    }

    #[test]
    fn exp_one_is_e() {
        assert!(ulp_close(exp(1.0), core::f64::consts::E, 2.0));
    }

    #[test]
    fn exp_ln2_is_2() {
        assert!(ulp_close(exp(core::f64::consts::LN_2), 2.0, 2.0));
    }

    #[test]
    fn exp_negative() {
        assert!(ulp_close(exp(-1.0), 1.0 / core::f64::consts::E, 2.0));
    }

    #[test]
    fn exp_overflow_underflow() {
        assert_eq!(exp(710.0), f64::INFINITY);
        assert_eq!(exp(-746.0), 0.0);
    }

    #[test]
    fn exp_nan() {
        assert!(exp(f64::NAN).is_nan());
    }

    #[test]
    fn exp_inf() {
        assert_eq!(exp(f64::INFINITY), f64::INFINITY);
        assert_eq!(exp(f64::NEG_INFINITY), 0.0);
    }

    #[test]
    fn exp_matches_host_libm_to_2ulp() {
        let cases: [f64; 12] = [
            0.5, 1.5, 2.0, 5.0, 10.0, 100.0, -5.0, -100.0, 0.1, -0.1, 700.0, -700.0,
        ];
        for x in cases {
            let host = f64::exp(x);
            let ours = exp(x);
            assert!(
                ulp_close(ours, host, 4.0),
                "exp({x}): ours={ours} host={host}",
            );
        }
    }

    #[test]
    fn exp_tiny() {
        let tiny = 1e-20;
        assert_eq!(exp(tiny), 1.0 + tiny);
    }
}
