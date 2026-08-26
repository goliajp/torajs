//! f64 → shortest-roundtrip decimal string, JS spec-compliant
//! per ES §6.1.6.1.13 [ToString applied to Number].
//!
//! ## Implementation strategy
//!
//! Self-ported Ryū algorithm (Adams 2018) — see [`crate::ryu`]
//! for the kernel `d2d(bits) -> Decimal64`. The kernel produces
//! `(sign, mantissa, exponent)` such that `mantissa · 10^exponent`
//! round-trips through `str::parse::<f64>` to the input bits.
//!
//! This module owns the wrapper:
//!
//! - NaN / ±Infinity / ±0 fast-path → literal strings per §6.1.6.1.13
//!   steps 1-3 (Ryū kernel never sees these inputs).
//! - JS spec exponent boundary selection: `1e21` / `1e-7` mode flip
//!   (exponent form when `|d| < 1e-6` or `|d| >= 1e21`, fixed-point
//!   otherwise).
//! - JS spec digit shape: `1e+21` (explicit + on positive exponent),
//!   plain `100000000000000000000` for fixed-point integer, `.dddd`
//!   for fractional-only.
//! - `-0.0` → `"-0"` in this layer; `__torajs_f64_to_str` strips the
//!   sign for the `String(-0)` coercion path. `console.log(-0)` keeps
//!   the sign.

use crate::ryu::{Decimal64, d2d, decimal_length};

/// `__torajs_fmt_dtoa(d, out_buf, out_cap) -> bytes_written | -1`.
///
/// Writes the JS-spec ToString-of-Number representation of `d`
/// to `out_buf`. Returns the number of bytes written, or -1 on
/// out-of-bounds / buffer too small.
///
/// # Safety
///
/// `out_buf` must be writable for at least `out_cap` bytes.
/// `out_cap` ≥ 32 is required for any valid f64 to fit.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_fmt_dtoa(d: f64, out_buf: *mut u8, out_cap: usize) -> i32 {
    if out_buf.is_null() || out_cap < 32 {
        return -1;
    }
    // SAFETY: caller covenant — out_buf is writable for out_cap
    // bytes, which we just verified is ≥ 32.
    let buf = unsafe { core::slice::from_raw_parts_mut(out_buf, out_cap) };
    let written = format_f64_into(d, buf);
    written as i32
}

/// Pure-Rust implementation (no unsafe, no extern). Exercised by
/// the unit test suite via direct call.
fn format_f64_into(d: f64, buf: &mut [u8]) -> usize {
    // Step 1: NaN per ES §6.1.6.1.13 step 1.
    if d.is_nan() {
        return write_literal(buf, b"NaN");
    }
    // Step 2: ±0. `console.log(-0)` (Display path) preserves the
    // sign per JS spec ToString-as-Display — emit "-0" for
    // negative zero, "0" for positive. Caller `__torajs_f64_to_str`
    // (the String() coercion path per §22.1.3.6) post-strips the
    // leading "-" when needed.
    if d == 0.0 {
        if d.is_sign_negative() {
            return write_literal(buf, b"-0");
        }
        return write_literal(buf, b"0");
    }
    // Step 3: ±Infinity per step 4. Sign handled separately.
    if d.is_infinite() {
        return if d < 0.0 {
            write_literal(buf, b"-Infinity")
        } else {
            write_literal(buf, b"Infinity")
        };
    }
    // Normal finite non-zero number. JS spec §6.1.6.1.13 step
    // 5-9 picks fixed-point vs exponential based on the
    // magnitude (matching v8 / bun): exponential when
    // `|d| < 1e-6` OR `|d| >= 1e21`.
    let abs_d = if d < 0.0 { -d } else { d };
    let use_exp = abs_d < 1e-6 || abs_d >= 1e21;
    let dec = d2d(d.to_bits());
    format_decimal(buf, dec, use_exp)
}

/// Format a `Decimal64` into `buf` using JS spec digit shape.
///
/// - `use_exp = true`  → `d[.ddd...]e+NN` form (explicit sign on
///   the exponent).
/// - `use_exp = false` → plain decimal — integer with trailing
///   zeros (for positive `dec.exponent`), or `[d...].[d...]` with
///   leading-zero fill for fractional values.
///
/// The sign byte (if any) is written first; the digit count is at
/// most 17 + 1 (decimal point) + 5 (`e+NNN`) + 1 (sign) = 24 bytes
/// for any valid f64. The caller's 32-byte buffer cap is sufficient.
fn format_decimal(buf: &mut [u8], dec: Decimal64, use_exp: bool) -> usize {
    let mut pos = 0;
    if dec.sign {
        put(buf, pos, b'-');
        pos += 1;
    }

    let mantissa = dec.mantissa;
    let exponent = dec.exponent;
    let n_digits = decimal_length(mantissa) as i32;
    // True decimal exponent (position of MSB), i.e., the value such
    // that `mantissa` written with one digit before the decimal point
    // means `d.ddd…e^{abs_exp}`. abs_exp = exponent + n_digits - 1.
    let abs_exp = exponent + n_digits - 1;

    if use_exp {
        // Exponential form: `d[.ddd...]e±NN`.
        //
        // Emit digits at buf[digit_start..digit_end], then if more
        // than one digit, shift digits[1..] right by 1 and insert '.'.
        let digit_start = pos;
        pos = write_digits(buf, pos, mantissa, n_digits as usize);
        if n_digits > 1 {
            // Shift digits after digit_start by 1 to make room for '.'.
            for i in (digit_start + 2..=pos).rev() {
                put(buf, i, at(buf, i - 1));
            }
            put(buf, digit_start + 1, b'.');
            pos += 1;
        }
        // 'e' + explicit sign + exponent digits.
        put(buf, pos, b'e');
        pos += 1;
        if abs_exp >= 0 {
            put(buf, pos, b'+');
            pos += 1;
        } else {
            put(buf, pos, b'-');
            pos += 1;
        }
        let abs_e_abs = abs_exp.unsigned_abs();
        pos = write_u32(buf, pos, abs_e_abs);
        pos
    } else if exponent >= 0 {
        // Integer with trailing zeros (e.g., 12340000…).
        // Plain decimal form, mantissa followed by `exponent` zeros.
        pos = write_digits(buf, pos, mantissa, n_digits as usize);
        for _ in 0..exponent {
            put(buf, pos, b'0');
            pos += 1;
        }
        pos
    } else {
        // Fractional value. `dot_pos` is the number of digits before
        // the decimal point (0 if the first digit is fractional).
        let dot_pos = n_digits + exponent;
        if dot_pos <= 0 {
            // `0.` followed by `(-dot_pos)` leading zeros, then all
            // mantissa digits.
            put(buf, pos, b'0');
            put(buf, pos + 1, b'.');
            pos += 2;
            for _ in 0..(-dot_pos) {
                put(buf, pos, b'0');
                pos += 1;
            }
            pos = write_digits(buf, pos, mantissa, n_digits as usize);
            pos
        } else {
            // `digits[..dot_pos]` `.` `digits[dot_pos..]`.
            // Emit all digits, then shift digits[dot_pos..] right by
            // 1 and insert '.' at dot_pos.
            let digit_start = pos;
            pos = write_digits(buf, pos, mantissa, n_digits as usize);
            let dot_pos_u = dot_pos as usize;
            for i in (digit_start + dot_pos_u + 1..=pos).rev() {
                put(buf, i, at(buf, i - 1));
            }
            put(buf, digit_start + dot_pos_u, b'.');
            pos += 1;
            pos
        }
    }
}

/// Write `v` as `n_digits` ASCII characters into `buf[pos..]`,
/// most-significant first. Returns `pos + n_digits`.
fn write_digits(buf: &mut [u8], pos: usize, mut v: u64, n_digits: usize) -> usize {
    for i in (0..n_digits).rev() {
        put(buf, pos + i, b'0' + (v % 10) as u8);
        v /= 10;
    }
    pos + n_digits
}

/// Write `v` as a decimal ASCII string into `buf[pos..]`,
/// most-significant first, no leading zeros. Returns new pos.
fn write_u32(buf: &mut [u8], pos: usize, v: u32) -> usize {
    if v == 0 {
        put(buf, pos, b'0');
        return pos + 1;
    }
    let mut n = 0usize;
    let mut tmp = v;
    while tmp > 0 {
        n += 1;
        tmp /= 10;
    }
    let mut v = v;
    for i in (0..n).rev() {
        put(buf, pos + i, b'0' + (v % 10) as u8);
        v /= 10;
    }
    pos + n
}

#[inline]
fn write_literal(buf: &mut [u8], lit: &[u8]) -> usize {
    let n = lit.len();
    if n > buf.len() {
        return 0;
    }
    for (dst, &b) in buf.iter_mut().zip(lit) {
        *dst = b;
    }
    n
}

/// r505 — every write into the caller's buffer goes through these
/// two, never `buf[i]`: the algorithm's longest line is 24 bytes and
/// the entry rejects a buffer under 32, so an out-of-range index is
/// unreachable — but not provably so to the compiler, and each `[]`
/// carried a bounds-check panic whose formatting path dragged
/// `core::fmt` (pad_integral, Display for usize, …) into every
/// program that prints a float (the r505 loop-accumulator census).
/// A miss drops the byte; loud nowhere is the honest price of the
/// renderer's absence (same posture as torajs-io's line buffer).
#[inline]
fn put(buf: &mut [u8], i: usize, b: u8) {
    if let Some(slot) = buf.get_mut(i) {
        *slot = b;
    }
}

#[inline]
fn at(buf: &[u8], i: usize) -> u8 {
    buf.get(i).copied().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    extern crate alloc;
    use alloc::string::{String, ToString as _};

    fn call(d: f64) -> String {
        let mut buf = [0u8; 32];
        let n = unsafe { __torajs_fmt_dtoa(d, buf.as_mut_ptr(), buf.len()) };
        assert!(n > 0, "dtoa returned non-positive {n} for {d}");
        let n = n as usize;
        core::str::from_utf8(&buf[..n])
            .expect("ascii output")
            .to_string()
    }

    fn check(d: f64, expected: &str) {
        let got = call(d);
        assert_eq!(got.as_str(), expected, "for input {d}");
    }

    /// JS spec edge cases per §6.1.6.1.13.
    #[test]
    fn nan() {
        check(f64::NAN, "NaN");
    }

    #[test]
    fn positive_zero() {
        check(0.0, "0");
    }

    #[test]
    fn negative_zero() {
        // dtoa preserves the sign — `__torajs_f64_to_str` strips
        // for String() coercion, but `console.log(-0)` shows "-0".
        check(-0.0, "-0");
    }

    #[test]
    fn positive_infinity() {
        check(f64::INFINITY, "Infinity");
    }

    #[test]
    fn negative_infinity() {
        check(f64::NEG_INFINITY, "-Infinity");
    }

    /// Basic finite values.
    #[test]
    fn one() {
        check(1.0, "1");
    }

    #[test]
    fn negative_one() {
        check(-1.0, "-1");
    }

    #[test]
    fn three_point_one_four() {
        check(3.14, "3.14");
    }

    /// JS spec exponent form for positive exponents uses `e+`.
    #[test]
    fn exponent_positive_sign() {
        check(1e21, "1e+21");
    }

    /// Negative exponent keeps its '-' (no double sign).
    #[test]
    fn exponent_negative_sign() {
        check(1e-7, "1e-7");
    }

    /// `1e-6` boundary — JS spec uses fixed-point at the boundary.
    #[test]
    fn boundary_1e_minus_6() {
        check(1e-6, "0.000001");
    }

    /// `1e20` boundary — JS spec uses fixed-point at the boundary.
    #[test]
    fn boundary_1e20() {
        check(1e20, "100000000000000000000");
    }

    /// 0.1 + 0.2 round-trip (the classic IEEE 754 demonstration).
    #[test]
    fn zero_one_plus_zero_two() {
        check(0.1 + 0.2, "0.30000000000000004");
    }

    #[test]
    fn pi() {
        check(core::f64::consts::PI, "3.141592653589793");
    }

    /// Round-trip property: every f64 in our sample set must
    /// parse back to the original bits. Covers normal range,
    /// subnormals, and the boundary cases.
    #[test]
    fn round_trip_samples() {
        let samples: &[f64] = &[
            1.0,
            -1.0,
            2.0,
            -2.0,
            0.1,
            0.2,
            0.3,
            -0.5,
            3.14159265358979323846,
            2.718281828459045,
            f64::MIN_POSITIVE,
            f64::EPSILON,
            f64::MAX,
            f64::MIN,
            42.0,
            -42.0,
            100.0,
            1000.0,
            10000.0,
            100000.0,
            1_000_000.0,
            1e15,
            1e16,
            1e20,
            1e21,
            1e22,
            1e-5,
            1e-10,
            1e-300,
            1e308,
            -1e308,
            123.456,
            -987.654,
            0.0001,
            0.00001,
            7.0,
            // Round-edge from the audit doc.
            (i64::MAX as f64) + 1.0,
        ];
        for &d in samples {
            let s = call(d);
            let parsed: f64 = s.parse().unwrap_or_else(|_| {
                panic!("parse failed for {s:?} (input was {d})");
            });
            assert_eq!(
                parsed.to_bits(),
                d.to_bits(),
                "round-trip mismatch for {d}: got {s:?} parsed back to {parsed}",
            );
        }
    }

    /// Buffer too small returns -1.
    #[test]
    fn buf_too_small() {
        let mut buf = [0u8; 8]; // less than the 32-byte minimum
        let n = unsafe { __torajs_fmt_dtoa(3.14, buf.as_mut_ptr(), buf.len()) };
        assert_eq!(n, -1, "tiny buffer must return -1");
    }
}
