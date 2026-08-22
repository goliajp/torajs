//! IEEE-754 binary16 ↔ binary64, for `Float16Array`
//! (RFC 20260823-typedarray-substrate 刀 5).
//!
//! Rust has no stable `f16`, so both directions are written out. The
//! encode rounds ties to EVEN, which is what §6.2.9
//! NumberToRawBytes specifies and what makes 2049 store as 2048
//! rather than 2052.
//!
//! It does NOT go through `f32` on the way. A double rounding
//! (binary64 → binary32 → binary16) differs from a single one on the
//! values that sit exactly halfway in binary16 but not in binary32 —
//! rare, silent, and exactly the kind of thing a conformance suite
//! finds and a spot check does not.

/// binary16 exponent field for infinity / NaN.
const EXP_INF: u16 = 0x7C00;
/// binary64 mantissa bits that do not survive into binary16.
const DROPPED: u32 = 42;

/// Decode — every binary16 value is exactly representable in
/// binary64, so this direction never rounds.
pub(crate) fn f16_bits_to_f64(h: u16) -> f64 {
    let sign = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exp = ((h >> 10) & 0x1F) as i32;
    let man = f64::from(h & 0x03FF);
    if exp == 0 {
        // Subnormal: no implicit leading 1, fixed scale 2^-24.
        return sign * man * f64::from(1u32 << 24).recip();
    }
    if exp == 0x1F {
        return if man == 0.0 {
            sign * f64::INFINITY
        } else {
            f64::NAN
        };
    }
    sign * (1.0 + man / 1024.0) * exp2(exp - 15)
}

/// Encode, rounding ties to even.
pub(crate) fn f64_to_f16_bits(v: f64) -> u16 {
    let bits = v.to_bits();
    let sign = ((bits >> 48) as u16) & 0x8000;
    let exp64 = ((bits >> 52) & 0x7FF) as i32;
    let man64 = bits & 0x000F_FFFF_FFFF_FFFF;
    if exp64 == 0x7FF {
        // A NaN stays a NaN (quiet, canonical payload); an infinity
        // stays an infinity.
        return sign | EXP_INF | if man64 != 0 { 0x0200 } else { 0 };
    }
    if exp64 == 0 && man64 == 0 {
        return sign;
    }
    // binary64 subnormals are far below binary16's smallest
    // subnormal, so they all round to zero.
    if exp64 == 0 {
        return sign;
    }
    let e16 = exp64 - 1023 + 15;
    if e16 >= 0x1F {
        // Anything at or above 2^16 overflows; so does anything from
        // 65520 up, which is the tie between the largest normal
        // (65504) and 65536 and rounds AWAY under ties-to-even.
        return sign | EXP_INF;
    }
    let full = (1u64 << 52) | man64;
    if e16 <= 0 {
        // Subnormal: shift the implicit-1 significand down to the
        // fixed 2^-24 scale, then round what falls off.
        let shift = DROPPED + (1 - e16) as u32;
        if shift >= 64 {
            return sign;
        }
        return sign | round_shift(full, shift) as u16;
    }
    let m = round_shift(man64, DROPPED);
    if m == 0x0400 {
        // The rounding carried out of the mantissa — the exponent
        // takes it, and an exponent that then reaches 0x1F is an
        // overflow to infinity.
        let e = (e16 + 1) as u16;
        if e >= 0x1F {
            return sign | EXP_INF;
        }
        return sign | (e << 10);
    }
    sign | ((e16 as u16) << 10) | m as u16
}

/// `value >> shift`, rounding the discarded bits to nearest with
/// ties to even. The carry is left in the result — a subnormal that
/// rounds up to the smallest normal lands on the right bit pattern
/// by construction, because the exponent field sits immediately
/// above the mantissa field.
fn round_shift(value: u64, shift: u32) -> u64 {
    let q = value >> shift;
    let rem = value & ((1u64 << shift) - 1);
    let half = 1u64 << (shift - 1);
    if rem > half || (rem == half && q & 1 == 1) {
        q + 1
    } else {
        q
    }
}

/// `2^n` without `powi`, which is not const-friendly here and drags
/// in a libm symbol the AOT binary would have to resolve.
fn exp2(n: i32) -> f64 {
    f64::from_bits((((n + 1023) as u64) & 0x7FF) << 52)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(v: f64) -> f64 {
        f16_bits_to_f64(f64_to_f16_bits(v))
    }

    #[test]
    fn exact_values_survive() {
        for v in [0.0, 1.0, -1.0, 0.5, 2.0, 1024.0, 65504.0, -65504.0, 0.25] {
            assert_eq!(round_trip(v), v, "value {v}");
        }
    }

    #[test]
    fn signed_zero_keeps_its_sign() {
        assert_eq!(f64_to_f16_bits(0.0), 0x0000);
        assert_eq!(f64_to_f16_bits(-0.0), 0x8000);
        assert!(round_trip(-0.0).is_sign_negative());
    }

    #[test]
    fn ties_round_to_even() {
        // 2049 sits exactly halfway between the representable 2048
        // and 2052; ties-to-even picks 2048. 2051 is nearer 2052.
        assert_eq!(round_trip(2049.0), 2048.0);
        assert_eq!(round_trip(2051.0), 2052.0);
        assert_eq!(round_trip(2050.0), 2050.0);
        // 4097 lies between 4096 and 4100; the tie is 4098.
        assert_eq!(round_trip(4098.0), 4096.0);
    }

    #[test]
    fn overflow_and_underflow() {
        assert_eq!(round_trip(65520.0), f64::INFINITY);
        assert_eq!(round_trip(65519.0), 65504.0);
        assert_eq!(round_trip(1e30), f64::INFINITY);
        assert_eq!(round_trip(-1e30), f64::NEG_INFINITY);
        assert_eq!(round_trip(f64::INFINITY), f64::INFINITY);
        assert!(round_trip(f64::NAN).is_nan());
        // Below half the smallest subnormal rounds to zero, and the
        // sign is kept.
        assert_eq!(round_trip(1e-30), 0.0);
        assert!(round_trip(-1e-30).is_sign_negative());
    }

    #[test]
    fn subnormals() {
        let smallest_sub = 1.0 / 16777216.0; // 2^-24
        assert_eq!(round_trip(smallest_sub), smallest_sub);
        assert_eq!(f64_to_f16_bits(smallest_sub), 0x0001);
        let largest_sub = 1023.0 / 16777216.0; // 1023 * 2^-24
        assert_eq!(round_trip(largest_sub), largest_sub);
        assert_eq!(f64_to_f16_bits(largest_sub), 0x03FF);
        // The smallest NORMAL is one step above the largest subnormal.
        let smallest_normal = 1024.0 / 16777216.0; // 2^-14
        assert_eq!(f64_to_f16_bits(smallest_normal), 0x0400);
        assert_eq!(round_trip(smallest_normal), smallest_normal);
        // A subnormal that rounds up into the normal range carries
        // into the exponent field.
        let just_below = 1023.5 / 16777216.0;
        assert_eq!(f64_to_f16_bits(just_below), 0x0400);
    }

    #[test]
    fn the_classic_inexact_values() {
        // 0.1 in binary16 is 0x2E66 = 0.0999755859375.
        assert_eq!(f64_to_f16_bits(0.1), 0x2E66);
        assert_eq!(round_trip(0.1), 0.0999755859375);
        assert_eq!(f64_to_f16_bits(1.0 / 3.0), 0x3555);
    }

    #[test]
    fn exp2_matches_powi() {
        for n in -20..=20 {
            assert_eq!(exp2(n), 2f64.powi(n), "2^{n}");
        }
    }
}
