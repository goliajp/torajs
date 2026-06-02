//! `fmod(x, y)` — IEEE-754 floating-point remainder.
//!
//! `x = q*y + r` where `q` is the integer trunc of `x/y` and the
//! result `r` has the sign of `x`. LLVM lowers the f64 `frem` IR
//! operator and `f64 % f64` to a call to this symbol; without an
//! in-binary `fmod` definition the linker resolves it against
//! libSystem libm, pulling `_fmod` into every user binary that does
//! float modulo. This shim drops it.
//!
//! ## Algorithm
//!
//! Bit-level long-division on the IEEE-754 mantissa. Same shape as
//! Sun fdlibm's `e_fmod.c` (which musl / FreeBSD / glibc all
//! inherit): normalize both operands' mantissas to bit-52, shift
//! `x`'s mantissa one bit at a time, subtracting `y`'s whenever the
//! running difference stays non-negative. At end-of-loop the
//! remaining mantissa is the modulo result; rescale it back to its
//! exponent and re-pack with the sign of the original `x`.
//!
//! This is the textbook IEEE-correct path: no transcendental, no
//! polynomial — every step is exact in two's complement on the
//! 52-bit mantissa. Matches glibc / libsystem fmod bit-for-bit on
//! all finite normal / subnormal inputs.
//!
//! Special cases (handled before the loop):
//! - `y == 0` or `x = ±∞` or NaN-in → NaN (`(x*y)/(x*y)` returns
//!   IEEE-correct NaN for all three, matching Sun's idiom).
//! - `|x| < |y|` → x unchanged.
//! - `|x| == |y|` → `0 * x` (preserves sign of x, IEEE rule).

/// `fmod(x, y) -> r` where `x = q*y + r`, `|r| < |y|`, `sign(r) ==
/// sign(x)`. Backs the LLVM-emitted libm `fmod` symbol and the
/// runtime `Math.*` fmod-using paths.
#[unsafe(no_mangle)]
pub extern "C" fn fmod(x: f64, y: f64) -> f64 {
    let mut uxi = x.to_bits();
    let mut uyi = y.to_bits();
    let mut ex = ((uxi >> 52) & 0x7ff) as i32;
    let mut ey = ((uyi >> 52) & 0x7ff) as i32;
    let sx = uxi >> 63;

    // Special: y == 0, or x is Inf/NaN, or y is NaN → NaN.
    // The Sun idiom `(x*y)/(x*y)` lets the FPU produce the
    // IEEE-correct NaN without an explicit NaN constant.
    if (uyi << 1) == 0 || y.is_nan() || ex == 0x7ff {
        return (x * y) / (x * y);
    }

    // |x| <= |y| short-circuits.
    if (uxi << 1) <= (uyi << 1) {
        if (uxi << 1) == (uyi << 1) {
            // |x| == |y| → ±0 with sign of x.
            return 0.0 * x;
        }
        return x;
    }

    // Normalize subnormals: extract leading bit so the mantissa
    // matches the normal-number 53-bit (1.fraction) shape.
    if ex == 0 {
        // shift left until the leading 1 hits bit 63
        let mut i = uxi << 12;
        while (i >> 63) == 0 {
            ex -= 1;
            i <<= 1;
        }
        uxi <<= (-ex + 1) as u32;
    } else {
        // Normal: clear the exponent bits, set the implicit bit-52.
        uxi &= u64::MAX >> 12;
        uxi |= 1u64 << 52;
    }
    if ey == 0 {
        let mut i = uyi << 12;
        while (i >> 63) == 0 {
            ey -= 1;
            i <<= 1;
        }
        uyi <<= (-ey + 1) as u32;
    } else {
        uyi &= u64::MAX >> 12;
        uyi |= 1u64 << 52;
    }

    // Long-division mod loop. At each step, shift uxi left by 1
    // (multiply by 2) and conditionally subtract uyi if the top
    // bit went into the "fits one more y" range.
    while ex > ey {
        let i = uxi.wrapping_sub(uyi);
        if (i >> 63) == 0 {
            if i == 0 {
                return 0.0 * x;
            }
            uxi = i;
        }
        uxi <<= 1;
        ex -= 1;
    }
    let i = uxi.wrapping_sub(uyi);
    if (i >> 63) == 0 {
        if i == 0 {
            return 0.0 * x;
        }
        uxi = i;
    }
    // Re-normalize: find the leading set bit and shift it back to
    // mantissa position 52.
    while (uxi >> 52) == 0 {
        uxi <<= 1;
        ex -= 1;
    }

    // Re-pack. ex may be ≤ 0 → produce a subnormal by shifting
    // right; > 0 → drop the implicit bit, OR in the biased exponent.
    if ex > 0 {
        uxi -= 1u64 << 52;
        uxi |= (ex as u64) << 52;
    } else {
        uxi >>= (-ex + 1) as u32;
    }
    uxi |= sx << 63;
    f64::from_bits(uxi)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_integer_remainders() {
        assert_eq!(fmod(7.0, 3.0), 1.0);
        assert_eq!(fmod(10.0, 4.0), 2.0);
        assert_eq!(fmod(20.0, 5.0), 0.0);
    }

    #[test]
    fn sign_of_x_preserved() {
        // C99 / IEEE-754: result has sign of x, NOT y.
        assert_eq!(fmod(-7.0, 3.0), -1.0);
        assert_eq!(fmod(7.0, -3.0), 1.0);
        assert_eq!(fmod(-7.0, -3.0), -1.0);
    }

    #[test]
    fn fractional_inputs() {
        // 5.5 = 2*2 + 1.5
        assert!((fmod(5.5, 2.0) - 1.5).abs() < 1e-15);
        // 0.5 = 0*0.3 + 0.5? actually 0.5 = 1*0.3 + 0.2
        let r = fmod(0.5, 0.3);
        assert!((r - 0.2).abs() < 1e-15);
    }

    #[test]
    fn x_smaller_than_y_returns_x() {
        assert_eq!(fmod(2.0, 7.0), 2.0);
        assert_eq!(fmod(-2.0, 7.0), -2.0);
    }

    #[test]
    fn y_zero_yields_nan() {
        assert!(fmod(7.0, 0.0).is_nan());
        assert!(fmod(-7.0, 0.0).is_nan());
        assert!(fmod(0.0, 0.0).is_nan());
    }

    #[test]
    fn x_infinite_yields_nan() {
        assert!(fmod(f64::INFINITY, 3.0).is_nan());
        assert!(fmod(f64::NEG_INFINITY, 3.0).is_nan());
    }

    #[test]
    fn nan_in_yields_nan_out() {
        assert!(fmod(f64::NAN, 3.0).is_nan());
        assert!(fmod(3.0, f64::NAN).is_nan());
    }

    #[test]
    fn zero_x_finite_y_returns_zero() {
        assert_eq!(fmod(0.0, 3.0), 0.0);
        // Sign of x is preserved on the ±0 result.
        let nz = fmod(-0.0, 3.0);
        assert_eq!(nz, 0.0);
        assert!(nz.is_sign_negative());
    }

    #[test]
    fn x_equals_y_returns_zero_with_x_sign() {
        let nz = fmod(-5.0, 5.0);
        assert_eq!(nz, 0.0);
        assert!(nz.is_sign_negative());
        let pz = fmod(5.0, 5.0);
        assert_eq!(pz, 0.0);
        assert!(pz.is_sign_positive());
    }

    #[test]
    fn matches_native_on_representative_pairs() {
        // Cross-check against the f64 `%` operator (which LLVM
        // lowers to libc fmod on glibc / libsystem — but in cargo
        // test that's the host's libm, not our shim). The
        // operator's host-libm output is the IEEE oracle.
        let cases = [
            (1.5_f64, 1.0),
            (5.25, 2.5),
            (100.7, 6.3),
            (-13.0, 4.0),
            (1e10, 7.0),
            (1e-300, 1e-301),
        ];
        for (x, y) in cases {
            let ours = fmod(x, y);
            let host = x % y;
            assert!(
                (ours - host).abs() <= host.abs().max(1.0) * 1e-15,
                "fmod({x}, {y}): ours={ours} host={host}",
            );
        }
    }
}
