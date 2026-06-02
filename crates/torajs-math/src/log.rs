//! `log(x)` — IEEE-754 natural logarithm.
//!
//! Algorithm (Sun fdlibm `e_log.c` shape):
//! 1. Decompose `x = 2^k · m` where `m ∈ [√2/2, √2)` via bit
//!    extraction. If the raw mantissa `m₀ ∈ [1, 2)` falls above
//!    `√2`, divide it by 2 and bump `k`. This narrows the
//!    polynomial input range to roughly `[-0.293, 0.414]`.
//! 2. Let `f = m − 1`, `s = f / (2 + f)`. Then
//!    `log(m) = 2·atanh(s) = log((1+s)/(1−s))`.
//!    Approximate with the Sun fdlibm polynomial in `z = s²`:
//!    `R(z) = z·Lg1 + z²·Lg2 + z³·Lg3 + … + z⁷·Lg7`
//!    plus the correction `−hfsq + s·(hfsq + R)`, where
//!    `hfsq = f²/2`.
//! 3. Reconstruct: `log(x) = k·ln2_hi − ((hfsq − (s·(hfsq + R))) − f) +
//!    k·ln2_lo`. Two-piece ln2 keeps the `k` multiply ULP-accurate.
//!
//! Special cases:
//! - `NaN` → `NaN`
//! - `+∞` → `+∞`
//! - `x < 0` (including `-0`) → `NaN` (`log` of negative is
//!   undefined in real-valued ES Math.log spec)
//! - `+0` → `-∞`
//! - `x = 1` → `+0` (exact)
//!
//! Coefficients are Sun fdlibm constants (public-domain algorithm,
//! hand-typed code per pillar #2).

/// `ln(2)` high-precision split — same constants as `exp.rs`.
const LN2_HI: f64 = 6.93147180369123816490e-01;
const LN2_LO: f64 = 1.90821492927058770002e-10;

/// Polynomial coefficients for the Sun fdlibm log reduced form,
/// powers of `s²`.
const LG1: f64 = 6.666666666666735130e-01;
const LG2: f64 = 3.999999999940941908e-01;
const LG3: f64 = 2.857142874366239149e-01;
const LG4: f64 = 2.222219843214978396e-01;
const LG5: f64 = 1.818357216161805012e-01;
const LG6: f64 = 1.531383769920937332e-01;
const LG7: f64 = 1.479819860511658591e-01;

/// `log(x) -> ln(x)` — backs `Math.log` and the LLVM-emitted libm
/// `log` symbol; replaces the `f64::ln` path that linked to libc.
#[unsafe(no_mangle)]
pub extern "C" fn log(x: f64) -> f64 {
    // NaN / negative → NaN; `-0` is negative (sign bit set).
    if x.is_nan() || x < 0.0 {
        return f64::NAN;
    }
    if x == 0.0 {
        return f64::NEG_INFINITY;
    }
    if x.is_infinite() {
        return f64::INFINITY;
    }

    // Decompose x = 2^k · m, m ∈ [1, 2).
    let bits = x.to_bits();
    let raw_exp = ((bits >> 52) & 0x7ff) as i32;
    let mut k: i32;
    let mut m: f64;
    if raw_exp == 0 {
        // Subnormal: normalize by multiplying by 2^54 then adjusting k.
        let scaled = x * 18014398509481984.0_f64; // 2^54
        let sbits = scaled.to_bits();
        let sraw = ((sbits >> 52) & 0x7ff) as i32;
        k = sraw - 1023 - 54;
        m = f64::from_bits((sbits & 0xfffffffffffff) | (0x3ff << 52));
    } else {
        k = raw_exp - 1023;
        m = f64::from_bits((bits & 0xfffffffffffff) | (0x3ff << 52));
    }
    // Pull m into [√2/2, √2). √2 ≈ 1.41421356.
    if m > core::f64::consts::SQRT_2 {
        m *= 0.5;
        k += 1;
    }

    // f = m - 1, s = f / (2 + f), z = s², w = z².
    let f = m - 1.0;
    let s = f / (2.0 + f);
    let z = s * s;
    let w = z * z;

    // Sun fdlibm polynomial:
    //   t1 = w · (Lg2 + w·(Lg4 + w·Lg6))   ← even powers of s²
    //   t2 = z · (Lg1 + w·(Lg3 + w·(Lg5 + w·Lg7)))   ← odd powers
    //   R = t1 + t2.
    let t1 = w * (LG2 + w * (LG4 + w * LG6));
    let t2 = z * (LG1 + w * (LG3 + w * (LG5 + w * LG7)));
    let r = t1 + t2;
    let hfsq = 0.5 * f * f;

    // log(x) = k·ln2 − ((hfsq − (s·(hfsq + R))) − f)
    // with the two-piece ln2 for ULP-accurate k multiply.
    let kf = k as f64;
    kf * LN2_HI - ((hfsq - (s * (hfsq + r) + kf * LN2_LO)) - f)
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
    fn log_one_is_zero() {
        assert_eq!(log(1.0), 0.0);
    }

    #[test]
    fn log_e_is_one() {
        assert!(ulp_close(log(core::f64::consts::E), 1.0, 2.0));
    }

    #[test]
    fn log_2_is_ln2() {
        assert!(ulp_close(log(2.0), core::f64::consts::LN_2, 2.0));
    }

    #[test]
    fn log_negative_is_nan() {
        // Strictly-negative reals → NaN. `-0.0` is treated as zero by
        // the IEEE comparison (`-0 == 0`), so it falls through to the
        // zero branch and returns `-∞`, matching ES + C99 spec.
        assert!(log(-1.0).is_nan());
        assert!(log(-1.5).is_nan());
    }

    #[test]
    fn log_zero_is_neg_inf() {
        assert_eq!(log(0.0), f64::NEG_INFINITY);
        assert_eq!(log(-0.0), f64::NEG_INFINITY);
    }

    #[test]
    fn log_nan_is_nan() {
        assert!(log(f64::NAN).is_nan());
    }

    #[test]
    fn log_inf_is_inf() {
        assert_eq!(log(f64::INFINITY), f64::INFINITY);
    }

    #[test]
    fn log_matches_host_libm_to_4ulp() {
        let cases = [0.5_f64, 1.5, 2.0, 10.0, 100.0, 1e10, 1e-10, 0.999, 1.001];
        for x in cases {
            let host = x.ln();
            let ours = log(x);
            assert!(
                ulp_close(ours, host, 4.0),
                "log({x}): ours={ours} host={host}",
            );
        }
    }
}
