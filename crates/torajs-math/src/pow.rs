//! `pow(x, y)` — IEEE-754 x^y.
//!
//! Composite algorithm: reduce to `exp(y · log|x|)` for the general
//! case, with explicit handling of every special case the C99 /
//! POSIX `pow(3)` and ES `Math.pow` specs distinguish. The two
//! transcendentals come from [`super::exp`] / [`super::log`]; the
//! special-case lattice is the textbook 28-case table from C99
//! Annex F.10.4.4.
//!
//! Precision: each of `log` and `exp` is ~1 ULP on the reduced
//! range, the inner multiply `y · log(x)` adds ~0.5 ULP, and the
//! outer `exp` rounds; total worst-case ~3 ULP. For the JS use
//! pattern (integer / half-integer / small-fractional bases) this
//! is comfortably bit-exact in the print-rounded `console.log`
//! output that fixtures compare. Sub-ULP impl is the musl
//! split-table approach; we'll upgrade if a fixture surfaces a
//! diff that needs it.

use crate::exp::exp;
use crate::log::log;

/// True if `y` is mathematically an integer that fits in f64 with no
/// fractional bits — used by the `pow(negative, y)` branch to decide
/// between real-valued result and `NaN`.
#[inline]
fn is_integer(y: f64) -> bool {
    y == y.trunc() && y.is_finite()
}

/// True if `y` is an odd integer (within f64's integer-exact range
/// ±2^53). Used for the `pow(neg, odd_int)` sign-flip case.
#[inline]
fn is_odd_integer(y: f64) -> bool {
    if !is_integer(y) {
        return false;
    }
    if y.abs() >= 9007199254740992.0 {
        // |y| ≥ 2^53 → every f64 here is even (mantissa has no bit-0).
        return false;
    }
    (y as i64) & 1 != 0
}

/// `pow(x, y) -> x^y` — backs `Math.pow` and the LLVM-emitted libm
/// `pow` symbol; replaces the `f64::powf` path that linked to libc.
///
/// Special-case lattice is ES §6.1.6.1.3 Number::exponentiate. It
/// matches C99 Annex F.10.4.4 in every cell but one: C99 says
/// `pow(±1, ±∞) = 1`, ES says NaN — every caller of this symbol is
/// a JS `**` / `Math.pow`, so ES wins (rotation 240: the C99 cell
/// answered `(-1) ** Infinity === 1` where bun answers NaN).
#[unsafe(no_mangle)]
pub extern "C" fn pow(x: f64, y: f64) -> f64 {
    pow_es(x, y)
}

/// The implementation behind [`pow`]. Split out (rotation 240) so
/// the unit tests exercise THIS code: in a host-linked test binary
/// the `no_mangle pow` symbol resolves to libSystem's C99 `pow`,
/// and the pre-split assertions silently tested libm — the one
/// lattice cell where ES and C99 differ was pinned to the WRONG
/// (C99) answer and never caught.
fn pow_es(x: f64, y: f64) -> f64 {
    // y == 0 → 1 for ANY x (including NaN, ±∞, ±0). ES + C99 agree.
    if y == 0.0 {
        return 1.0;
    }
    // NaN-in (other than the y=0 hatch above) → NaN.
    if x.is_nan() || y.is_nan() {
        return f64::NAN;
    }
    // y == 1 → x.
    if y == 1.0 {
        return x;
    }

    // |y| == ∞ branches: depends on |x| vs 1. Ordered BEFORE the
    // x == 1 shortcut — ES answers NaN for |x| == 1 here.
    if y.is_infinite() {
        let abs_x = x.abs();
        if abs_x == 1.0 {
            return f64::NAN;
        }
        // y == +∞:  |x| < 1 → 0,  |x| > 1 → +∞
        // y == -∞:  |x| < 1 → +∞, |x| > 1 → 0
        let big = (abs_x > 1.0) ^ y.is_sign_negative();
        return if big { f64::INFINITY } else { 0.0 };
    }

    // x == 1 → 1 for any finite y (infinite y handled above).
    if x == 1.0 {
        return 1.0;
    }

    // x == ±0 branches.
    if x == 0.0 {
        // pow(±0, y<0):  ±∞ (sign per y odd-int)
        // pow(±0, y>0):  ±0  (sign per y odd-int)
        let neg = x.is_sign_negative() && is_odd_integer(y);
        if y < 0.0 {
            return if neg {
                f64::NEG_INFINITY
            } else {
                f64::INFINITY
            };
        } else {
            return if neg { -0.0 } else { 0.0 };
        }
    }

    // x == ±∞ branches.
    if x.is_infinite() {
        let neg = x.is_sign_negative() && is_odd_integer(y);
        if y < 0.0 {
            return if neg { -0.0 } else { 0.0 };
        } else {
            return if neg {
                f64::NEG_INFINITY
            } else {
                f64::INFINITY
            };
        }
    }

    // x < 0: real-valued result only when y is an integer; otherwise
    // ES says NaN. Sign of result flips on odd y.
    let sign_flip = x.is_sign_negative();
    if sign_flip {
        if !is_integer(y) {
            return f64::NAN;
        }
    }

    // y == 0.5 → sqrt: correctly rounded where the exp(y·log x)
    // general path drifts a ulp (`2 ** 0.5` printed …373095 vs
    // libm's …3730951). Safe only AFTER the x == ±0 / x == ±∞
    // branches (C99+ES: pow(-0, 0.5) = +0 but sqrt(-0) = -0;
    // pow(-∞, 0.5) = +∞ but sqrt(-∞) = NaN); a remaining negative
    // x answers NaN through sqrt exactly as pow requires.
    if y == 0.5 {
        return x.sqrt();
    }

    // Integer-y fast path: repeated squaring. Gives bit-exact
    // results for cases like `2 ** 53` that the
    // `exp(y · log(x))` general path drifts a few ULPs on (which
    // matters for `Number.isSafeInteger` predicates and any
    // integer-arithmetic chain). Bounded |y| so the loop count is
    // 64 iterations max (log₂(2^63) = 63 bits in i64). Outside the
    // bound, fall through to the transcendental path (precision
    // loss there is already the value's own large-magnitude noise).
    if is_integer(y) && y.abs() < 9223372036854775808.0 {
        return powi(x, y as i64);
    }

    // General path: |x|^y = exp(y * log|x|).
    let abs_x = x.abs();
    let r = exp(y * log(abs_x));
    if sign_flip && is_odd_integer(y) {
        -r
    } else {
        r
    }
}

/// Integer-exponent power by repeated squaring — bit-exact for all
/// representable results, no transcendental round-off.
#[inline]
fn powi(x: f64, mut n: i64) -> f64 {
    // Handle the negative-exponent symmetry as `1 / x^|n|` so the
    // squaring loop only ever sees a non-negative exponent.
    let invert = n < 0;
    if invert {
        // i64::MIN abs overflows; n=-2^63 means infinite for any
        // finite x ≠ 1, so just emulate the limit.
        if n == i64::MIN {
            return 0.0;
        }
        n = -n;
    }
    let mut acc = 1.0_f64;
    let mut base = x;
    while n > 0 {
        if (n & 1) != 0 {
            acc *= base;
        }
        n >>= 1;
        if n > 0 {
            base *= base;
        }
    }
    if invert { 1.0 / acc } else { acc }
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
    fn integer_powers() {
        assert!(ulp_close(pow_es(2.0, 10.0), 1024.0, 4.0));
        assert!(ulp_close(pow_es(3.0, 4.0), 81.0, 4.0));
        assert!(ulp_close(pow_es(10.0, 6.0), 1000000.0, 4.0));
    }

    #[test]
    fn fractional_exponents() {
        assert!(ulp_close(pow_es(4.0, 0.5), 2.0, 4.0));
        assert!(ulp_close(pow_es(8.0, 1.0 / 3.0), 2.0, 16.0));
    }

    #[test]
    fn negative_base_integer_exponent() {
        assert_eq!(pow_es(-2.0, 3.0), -8.0);
        assert!(ulp_close(pow_es(-2.0, 4.0), 16.0, 4.0));
    }

    #[test]
    fn negative_base_noninteger_exponent_is_nan() {
        assert!(pow_es(-2.0, 0.5).is_nan());
        assert!(pow_es(-2.0, 1.5).is_nan());
    }

    #[test]
    fn y_zero_returns_one_for_any_x() {
        assert_eq!(pow_es(0.0, 0.0), 1.0);
        assert_eq!(pow_es(f64::NAN, 0.0), 1.0);
        assert_eq!(pow_es(f64::INFINITY, 0.0), 1.0);
        assert_eq!(pow_es(-1.0, 0.0), 1.0);
    }

    #[test]
    fn x_one_returns_one_for_any_finite_y() {
        assert_eq!(pow_es(1.0, 100.0), 1.0);
        assert_eq!(pow_es(1.0, -7.5), 1.0);
    }

    #[test]
    fn abs_one_base_infinite_exponent_is_nan() {
        // ES §6.1.6.1.3 — the one cell where ES differs from C99
        // (C99 says 1).
        assert!(pow_es(1.0, f64::INFINITY).is_nan());
        assert!(pow_es(-1.0, f64::INFINITY).is_nan());
        assert!(pow_es(1.0, f64::NEG_INFINITY).is_nan());
        assert!(pow_es(-1.0, f64::NEG_INFINITY).is_nan());
    }

    #[test]
    fn half_exponent_is_correctly_rounded_sqrt() {
        assert_eq!(pow_es(2.0, 0.5), 2.0_f64.sqrt());
        assert_eq!(pow_es(-0.0, 0.5), 0.0);
        assert!(pow_es(-0.0, 0.5).is_sign_positive());
        assert_eq!(pow_es(f64::NEG_INFINITY, 0.5), f64::INFINITY);
        assert!(pow_es(-2.0, 0.5).is_nan());
    }

    #[test]
    fn pow_zero_negative_exponent_is_infinity() {
        assert_eq!(pow_es(0.0, -2.0), f64::INFINITY);
        assert_eq!(pow_es(-0.0, -3.0), f64::NEG_INFINITY); // -∞ for odd int
        assert_eq!(pow_es(-0.0, -4.0), f64::INFINITY); // +∞ for even int
    }

    #[test]
    fn pow_infinity_cases() {
        assert_eq!(pow_es(2.0, f64::INFINITY), f64::INFINITY);
        assert_eq!(pow_es(0.5, f64::INFINITY), 0.0);
        assert_eq!(pow_es(2.0, f64::NEG_INFINITY), 0.0);
        assert_eq!(pow_es(0.5, f64::NEG_INFINITY), f64::INFINITY);
    }

    #[test]
    fn matches_host_powf_on_common_cases() {
        let cases = [
            (2.0_f64, 10.0),
            (3.0, 4.0),
            (10.0, 0.5),
            (1.5, 2.5),
            (100.0, 0.25),
            (0.7, 3.0),
        ];
        for (x, y) in cases {
            let host = x.powf(y);
            let ours = pow_es(x, y);
            assert!(
                ulp_close(ours, host, 32.0),
                "pow_es({x}, {y}): ours={ours} host={host}",
            );
        }
    }
}
