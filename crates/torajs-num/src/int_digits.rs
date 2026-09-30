//! The Number nearest the integer a digit string spells, once the
//! digits no longer fit a u64: one rounding from the exact value for
//! radix 10 and the powers of two, a Horner approximation for the
//! other radices.
//!
//! Radix 10 keeps the whole integer in a small array of 32-bit limbs
//! (309 digits is where f64 ends, so 34 limbs suffice) and rounds it
//! the way a power-of-two string is rounded: the leading 64 bits with
//! everything below folded into a sticky bit convert to f64 in one
//! nearest-even rounding, then exact doublings put the exponent back.
//! Nothing here reaches core's decimal-to-float tables, which would
//! otherwise ride into every program that calls `parseInt`.

use crate::parse::digit_value;

/// `digits` spell an integer too large for the caller's u64 walk.
pub(crate) fn value_past_u64(digits: &[u8], radix: u32) -> f64 {
    if radix == 10 {
        decimal(digits)
    } else if radix.is_power_of_two() {
        pow2(digits, radix.trailing_zeros())
    } else {
        digits.iter().fold(0.0f64, |v, &c| {
            v * radix as f64 + digit_value(c).unwrap_or(0) as f64
        })
    }
}

/// past this many significant digits the value is past f64
const MAX_DECIMAL_DIGITS: usize = 309;
const LIMBS: usize = 34;

fn decimal(digits: &[u8]) -> f64 {
    let first = digits
        .iter()
        .position(|&c| c != b'0')
        .unwrap_or(digits.len());
    let digits = &digits[first..];
    if digits.len() > MAX_DECIMAL_DIGITS {
        return f64::INFINITY;
    }
    let mut limbs = [0u32; LIMBS];
    let mut n = 0usize;
    for &c in digits {
        let mut carry = digit_value(c).unwrap_or(0) as u64;
        for limb in limbs.iter_mut().take(n) {
            let t = *limb as u64 * 10 + carry;
            *limb = t as u32;
            carry = t >> 32;
        }
        if carry != 0 {
            limbs[n] = carry as u32;
            n += 1;
        }
    }
    let bits = match (0..n).rev().find(|&k| limbs[k] != 0) {
        Some(top) => top as u32 * 32 + (32 - limbs[top].leading_zeros()),
        None => return 0.0,
    };
    let bit = |k: u32| (limbs[(k / 32) as usize] >> (k % 32)) & 1;
    if bits <= 64 {
        return (0..bits)
            .rev()
            .fold(0u64, |lead, k| (lead << 1) | bit(k) as u64) as f64;
    }
    let dropped = bits - 64;
    let lead = (dropped..bits)
        .rev()
        .fold(0u64, |lead, k| (lead << 1) | bit(k) as u64);
    let sticky = (0..dropped).any(|k| bit(k) != 0);
    scale(lead, sticky, dropped)
}

/// Digits of a power-of-two radix are a bit string: keep its leading
/// 64 bits, fold everything below into a sticky bit, scale by the bits
/// that fell off.
fn pow2(digits: &[u8], bits_per_digit: u32) -> f64 {
    let mut lead = 0u64;
    let mut dropped = 0u32;
    let mut sticky = false;
    for &c in digits {
        let d = digit_value(c).unwrap_or(0);
        for bit in (0..bits_per_digit).rev() {
            let b = (d >> bit) & 1;
            if lead >> 63 == 0 {
                lead = (lead << 1) | b as u64;
            } else {
                dropped += 1;
                sticky |= b != 0;
            }
        }
    }
    scale(lead, sticky, dropped)
}

/// `lead` holds the leading bits with the rest summarized by `sticky`;
/// one conversion rounds to nearest-even on the exact value, and the
/// doublings are exact (a value past f64's range ends at infinity).
fn scale(lead: u64, sticky: bool, mut dropped: u32) -> f64 {
    let mut v = (lead | sticky as u64) as f64;
    while dropped > 0 {
        let step = dropped.min(1000);
        v *= f64::from_bits(((1023 + step) as u64) << 52);
        dropped -= step;
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_rounds_once_like_core() {
        // a deterministic spread of lengths and digit patterns, checked against core's
        // correctly rounded parser; the crate's runtime path never links it
        let mut x = 0x9E3779B97F4A7C15u64;
        for len in [17usize, 18, 19, 20, 25, 40, 100, 200, 308, 309] {
            for _ in 0..40 {
                let mut s = Vec::with_capacity(len);
                for i in 0..len {
                    x ^= x << 13;
                    x ^= x >> 7;
                    x ^= x << 17;
                    let d = if i == 0 { 1 + (x % 9) } else { x % 10 };
                    s.push(b'0' + d as u8);
                }
                let want: f64 = core::str::from_utf8(&s).unwrap().parse().unwrap();
                assert_eq!(decimal(&s), want, "{}", core::str::from_utf8(&s).unwrap());
            }
        }
    }

    #[test]
    fn decimal_edges() {
        assert_eq!(decimal(b"00000000000000000000000000000000000000042"), 42.0);
        assert_eq!(decimal(b"18446744073709551616"), 18446744073709551616.0);
        assert_eq!(decimal(b"18446744073709551617"), 18446744073709551616.0);
        let max = "17976931348623157".to_string() + &"0".repeat(292);
        assert_eq!(decimal(max.as_bytes()), f64::MAX);
        let past = "17976931348623159".to_string() + &"0".repeat(292);
        assert_eq!(decimal(past.as_bytes()), f64::INFINITY);
        let long = "1".to_string() + &"0".repeat(309);
        assert_eq!(decimal(long.as_bytes()), f64::INFINITY);
        assert_eq!(decimal(&[b'0'; 400]), 0.0);
    }
}
