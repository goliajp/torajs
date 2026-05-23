//! `Number.prototype.toFixed / toExponential / toPrecision` —
//! fixed-point, scientific, and significant-digit formatting.
//!
//! Port of `runtime_str.c::__torajs_num_to_{fixed,exp,precision}_{i,f}`
//! + `js_normalize_exp_` (P3.2-c.3.b, 2026-05-23).
//!
//! Sub-modules:
//! - `to_fixed_*`: `%f` form with explicit half-away-from-zero
//!   pre-rounding for `digits < 16` (matches JS spec §21.1.3.4
//!   "ties broken by choosing the larger m"). Rust's
//!   `format!("{:.*}", _, _)` uses round-half-to-even (banker's)
//!   like libc's `snprintf`; pre-rounding to the exact grid point
//!   neutralizes that. For `digits >= 16` we fall through to
//!   Rust's default rounding, same wedge as C.
//! - `to_exp_*`: `%e` form via Rust `{:e}`, then exponent
//!   normalization — Rust's `{:e}` omits `'+'` for positive
//!   exponents while JS spec / libc `%e` requires it, and both
//!   sides agree on stripping leading zeros (`e+05 → e+5`).
//! - `to_precision_*`: manual `%g` — pick `%f` vs `%e` form by
//!   the actual exponent (computed from a `{:e}` pre-format to
//!   sidestep `log10` precision wobble), then strip trailing
//!   zeros from the fractional part. This preserves the C subset
//!   bit-for-bit; it diverges from JS spec which keeps the zeros
//!   to indicate precision (e.g. `(1.5).toPrecision(3) → "1.50"`
//!   per spec, but `"1.5"` here). Spec-correctness follow-up is
//!   on the backlog.
//!
//! Special values (NaN / ±Infinity) match `snprintf` output of
//! the original C subset: `"nan"`, `"inf"`, `"-inf"`. JS spec
//! would emit `"NaN"`, `"Infinity"`, `"-Infinity"` — that's the
//! same wedge as `Math.round` and is tracked in the L3b backlog.

use crate::str_bridge::alloc_str;

// ============================================================
// Helpers
// ============================================================

/// Normalize a formatted exponent. For any `'e'` followed by
/// optional sign + digits:
///
/// - if no sign follows `'e'`, insert `'+'` (Rust's `{:e}` omits
///   the `+` that JS spec / libc `%e` always emit)
/// - strip leading zeros from the digit run
/// - keep at least one digit (emit `'0'` if nothing remains)
///
/// Examples:
///   `"1.23e5"`   → `"1.23e+5"`
///   `"1.23e-05"` → `"1.23e-5"`
///   `"1e0"`      → `"1e+0"`
fn normalize_exp(src: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(src.len() + 1);
    let mut i = 0;
    while i < src.len() {
        let c = src[i];
        out.push(c);
        i += 1;
        if c == b'e' && i < src.len() {
            let sign = src[i];
            if sign == b'+' || sign == b'-' {
                out.push(sign);
                i += 1;
            } else {
                out.push(b'+');
            }
            while i < src.len() && src[i] == b'0' {
                i += 1;
            }
            if i >= src.len() || !src[i].is_ascii_digit() {
                out.push(b'0');
            }
        }
    }
    out
}

/// Strip trailing zeros from the fractional part of an
/// `INT[.FRAC][e...]` string. If the fractional part collapses
/// to empty, also strip the `'.'`.
///
/// Examples:
///   `"1234.500"`  → `"1234.5"`
///   `"1234.000"`  → `"1234"`
///   `"1.230e3"`   → `"1.23e3"`
///   `"1.000e3"`   → `"1e3"`
fn strip_trailing_zeros_in_frac(src: &[u8]) -> Vec<u8> {
    let Some(dot_pos) = src.iter().position(|&b| b == b'.') else {
        return src.to_vec();
    };
    let exp_pos = src.iter().position(|&b| b == b'e').unwrap_or(src.len());
    let mut last = exp_pos;
    while last > dot_pos + 1 && src[last - 1] == b'0' {
        last -= 1;
    }
    let keep_end = if last == dot_pos + 1 { dot_pos } else { last };
    let mut out = Vec::with_capacity(src.len());
    out.extend_from_slice(&src[..keep_end]);
    out.extend_from_slice(&src[exp_pos..]);
    out
}

/// Bit-for-bit C-subset special-value formatter shared by all
/// three families. Returns `Some(bytes)` for NaN / ±Infinity,
/// `None` for finite values.
fn special_value(n: f64) -> Option<Vec<u8>> {
    if n.is_nan() {
        return Some(b"nan".to_vec());
    }
    if n.is_infinite() {
        return Some(if n > 0.0 {
            b"inf".to_vec()
        } else {
            b"-inf".to_vec()
        });
    }
    None
}

// ============================================================
// Pure-Rust cores
// ============================================================

/// `n.toFixed(digits)` core. `digits` clamped to `[0, 20]`.
pub fn to_fixed_f(n: f64, digits: i64) -> Vec<u8> {
    if let Some(s) = special_value(n) {
        return s;
    }
    let digits = digits.clamp(0, 20);
    let value = if digits < 16 {
        let scale = 10f64.powi(digits as i32);
        (n * scale).round() / scale
    } else {
        n
    };
    format!("{:.*}", digits as usize, value).into_bytes()
}

/// `n.toFixed(digits)` core for integer receivers — delegates.
pub fn to_fixed_i(n: i64, digits: i64) -> Vec<u8> {
    to_fixed_f(n as f64, digits)
}

/// `n.toExponential(digits)` core. `digits` clamped to `[0, 100]`.
pub fn to_exp_f(n: f64, digits: i64) -> Vec<u8> {
    if let Some(s) = special_value(n) {
        return s;
    }
    let digits = digits.clamp(0, 100);
    let raw = format!("{:.*e}", digits as usize, n);
    normalize_exp(raw.as_bytes())
}

/// `n.toExponential(digits)` core for integer receivers.
pub fn to_exp_i(n: i64, digits: i64) -> Vec<u8> {
    to_exp_f(n as f64, digits)
}

/// `n.toPrecision(digits)` core. `digits <= 0` defaults to 6
/// sig figs (matches C `%g` with no precision spec); positive
/// `digits` clamped to `[1, 100]`.
pub fn to_precision_f(n: f64, digits: i64) -> Vec<u8> {
    if let Some(s) = special_value(n) {
        return s;
    }
    let precision = if digits <= 0 { 6 } else { digits.min(100) };
    let mantissa_digits = (precision - 1).max(0) as usize;
    // Compute the actual decimal exponent X by pre-formatting %e
    // and parsing the suffix — avoids f64::log10 precision wobble
    // around exact powers of 10.
    let e_form = format!("{:.*e}", mantissa_digits, n);
    let e_pos = e_form.find('e').expect("Rust {:e} always emits an 'e'");
    let exp_str = &e_form[e_pos + 1..];
    let x: i64 = exp_str.parse().unwrap_or(0);

    let formatted = if x >= -4 && x < precision {
        // %f form: precision - 1 - X digits after the decimal.
        let frac_digits = (precision - 1 - x).max(0) as usize;
        format!("{:.*}", frac_digits, n).into_bytes()
    } else {
        e_form.into_bytes()
    };
    let stripped = strip_trailing_zeros_in_frac(&formatted);
    normalize_exp(&stripped)
}

/// `n.toPrecision(digits)` core for integer receivers.
pub fn to_precision_i(n: i64, digits: i64) -> Vec<u8> {
    to_precision_f(n as f64, digits)
}

// ============================================================
// extern "C" wrappers
// ============================================================

/// `n.toFixed(digits)` for f64 receivers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_num_to_fixed_f(n: f64, digits: i64) -> *mut u8 {
    alloc_str(&to_fixed_f(n, digits))
}

/// `n.toFixed(digits)` for i64 receivers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_num_to_fixed_i(n: i64, digits: i64) -> *mut u8 {
    alloc_str(&to_fixed_i(n, digits))
}

/// `n.toExponential(digits)` for f64 receivers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_num_to_exp_f(n: f64, digits: i64) -> *mut u8 {
    alloc_str(&to_exp_f(n, digits))
}

/// `n.toExponential(digits)` for i64 receivers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_num_to_exp_i(n: i64, digits: i64) -> *mut u8 {
    alloc_str(&to_exp_i(n, digits))
}

/// `n.toPrecision(digits)` for f64 receivers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_num_to_precision_f(n: f64, digits: i64) -> *mut u8 {
    alloc_str(&to_precision_f(n, digits))
}

/// `n.toPrecision(digits)` for i64 receivers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_num_to_precision_i(n: i64, digits: i64) -> *mut u8 {
    alloc_str(&to_precision_i(n, digits))
}

// ============================================================
// Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ---- normalize_exp ----

    #[test]
    fn normalize_exp_inserts_plus() {
        assert_eq!(normalize_exp(b"1.23e5"), b"1.23e+5".to_vec());
        assert_eq!(normalize_exp(b"1e0"), b"1e+0".to_vec());
    }

    #[test]
    fn normalize_exp_keeps_minus_strips_zeros() {
        assert_eq!(normalize_exp(b"1.23e-05"), b"1.23e-5".to_vec());
        assert_eq!(normalize_exp(b"1.23e+05"), b"1.23e+5".to_vec());
    }

    #[test]
    fn normalize_exp_no_e_passthrough() {
        assert_eq!(normalize_exp(b"1234.5"), b"1234.5".to_vec());
        assert_eq!(normalize_exp(b"nan"), b"nan".to_vec());
    }

    #[test]
    fn normalize_exp_sign_then_no_digits_emits_zero() {
        // The "keep at least one digit" guard fires only when a
        // sign is present but every following digit got stripped
        // (e.g. an input like "1e+00" after strip → "1e+0").
        // Rust's `{:e}` itself never emits a trailing-empty form,
        // but `js_normalize_exp_`'s leading-zero strip can.
        assert_eq!(normalize_exp(b"1e+00"), b"1e+0".to_vec());
        assert_eq!(normalize_exp(b"1e-00"), b"1e-0".to_vec());
    }

    // ---- strip_trailing_zeros_in_frac ----

    #[test]
    fn strip_zeros_keeps_significant() {
        assert_eq!(
            strip_trailing_zeros_in_frac(b"1234.500"),
            b"1234.5".to_vec()
        );
        assert_eq!(strip_trailing_zeros_in_frac(b"1234.5"), b"1234.5".to_vec());
    }

    #[test]
    fn strip_zeros_removes_dot_when_frac_empty() {
        assert_eq!(strip_trailing_zeros_in_frac(b"1234.000"), b"1234".to_vec());
        assert_eq!(strip_trailing_zeros_in_frac(b"0.00000"), b"0".to_vec());
    }

    #[test]
    fn strip_zeros_with_exponent() {
        assert_eq!(strip_trailing_zeros_in_frac(b"1.230e3"), b"1.23e3".to_vec());
        assert_eq!(strip_trailing_zeros_in_frac(b"1.000e3"), b"1e3".to_vec());
        assert_eq!(
            strip_trailing_zeros_in_frac(b"1.234e3"),
            b"1.234e3".to_vec()
        );
    }

    #[test]
    fn strip_zeros_no_dot_passthrough() {
        assert_eq!(strip_trailing_zeros_in_frac(b"1234"), b"1234".to_vec());
        assert_eq!(strip_trailing_zeros_in_frac(b"1234e5"), b"1234e5".to_vec());
    }

    // ---- to_fixed ----

    #[test]
    fn to_fixed_basic() {
        assert_eq!(to_fixed_f(3.14, 2), b"3.14".to_vec());
        assert_eq!(to_fixed_f(3.14159, 4), b"3.1416".to_vec());
        assert_eq!(to_fixed_f(0.0, 0), b"0".to_vec());
        assert_eq!(to_fixed_f(0.0, 5), b"0.00000".to_vec());
    }

    #[test]
    fn to_fixed_half_away_from_zero() {
        // JS spec §21.1.3.4 — round half toward larger m
        // (= half-away-from-zero for positive values).
        assert_eq!(to_fixed_f(1.5, 0), b"2".to_vec());
        assert_eq!(to_fixed_f(2.5, 0), b"3".to_vec());
        assert_eq!(to_fixed_f(1234.5, 0), b"1235".to_vec());
        // Negative-half wedge: C subset preserves libc round
        // (half-away-from-zero), giving "-3" / "-1" instead of
        // JS-spec "-2" / "0".
        assert_eq!(to_fixed_f(-2.5, 0), b"-3".to_vec());
        assert_eq!(to_fixed_f(-0.5, 0), b"-1".to_vec());
    }

    #[test]
    fn to_fixed_clamps() {
        // digits < 0 → 0
        assert_eq!(to_fixed_f(3.14, -5), b"3".to_vec());
        // digits > 20 → 20
        let r = to_fixed_f(1.0, 100);
        assert_eq!(r.len(), "1.".len() + 20);
        assert!(r.starts_with(b"1."));
    }

    #[test]
    fn to_fixed_integer_path() {
        assert_eq!(to_fixed_i(42, 2), b"42.00".to_vec());
        assert_eq!(to_fixed_i(42, 0), b"42".to_vec());
        assert_eq!(to_fixed_i(-7, 3), b"-7.000".to_vec());
    }

    #[test]
    fn to_fixed_special_values() {
        assert_eq!(to_fixed_f(f64::NAN, 2), b"nan".to_vec());
        assert_eq!(to_fixed_f(f64::INFINITY, 2), b"inf".to_vec());
        assert_eq!(to_fixed_f(-f64::INFINITY, 2), b"-inf".to_vec());
    }

    // ---- to_exp ----

    #[test]
    fn to_exp_basic_positive_exp() {
        assert_eq!(to_exp_f(100.0, 0), b"1e+2".to_vec());
        assert_eq!(to_exp_f(100.0, 2), b"1.00e+2".to_vec());
        // 12345 = 1.2345e4; half-even tie at last digit picks 4
        // (even), giving "1.234e+4". Matches JS §21.1.3.3.
        assert_eq!(to_exp_f(12345.0, 3), b"1.234e+4".to_vec());
        // 12355 = 1.2355e4; half-even tie picks 6 (even) → 1.236.
        assert_eq!(to_exp_f(12355.0, 3), b"1.236e+4".to_vec());
    }

    #[test]
    fn to_exp_negative_exp() {
        assert_eq!(to_exp_f(0.001, 2), b"1.00e-3".to_vec());
        assert_eq!(to_exp_f(0.00001234, 4), b"1.2340e-5".to_vec());
    }

    #[test]
    fn to_exp_zero() {
        assert_eq!(to_exp_f(0.0, 0), b"0e+0".to_vec());
        assert_eq!(to_exp_f(0.0, 3), b"0.000e+0".to_vec());
    }

    #[test]
    fn to_exp_integer_path() {
        assert_eq!(to_exp_i(100, 0), b"1e+2".to_vec());
        assert_eq!(to_exp_i(-1000, 2), b"-1.00e+3".to_vec());
    }

    #[test]
    fn to_exp_special_values() {
        assert_eq!(to_exp_f(f64::NAN, 2), b"nan".to_vec());
        assert_eq!(to_exp_f(f64::INFINITY, 2), b"inf".to_vec());
        assert_eq!(to_exp_f(-f64::INFINITY, 2), b"-inf".to_vec());
    }

    // ---- to_precision ----

    #[test]
    fn to_precision_uses_f_form_in_range() {
        // X = 2 (since 123.456 = 1.23456e2), precision = 6,
        // 2 in [-4, 6) → %f form, 3 digits after decimal.
        assert_eq!(to_precision_f(123.456, 6), b"123.456".to_vec());
        // X = 0, precision = 3 → %f, 2 digits.
        assert_eq!(to_precision_f(1.5, 3), b"1.5".to_vec());
        // Integer-shaped: trailing zero strip drops dot too.
        assert_eq!(to_precision_f(100.0, 3), b"100".to_vec());
    }

    #[test]
    fn to_precision_uses_e_form_out_of_range() {
        // X = 6, precision = 3, 6 NOT in [-4, 3) → %e form.
        assert_eq!(to_precision_f(1234567.0, 3), b"1.23e+6".to_vec());
        // X = -5, precision = 6, -5 NOT in [-4, 6) → %e form.
        assert_eq!(to_precision_f(0.00001234, 6), b"1.234e-5".to_vec());
    }

    #[test]
    fn to_precision_default_when_zero() {
        // digits <= 0 → default precision 6 (matches C %g).
        assert_eq!(to_precision_f(123.456, 0), b"123.456".to_vec());
        assert_eq!(to_precision_f(1.5, -3), b"1.5".to_vec());
    }

    #[test]
    fn to_precision_zero_value() {
        assert_eq!(to_precision_f(0.0, 6), b"0".to_vec());
        assert_eq!(to_precision_f(0.0, 1), b"0".to_vec());
    }

    #[test]
    fn to_precision_integer_path() {
        assert_eq!(to_precision_i(100, 3), b"100".to_vec());
        assert_eq!(to_precision_i(1234567, 3), b"1.23e+6".to_vec());
    }

    #[test]
    fn to_precision_special_values() {
        assert_eq!(to_precision_f(f64::NAN, 6), b"nan".to_vec());
        assert_eq!(to_precision_f(f64::INFINITY, 6), b"inf".to_vec());
        assert_eq!(to_precision_f(-f64::INFINITY, 6), b"-inf".to_vec());
    }

    #[test]
    fn to_precision_clamps() {
        // digits > 100 → 100
        let r = to_precision_f(1.0, 200);
        // 1.0 at precision 100: %f form, 99 digits after decimal
        // → "1." + 99 zeros, strip → "1".
        assert_eq!(r, b"1".to_vec());
    }
}
