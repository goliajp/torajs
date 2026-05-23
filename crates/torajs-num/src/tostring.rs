//! `Number.prototype.toString(radix)` — radix-aware stringify for
//! both i64 and f64 receivers.
//!
//! Port of `runtime_str.c::__torajs_num_to_string_radix_{i,f}`
//! (P3.2-c.3.a, 2026-05-23). The f-version handles NaN / Infinity
//! / -Infinity sentinels + integer-valued shortcut + fractional
//! digit loop. The i-version is straightforward radix encoding via
//! the canonical "divide-by-radix push-digits" pattern.
//!
//! Both allocate via [`crate::str_bridge::alloc_str`] (a thin
//! wrapper that re-exports [`__torajs_str_alloc_pooled`] across
//! the Layer-2 same-tier boundary).

use crate::str_bridge::alloc_str;

/// Base-36 digit charset shared by both i + f paths.
const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";

// ============================================================
// Pure-Rust cores
// ============================================================

/// Encode `n` in `radix` (clamped to `2..=36`). Returns a `Vec<u8>`
/// suitable for handing off to [`alloc_str`].
pub fn to_string_radix_i(n: i64, radix: i64) -> Vec<u8> {
    let radix = radix.clamp(2, 36) as u64;
    let (neg, mut u) = if n < 0 {
        // Two's-complement abs that handles i64::MIN cleanly.
        (true, (n as i128).unsigned_abs() as u64)
    } else {
        (false, n as u64)
    };
    let mut buf = [0u8; 80];
    let mut i = buf.len();
    if u == 0 {
        i -= 1;
        buf[i] = b'0';
    } else {
        while u > 0 {
            i -= 1;
            buf[i] = DIGITS[(u % radix) as usize];
            u /= radix;
        }
    }
    if neg {
        i -= 1;
        buf[i] = b'-';
    }
    buf[i..].to_vec()
}

/// Encode finite non-integer `d` in `radix` (clamped to `2..=36`).
/// NaN / Infinity / -Infinity routed to canonical sentinel strings.
/// Integer-valued doubles route to [`to_string_radix_i`].
/// Otherwise: integer part via `to_string_radix_i`, then a multiply
/// + extract + subtract loop for fractional digits (capped at 52,
/// the f64 mantissa bit count = worst-case radix-2 digit budget).
pub fn to_string_radix_f(d: f64, radix: i64) -> Vec<u8> {
    let radix = radix.clamp(2, 36);
    if d.is_nan() {
        return b"NaN".to_vec();
    }
    if d.is_infinite() {
        return if d > 0.0 {
            b"Infinity".to_vec()
        } else {
            b"-Infinity".to_vec()
        };
    }
    // Integer-valued + representable as i64 → straight to _i path.
    if d == d.floor() && d >= i64::MIN as f64 && d <= i64::MAX as f64 {
        return to_string_radix_i(d as i64, radix);
    }
    let neg = d < 0.0;
    let abs_d = if neg { -d } else { d };
    let int_part = abs_d.floor();
    let mut frac = abs_d - int_part;

    let int_bytes = to_string_radix_i(int_part as i64, radix);
    let mut frac_buf = Vec::with_capacity(52);
    let r_d = radix as f64;
    let radix_u = radix as usize;
    while frac > 0.0 && frac_buf.len() < 52 {
        frac *= r_d;
        let digit_d = frac.floor();
        let digit = (digit_d as usize).min(radix_u - 1);
        frac_buf.push(DIGITS[digit]);
        frac -= digit_d;
    }

    let mut out = Vec::with_capacity(int_bytes.len() + 1 + frac_buf.len() + 1);
    if neg {
        out.push(b'-');
    }
    out.extend_from_slice(&int_bytes);
    if !frac_buf.is_empty() {
        out.push(b'.');
        out.extend_from_slice(&frac_buf);
    }
    out
}

// ============================================================
// extern "C" wrappers
// ============================================================

/// `n.toString(radix)` for i64 receivers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_num_to_string_radix_i(n: i64, radix: i64) -> *mut u8 {
    let bytes = to_string_radix_i(n, radix);
    alloc_str(&bytes)
}

/// `n.toString(radix)` for f64 receivers. NaN / ±Infinity preserved.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_num_to_string_radix_f(d: f64, radix: i64) -> *mut u8 {
    let bytes = to_string_radix_f(d, radix);
    alloc_str(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn i_basic_decimal() {
        assert_eq!(to_string_radix_i(0, 10), b"0".to_vec());
        assert_eq!(to_string_radix_i(42, 10), b"42".to_vec());
        assert_eq!(to_string_radix_i(-42, 10), b"-42".to_vec());
        assert_eq!(to_string_radix_i(1000000, 10), b"1000000".to_vec());
    }

    #[test]
    fn i_binary_hex() {
        assert_eq!(to_string_radix_i(10, 2), b"1010".to_vec());
        assert_eq!(to_string_radix_i(255, 16), b"ff".to_vec());
        assert_eq!(to_string_radix_i(35, 36), b"z".to_vec());
        assert_eq!(to_string_radix_i(-255, 16), b"-ff".to_vec());
    }

    #[test]
    fn i_radix_clamp() {
        // radix < 2 → 2; radix > 36 → 36
        assert_eq!(to_string_radix_i(2, 1), b"10".to_vec());
        assert_eq!(to_string_radix_i(35, 100), b"z".to_vec());
    }

    #[test]
    fn i_int_min_edge() {
        // i64::MIN — two's complement abs would overflow signed neg,
        // we use i128 widen for unsigned_abs. Result is i64::MIN's
        // base-10 representation.
        assert_eq!(
            to_string_radix_i(i64::MIN, 10),
            b"-9223372036854775808".to_vec()
        );
    }

    #[test]
    fn f_special_values() {
        assert_eq!(to_string_radix_f(f64::NAN, 10), b"NaN".to_vec());
        assert_eq!(to_string_radix_f(f64::INFINITY, 10), b"Infinity".to_vec());
        assert_eq!(to_string_radix_f(-f64::INFINITY, 10), b"-Infinity".to_vec());
    }

    #[test]
    fn f_integer_shortcuts_to_i() {
        assert_eq!(to_string_radix_f(0.0, 10), b"0".to_vec());
        assert_eq!(to_string_radix_f(42.0, 10), b"42".to_vec());
        assert_eq!(to_string_radix_f(-42.0, 10), b"-42".to_vec());
        assert_eq!(to_string_radix_f(255.0, 16), b"ff".to_vec());
    }

    #[test]
    fn f_fractional_basic() {
        // 0.5 in binary = 0.1
        assert_eq!(to_string_radix_f(0.5, 2), b"0.1".to_vec());
        // 0.25 in binary = 0.01
        assert_eq!(to_string_radix_f(0.25, 2), b"0.01".to_vec());
        // 0.5 in base-10
        assert_eq!(to_string_radix_f(0.5, 10), b"0.5".to_vec());
        // -1.5 in base-10
        assert_eq!(to_string_radix_f(-1.5, 10), b"-1.5".to_vec());
    }
}
