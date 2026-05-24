//! Spec-compliance edge-case tests against ECMAScript §19 / §20.
//! Picks the corners where libm semantics diverge from ECMAScript so
//! a future polish that swaps the implementation backend doesn't
//! silently regress.

use torajs_num::parse::{parse_float, parse_int};

unsafe extern "C" {
    fn __torajs_math_round(x: f64) -> f64;
    fn __torajs_math_max(a: f64, b: f64) -> f64;
    fn __torajs_math_min(a: f64, b: f64) -> f64;
    fn __torajs_num_is_safe_integer_f(n: f64) -> i64;
    fn __torajs_num_is_safe_integer_i(n: i64) -> i64;
    fn __torajs_num_is_integer_f(n: f64) -> i64;
}

#[test]
fn math_round_half_away_from_zero_per_es_20_2_2_28() {
    // ES §20.2.2.28: Math.round returns the integer nearest, with
    // ties rounding TOWARD positive infinity (half-up for positive).
    // This is NOT libc round's tie-to-even behavior.
    unsafe {
        assert_eq!(__torajs_math_round(0.5), 1.0); // half → up
        assert_eq!(__torajs_math_round(1.5), 2.0); // tie at 1.5
        assert_eq!(__torajs_math_round(2.5), 3.0); // tie at 2.5
        assert_eq!(__torajs_math_round(-0.5), 0.0); // -0.5 toward +∞ = 0
        assert_eq!(__torajs_math_round(-1.5), -1.0); // -1.5 toward +∞ = -1
        assert_eq!(__torajs_math_round(0.0), 0.0);
        assert_eq!(__torajs_math_round(-0.0), -0.0);
    }
}

#[test]
fn math_max_min_propagate_nan_per_es_20_2_2_24() {
    // ES §20.2.2.24 / §20.2.2.25: if any arg is NaN, return NaN.
    // (Some libm implementations return the non-NaN operand.)
    unsafe {
        assert!(__torajs_math_max(f64::NAN, 1.0).is_nan());
        assert!(__torajs_math_max(1.0, f64::NAN).is_nan());
        assert!(__torajs_math_min(f64::NAN, 1.0).is_nan());
        assert!(__torajs_math_min(1.0, f64::NAN).is_nan());
        // Both NaN → NaN.
        assert!(__torajs_math_max(f64::NAN, f64::NAN).is_nan());
    }
}

#[test]
fn parse_int_handles_0x_prefix() {
    // ES §19.2.5: parseInt with radix 16 (or 0) accepts a leading
    // `0x` / `0X` prefix and skips it.
    assert_eq!(parse_int(b"0x10", 16), 16.0);
    assert_eq!(parse_int(b"0X10", 16), 16.0);
    assert_eq!(parse_int(b"0xff", 16), 255.0);
    // With radix 10, the `0` is consumed then `x` is junk → 0.
    assert_eq!(parse_int(b"0x10", 10), 0.0);
}

#[test]
fn parse_int_handles_leading_whitespace_and_sign() {
    assert_eq!(parse_int(b"   42", 10), 42.0);
    assert_eq!(parse_int(b"  -7", 10), -7.0);
    assert_eq!(parse_int(b"  +7", 10), 7.0);
    // Trailing junk: parse what's parseable, stop at first non-digit.
    assert_eq!(parse_int(b"42abc", 10), 42.0);
}

#[test]
fn parse_int_returns_nan_on_empty_or_non_numeric() {
    assert!(parse_int(b"", 10).is_nan());
    assert!(parse_int(b"abc", 10).is_nan());
    assert!(parse_int(b"  ", 10).is_nan());
}

#[test]
fn parse_float_accepts_infinity_and_signed() {
    assert_eq!(parse_float(b"Infinity"), f64::INFINITY);
    assert_eq!(parse_float(b"-Infinity"), f64::NEG_INFINITY);
    assert_eq!(parse_float(b"+Infinity"), f64::INFINITY);
    assert_eq!(parse_float(b"1.5e3"), 1500.0);
    assert_eq!(parse_float(b"1e-3"), 0.001);
}

#[test]
fn parse_float_returns_nan_on_garbage() {
    assert!(parse_float(b"").is_nan());
    assert!(parse_float(b"abc").is_nan());
    // Just a sign without digits.
    assert!(parse_float(b"+").is_nan());
    assert!(parse_float(b"-").is_nan());
}

#[test]
fn is_safe_integer_boundary_at_2_pow_53_minus_1() {
    // ES §20.1.2.5: Number.MAX_SAFE_INTEGER = 2^53 - 1 =
    // 9_007_199_254_740_991. Anything beyond that is unsafe even
    // if it looks like an integer.
    const MAX_SAFE_F: f64 = 9_007_199_254_740_991.0;
    const MAX_SAFE_I: i64 = 9_007_199_254_740_991;
    unsafe {
        assert_eq!(__torajs_num_is_safe_integer_f(MAX_SAFE_F), 1);
        assert_eq!(__torajs_num_is_safe_integer_f(MAX_SAFE_F + 1.0), 0);
        assert_eq!(__torajs_num_is_safe_integer_f(-MAX_SAFE_F), 1);
        assert_eq!(__torajs_num_is_safe_integer_i(MAX_SAFE_I), 1);
        assert_eq!(__torajs_num_is_safe_integer_i(MAX_SAFE_I + 1), 0);
        assert_eq!(__torajs_num_is_safe_integer_i(-MAX_SAFE_I), 1);
    }
}

#[test]
fn is_integer_nan_infinity_are_not_integers() {
    // ES §20.1.2.3: Number.isInteger returns false for NaN /
    // Infinity / non-finite.
    unsafe {
        assert_eq!(__torajs_num_is_integer_f(f64::NAN), 0);
        assert_eq!(__torajs_num_is_integer_f(f64::INFINITY), 0);
        assert_eq!(__torajs_num_is_integer_f(f64::NEG_INFINITY), 0);
        assert_eq!(__torajs_num_is_integer_f(1.5), 0);
        assert_eq!(__torajs_num_is_integer_f(1.0), 1);
        assert_eq!(__torajs_num_is_integer_f(-2.0), 1);
    }
}
