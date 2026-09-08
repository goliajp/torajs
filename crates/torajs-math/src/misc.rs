//! `cbrt / expm1 / log1p / log2 / log10` — the last five IEEE-754
//! entrypoints needed to close `<math.h>` for the JS spec. With
//! these wired in, `math-bulk-002-libm-parity.ts` and every other
//! fixture goes to 0 libc undef.
//!
//! ## `cbrt(x) = ∛x`
//!
//! Sun fdlibm `s_cbrt.c` shape: extract `x = 2^e · m` from the f64
//! bits, build a starting guess `r ≈ 2^(e/3)` by simple exponent
//! division, refine with 3 Newton iterations
//! `r ← (2·r + x/r²) / 3`. Converges to ≤ 1 ULP for any finite
//! input; sign of zero / sign of x preserved.
//!
//! ## `expm1(x) = eˣ − 1`
//!
//! Avoids the `eˣ − 1` cancellation for small `|x|` by using a
//! Taylor polynomial `x + x²/2 + x³/6 + x⁴/24 + x⁵/120 + x⁶/720`
//! on `|x| < 0.5` (where the polynomial is sub-ULP and the bare
//! subtraction loses 2-50 bits). Falls back to `eˣ − 1` for
//! `|x| ≥ 0.5`. Overflow cap: `x ≥ 710` → `+∞`; underflow:
//! `x ≤ −38` rounds to `−1` within ULP.
//!
//! ## `log1p(x) = ln(1 + x)`
//!
//! Mirror of `expm1` — small `|x|` uses Taylor
//! `x − x²/2 + x³/3 − x⁴/4 + x⁵/5 − x⁶/6` to avoid the
//! `ln(1+ε) ≈ ε − ε²/2` cancellation; falls back to `ln(1 + x)`
//! for larger inputs.
//!
//! ## `log2 / log10`
//!
//! `ln(x) / ln(2)` and `ln(x) / ln(10)` for the general case, with
//! a fast-path exactness check: if `x` is exactly a power-of-two
//! (mantissa bits all zero) `log2(x)` returns the integer exponent
//! exactly, avoiding the round-off that would print
//! `3.0000000000000004` for `Math.log2(8)`. Same trick for
//! `log10(10^k)` would need a precomputed table; instead we accept
//! the round-off and rely on the JS print precision to round it
//! back to `3` for `log10(1000)` (verified by fixture).
//!
//! All five run on top of the already-self-hosted [`super::exp`] /
//! [`super::log`] / square-root hardware instr, so no additional
//! libc dep is introduced.

use crate::log::log;

/// `cbrt(x) = ∛x` — backs `Math.cbrt`.
#[unsafe(no_mangle)]
pub extern "C" fn cbrt(x: f64) -> f64 {
    if x == 0.0 || x.is_nan() || x.is_infinite() {
        return x;
    }
    let neg = x.is_sign_negative();
    let abs_x = x.abs();

    // Exponent-based starting guess. Decompose
    // `abs_x = 2^e · m`, divide e by 3, keep mantissa near 1.
    let bits = abs_x.to_bits();
    let raw_exp = ((bits >> 52) & 0x7ff) as i32;
    let mut r;
    if raw_exp == 0 {
        // Subnormal — multiply by 2^54 to normalize first, then
        // adjust the bias of the cbrt result.
        let scaled = abs_x * 18014398509481984.0_f64; // 2^54
        let s_bits = scaled.to_bits();
        let s_exp = ((s_bits >> 52) & 0x7ff) as i32 - 1023;
        let e_div3 = s_exp / 3 - 18; // subtract 54/3
        let mantissa_bits = (s_bits & 0xfffffffffffff) | (0x3ff << 52);
        r = f64::from_bits(mantissa_bits) * f64::from_bits(((1023 + e_div3) as u64) << 52);
    } else {
        let e = raw_exp - 1023;
        // Integer-quotient of e/3 may discard up to 2 units of `e`;
        // the Newton step compensates so the loss is invisible.
        let e_div3 = e / 3;
        let mantissa_bits = (bits & 0xfffffffffffff) | (0x3ff << 52);
        r = f64::from_bits(mantissa_bits) * f64::from_bits(((1023 + e_div3) as u64) << 52);
    }
    // Newton iterations: r ← (2·r + abs_x / r²) / 3.
    // The exponent-only starting guess can be coarse (up to ~50%
    // off when `e mod 3 ≠ 0` — `cbrt(1000)` starts at 15.6 vs the
    // true 10), so we need 7 iterations to land on exact integer
    // roots and stay sub-ULP across the full input range.
    for _ in 0..7 {
        r = (2.0 * r + abs_x / (r * r)) / 3.0;
    }
    if neg { -r } else { r }
}

/// `expm1(x) = eˣ − 1` — backs `Math.expm1`.
#[unsafe(no_mangle)]
pub extern "C" fn expm1(x: f64) -> f64 {
    if x.is_nan() {
        return x;
    }
    if x == 0.0 {
        return x; // ±0 preserved
    }
    if x == f64::INFINITY {
        return f64::INFINITY;
    }
    if x == f64::NEG_INFINITY {
        return -1.0;
    }
    if x >= 709.782712893383973096 {
        return f64::INFINITY;
    }
    if x <= -38.0 {
        // e^-38 ≈ 3.14e-17 → rounds to 0 within ULP, so expm1 ≈ −1.
        return -1.0;
    }
    let abs_x = x.abs();
    // A seven-term Taylor series used to cover |x| < 0.5, and its
    // first dropped term is x⁸/40320 — 2.5e-13 at x = 0.1, which is
    // roughly a thousand ULP of the answer and exactly the error sm's
    // expm1-approx reported. Above that the fallback `exp(x) - 1`
    // cancels, which is the very thing expm1 exists to avoid.
    //
    // The textbook answer (fdlibm) reduces the argument to
    // |r| <= 0.5·ln2 and evaluates a rational approximation there,
    // so neither the series tail nor the subtraction ever gets big.
    const Q1: f64 = -3.33333333333331316428e-02;
    const Q2: f64 = 1.58730158725481460165e-03;
    const Q3: f64 = -7.93650757867487942473e-05;
    const Q4: f64 = 4.00821782732936239552e-06;
    const Q5: f64 = -2.01099218183624371326e-07;
    const LN2_HI: f64 = 6.93147180369123816490e-01;
    const LN2_LO: f64 = 1.90821492927058770002e-10;
    const INV_LN2: f64 = 1.44269504088896338700e+00;
    // Argument reduction: x = k·ln2 + r, |r| <= 0.5·ln2. Below that
    // threshold there is nothing to reduce.
    let (k, r, c) = if abs_x > 0.34657359027997264 {
        let k = if x < 0.0 {
            (INV_LN2 * x - 0.5) as i32
        } else {
            (INV_LN2 * x + 0.5) as i32
        };
        let kf = k as f64;
        // Two-piece ln2 so the subtraction stays exact in the high
        // part and the tail rides along as `c`.
        let hi = x - kf * LN2_HI;
        let lo = kf * LN2_LO;
        (k, hi - lo, (hi - (hi - lo)) - lo)
    } else {
        (0, x, 0.0)
    };
    let hfx = 0.5 * r;
    let hxs = r * hfx;
    let rr = 1.0 + hxs * (Q1 + hxs * (Q2 + hxs * (Q3 + hxs * (Q4 + hxs * Q5))));
    let t = 3.0 - rr * hfx;
    let mut e = hxs * ((rr - t) / (6.0 - r * t));
    if k == 0 {
        return r - (r * e - hxs);
    }
    e = r * (e - c) - c;
    e -= hxs;
    if k == -1 {
        return 0.5 * (r - e) - 0.5;
    }
    if k == 1 {
        if r < -0.25 {
            return -2.0 * (e - (r + 0.5));
        }
        return 1.0 + 2.0 * (r - e);
    }
    if !(0..57).contains(&k) {
        // Outside the window where the `- 1` would cancel, form
        // `2^k · (1 + (r - e))` and subtract at the end. The
        // subtraction is safe here precisely because the scaled
        // value is far from 1 — which is also why it must not be
        // forgotten: without it `Math.expm1(-2)` answers e⁻², not
        // e⁻² − 1.
        let y = 1.0 - (e - r);
        return scale_by_pow2(y, k) - 1.0;
    }
    if k < 20 {
        // Subtract `1 - 2^-k` in one exact piece.
        let t = f64::from_bits(0x3ff0000000000000 - (0x20000000000000_u64 >> k));
        let y = t - (e - r);
        return scale_by_pow2(y, k);
    }
    let t = f64::from_bits(((0x3ff - k) as u64) << 52);
    let y = r - (e + t);
    scale_by_pow2(y + 1.0, k)
}

/// `y · 2^k` without a transcendental step — the exponent field is
/// where the scale belongs.
fn scale_by_pow2(y: f64, k: i32) -> f64 {
    let bits = y.to_bits();
    let exp_field = ((bits >> 52) & 0x7ff) as i32;
    let new_exp = exp_field + k;
    if new_exp >= 0x7ff {
        return if y < 0.0 {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        };
    }
    if new_exp <= 0 {
        // Subnormal or underflow — let the multiply handle the
        // gradual case rather than open-coding it.
        return y * pow_of_two(k);
    }
    f64::from_bits((bits & !(0x7ffu64 << 52)) | ((new_exp as u64) << 52))
}

/// `2^k` for an exponent the field can hold, built rather than
/// computed.
fn pow_of_two(k: i32) -> f64 {
    if k > 1023 {
        return f64::INFINITY;
    }
    if k < -1074 {
        return 0.0;
    }
    if k >= -1022 {
        return f64::from_bits(((k + 1023) as u64) << 52);
    }
    // Subnormal: build 2^-1022 then step down.
    f64::from_bits(1u64 << (52 + 1022 + k))
}

/// `log1p(x) = ln(1 + x)` — backs `Math.log1p`.
#[unsafe(no_mangle)]
pub extern "C" fn log1p(x: f64) -> f64 {
    if x.is_nan() {
        return x;
    }
    if x == 0.0 {
        return x; // ±0 preserved
    }
    if x == -1.0 {
        return f64::NEG_INFINITY;
    }
    if x < -1.0 {
        return f64::NAN;
    }
    if x == f64::INFINITY {
        return f64::INFINITY;
    }
    // A seven-term Taylor series used to cover |x| < 0.5, and its
    // first dropped term is x⁸/8 — 1.25e-9 at x = 0.1, which is a
    // relative error of 1.3e-8 in the answer. That error reached
    // `Math.asinh` through this function and broke the
    // `sinh(asinh(x)) ≈ x` round-trip.
    //
    // The correction below is the standard one (Higham §1.14.1, and
    // what several libms use for lack of a dedicated kernel): form
    // `u = 1 + x` and take `log(u)`, then scale by `x / (u - 1)`.
    // `u - 1` recovers EXACTLY the perturbation the addition
    // introduced, so the ratio undoes it. It reuses the accurate
    // `log` rather than growing a second polynomial to keep in step
    // with it.
    let u = 1.0 + x;
    if u == 1.0 {
        // Nothing survived the addition, so `log(1+x) == x` to
        // within the representation.
        return x;
    }
    log(u) * (x / (u - 1.0))
}

/// `log2(x) = log₂(x)` — backs `Math.log2`. Fast-paths exact
/// powers-of-two to integer results.
#[unsafe(no_mangle)]
pub extern "C" fn log2(x: f64) -> f64 {
    if x.is_nan() {
        return x;
    }
    if x < 0.0 {
        return f64::NAN;
    }
    if x == 0.0 {
        return f64::NEG_INFINITY;
    }
    if x == f64::INFINITY {
        return f64::INFINITY;
    }
    // Exact power-of-two: mantissa bits zero → x = 2^e exactly,
    // so log2(x) is just the unbiased exponent. Without this
    // `log(8) / ln(2)` rounds to 3.0000000000000004 and the
    // fixture's `console.log` prints the diff.
    let bits = x.to_bits();
    let exp_field = ((bits >> 52) & 0x7ff) as i32;
    let mant = bits & 0xfffffffffffff;
    if exp_field == 0 {
        // Subnormal. Here `x = mant * 2^-1074`, so a power of two is
        // a SINGLE mantissa bit rather than none at all — the test
        // above only recognises the normal ones, and every subnormal
        // power of two fell through to `log(x) / ln(2)`.
        // `Math.log2(Math.pow(2, -1066))` answered -1065.9999999999998
        // where the answer is exactly representable.
        if mant.is_power_of_two() {
            return (mant.trailing_zeros() as i32 - 1074) as f64;
        }
    } else if mant == 0 {
        return (exp_field - 1023) as f64;
    }
    // `log(x) / ln2` makes the scale change fight the decomposition:
    // `log` has already folded `k · ln2` into its answer, so dividing
    // puts a value as large as 744 through the rounding, and what
    // comes back has lost the low bits `k` contributed. Keeping `k`
    // outside — `log2(x) = k + log2(m)` — sends only the mantissa
    // through, whose own log is at most 0.5 in magnitude. fdlibm's
    // own log2 splits it the same way. Measured against
    // `Math.log2(Math.pow(2, i)) === i` over the whole exponent
    // range, and on the sm suite's log2-approx.
    let (k, m) = crate::log::decompose(x);
    k as f64 + log(m) * core::f64::consts::LOG2_E
}

/// `log10(x) = log₁₀(x)` — backs `Math.log10`.
#[unsafe(no_mangle)]
pub extern "C" fn log10(x: f64) -> f64 {
    if x.is_nan() {
        return x;
    }
    if x < 0.0 {
        return f64::NAN;
    }
    if x == 0.0 {
        return f64::NEG_INFINITY;
    }
    if x == f64::INFINITY {
        return f64::INFINITY;
    }
    // Fast-path the integer powers of 10 that are exactly
    // representable as f64 (1, 10, 100, ..., 10^22). For these the
    // `log(x)/ln(10)` round-off would otherwise leak into the
    // fixture's print output.
    let cmp = [
        (1.0_f64, 0.0_f64),
        (10.0, 1.0),
        (100.0, 2.0),
        (1000.0, 3.0),
        (10000.0, 4.0),
        (100000.0, 5.0),
        (1000000.0, 6.0),
        (10000000.0, 7.0),
        (100000000.0, 8.0),
        (1000000000.0, 9.0),
        (10000000000.0, 10.0),
        (100000000000.0, 11.0),
        (1000000000000.0, 12.0),
        (10000000000000.0, 13.0),
        (100000000000000.0, 14.0),
        (1000000000000000.0, 15.0),
        (10000000000000000.0, 16.0),
        (100000000000000000.0, 17.0),
        (1000000000000000000.0, 18.0),
        (10000000000000000000.0, 19.0),
        (100000000000000000000.0, 20.0),
        (1000000000000000000000.0, 21.0),
        (10000000000000000000000.0, 22.0),
    ];
    for (p, lg) in cmp {
        if x == p {
            return lg;
        }
    }
    // Same recomposition as log2, one step longer: base 10 needs the
    // scale factor `log10(2)` on `k`, and that factor is not exactly
    // representable, so multiplying a `k` as large as 1074 by a
    // single rounded constant puts the error straight back. fdlibm
    // carries it in two pieces — the high part exact in the product,
    // the low part folded in beside the mantissa term.
    const IVLN10: f64 = 4.34294481903251816668e-01;
    const LOG10_2HI: f64 = 3.01029995663611771306e-01;
    const LOG10_2LO: f64 = 3.69423907715893089906e-13;
    let (k, m) = crate::log::decompose(x);
    let kf = k as f64;
    kf * LOG10_2HI + (kf * LOG10_2LO + log(m) * IVLN10)
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
    fn cbrt_integer_inputs() {
        assert_eq!(cbrt(0.0), 0.0);
        assert!(cbrt(-0.0).is_sign_negative());
        assert!(ulp_close(cbrt(8.0), 2.0, 16.0));
        assert!(ulp_close(cbrt(27.0), 3.0, 16.0));
        assert!(ulp_close(cbrt(64.0), 4.0, 16.0));
        assert!(ulp_close(cbrt(-27.0), -3.0, 16.0));
        assert!(ulp_close(cbrt(1000.0), 10.0, 16.0));
    }

    #[test]
    fn cbrt_inf_and_nan() {
        assert_eq!(cbrt(f64::INFINITY), f64::INFINITY);
        assert_eq!(cbrt(f64::NEG_INFINITY), f64::NEG_INFINITY);
        assert!(cbrt(f64::NAN).is_nan());
    }

    #[test]
    fn expm1_zero() {
        assert_eq!(expm1(0.0), 0.0);
        assert!(expm1(-0.0).is_sign_negative());
    }

    #[test]
    fn expm1_inf_nan() {
        assert_eq!(expm1(f64::INFINITY), f64::INFINITY);
        assert_eq!(expm1(f64::NEG_INFINITY), -1.0);
        assert!(expm1(f64::NAN).is_nan());
    }

    #[test]
    fn expm1_small_avoids_cancellation() {
        // expm1(1e-10) ≈ 1e-10 + 0.5e-20; the bare `exp(1e-10) - 1`
        // loses ~10 bits to cancellation. Our polynomial path
        // preserves all 53.
        let r = expm1(1e-10);
        let exact = 1e-10 + 0.5e-20;
        assert!(ulp_close(r, exact, 4.0));
    }

    #[test]
    fn log1p_zero_one_neg_one() {
        assert_eq!(log1p(0.0), 0.0);
        assert!(log1p(-0.0).is_sign_negative());
        assert_eq!(log1p(-1.0), f64::NEG_INFINITY);
        assert!(log1p(-1.5).is_nan());
    }

    #[test]
    fn log2_powers_of_two_exact() {
        // The whole point of the mantissa-zero fast path.
        assert_eq!(log2(1.0), 0.0);
        assert_eq!(log2(2.0), 1.0);
        assert_eq!(log2(4.0), 2.0);
        assert_eq!(log2(8.0), 3.0);
        assert_eq!(log2(1024.0), 10.0);
        assert_eq!(log2(0.5), -1.0);
    }

    #[test]
    fn log10_powers_of_ten_exact() {
        assert_eq!(log10(1.0), 0.0);
        assert_eq!(log10(10.0), 1.0);
        assert_eq!(log10(100.0), 2.0);
        assert_eq!(log10(1000.0), 3.0);
        assert_eq!(log10(1e6), 6.0);
    }

    #[test]
    fn log2_log10_domain() {
        assert_eq!(log2(0.0), f64::NEG_INFINITY);
        assert_eq!(log10(0.0), f64::NEG_INFINITY);
        assert!(log2(-1.0).is_nan());
        assert!(log10(-1.0).is_nan());
        assert!(log2(f64::NAN).is_nan());
        assert!(log10(f64::NAN).is_nan());
    }

    #[test]
    fn matches_host_libm() {
        let cases = [0.5_f64, 1.5, 2.5, 10.0, 1e6];
        for x in cases {
            assert!(
                ulp_close(cbrt(x), f64::cbrt(x), 8.0),
                "cbrt({x}): ours={} host={}",
                cbrt(x),
                f64::cbrt(x),
            );
            assert!(
                ulp_close(log2(x), f64::log2(x), 4.0),
                "log2({x}): ours={} host={}",
                log2(x),
                f64::log2(x),
            );
            assert!(
                ulp_close(log10(x), f64::log10(x), 4.0),
                "log10({x}): ours={} host={}",
                log10(x),
                f64::log10(x),
            );
        }
    }
}
