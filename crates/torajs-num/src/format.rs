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
//!   sidestep `log10` precision wobble). Per ES §22.1.3.36, emit
//!   exactly `precision` significant digits including trailing
//!   zeros — so `(1.5).toPrecision(3) → "1.50"`,
//!   `(0).toPrecision(3) → "0.00"`, `(1e-7).toPrecision(2) →
//!   "1.0e-7"`. (Pre-P12.3 a `strip_trailing_zeros_in_frac` post-
//!   process broke that — removed in P12.3.)
//!
//! Special values (NaN / ±Infinity) follow ES spec — `"NaN"`,
//! `"Infinity"`, `"-Infinity"` — for `toFixed` / `toExponential` /
//! `toPrecision` / `toLocaleString` (matches bun + V8 + SpiderMonkey
//! for the first three; the S167 fix drove these away from the
//! original C `snprintf` subset). The `toLocaleString` en-US
//! Unicode `"∞"` / `"-∞"` form is deferred to L3b until the Str
//! alloc surface separates byte-count from code-unit-count (multi-
//! byte UTF-8 currently trips Str.len's code-unit interpretation).

use crate::str_bridge::alloc_str;

unsafe extern "C" {
    /// Cross-tier — torajs-throw. Records a pending RangeError via
    /// TLS; returns normally so the caller's `emit_throw_check`
    /// after the call site propagates the throw.
    fn __torajs_throw_range_error(msg: *const u8);
}

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

/// ES-spec special-value formatter shared by `toFixed` /
/// `toExponential` / `toPrecision` / `toLocaleString`. Per ES
/// §6.1.6.1.13 Number::toString (whose conventions these inherit):
/// NaN → `"NaN"`, ±Infinity → `"Infinity"` / `"-Infinity"`. Returns
/// `Some(bytes)` for NaN / ±Infinity, `None` for finite values.
///
/// (`toLocaleString` en-US would prefer the Unicode `"∞"` symbol per
/// CLDR root, but the multi-byte UTF-8 payload trips alloc_str's
/// byte-count == code-unit-count assumption — `tora`'s Str layout
/// stores a code-unit count, so 3-byte `"∞"` gets length 3 and the
/// print path re-encodes each byte as Latin-1. Tracked in L3b until
/// the Str alloc surface separates byte-count from code-unit-count.)
fn special_value(n: f64) -> Option<Vec<u8>> {
    if n.is_nan() {
        return Some(b"NaN".to_vec());
    }
    if n.is_infinite() {
        return Some(if n > 0.0 {
            b"Infinity".to_vec()
        } else {
            b"-Infinity".to_vec()
        });
    }
    None
}

// ============================================================
// Pure-Rust cores
// ============================================================

/// `n.toFixed(digits)` core. `digits` clamped to `[0, 20]`.
///
/// Per ES §22.1.3.32: when `|n| >= 10^21`, return `ToString(n)` —
/// i.e., the same Ryū-based shortest-roundtrip output used by
/// `Number.prototype.toString` / `String(n)` (which for these
/// magnitudes is the JS-spec exponent form like `"1e+21"`).
pub fn to_fixed_f(n: f64, digits: i64) -> Vec<u8> {
    if let Some(s) = special_value(n) {
        return s;
    }
    // ES §21.1.3.3 step 7: `if x < 0` — IEEE 754 `-0` is not `< 0`,
    // so the sign string stays empty and the output starts with
    // "0". Rust `format!("{:.N}", -0.0)` preserves the IEEE 754
    // sign bit and emits "-0.0..." → normalise -0 to +0 at entry,
    // mirroring `to_exp_f`'s line 157 wedge.
    let n = if n == 0.0 { 0.0 } else { n };
    if n.abs() >= 1e21 {
        // Spec ToString(n) form = Ryū exp shape (e.g. "1e+21").
        // `format!("{:e}", n)` is shortest-roundtrip via Rust core::fmt;
        // normalize_exp brings the exponent shape into JS spec form
        // (insert '+' on positive exponents, strip leading zeros).
        return normalize_exp(format!("{:e}", n).as_bytes());
    }
    // Spec range is `[0, 100]`. The extern wrappers gate
    // out-of-range as RangeError before reaching here; this
    // `clamp` defends the pure-Rust callers that go through
    // `to_fixed_f` directly (e.g. unit tests).
    let digits = digits.clamp(0, 100);
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

/// `n.toExponential(digits)` core. `digits` clamped to `[0, 100]`,
/// or `digits < 0` selects the shortest-roundtrip form (ES
/// §22.1.3.5 — fractionDigits undefined → spec defers to Number's
/// shortest representation). Rust `format!("{:e}", _)` is ryu-shortest;
/// `normalize_exp` then aligns the exponent shape to JS spec
/// (insert '+' on positive, strip leading zeros).
pub fn to_exp_f(n: f64, digits: i64) -> Vec<u8> {
    if let Some(s) = special_value(n) {
        return s;
    }
    // ES §22.1.3.5 — `(-0).toExponential(...)` emits "0e+0"
    // (positive sign). Rust `format!("{:e}", -0.0)` preserves the
    // IEEE 754 sign bit and renders "-0e0". Normalise -0 to +0 at
    // entry; same shape as the `Math.round` signed-zero edge.
    let n = if n == 0.0 { 0.0 } else { n };
    if digits < 0 {
        return normalize_exp(format!("{:e}", n).as_bytes());
    }
    let digits = digits.clamp(0, 100);
    // Pre-round in mantissa basis half-away-from-zero (matches `to_fixed_f`
    // lines 116-121 wedge for %f form). Rust `{:.*e}` rounds half-to-even
    // (banker), but ES §22.1.3.5 ties go to the larger m — same fix shape
    // `to_fixed_f` / `to_precision_f` %f arm already neutralize. Without
    // this `(1.25).toExponential(1)` was "1.2e+0" (banker, 2 even) instead
    // of the spec "1.3e+0".
    let value = round_e_form(n, digits as usize);
    let raw = format!("{:.*e}", digits as usize, value);
    normalize_exp(raw.as_bytes())
}

/// Pre-round `n` in mantissa basis so a subsequent `format!("{:.*e}", _, _)`
/// produces a half-away-from-zero rounded mantissa (spec) instead of Rust's
/// default half-to-even (banker). `mantissa_digits` is the number of frac
/// digits kept after the decimal point in the mantissa (e.g. `precision - 1`).
///
/// Mechanism: format `n` once with `{:e}` to recover the decimal exponent
/// `x`, then scale to `mantissa_digits - x` decimal grid and `.round()`
/// (Rust `f64::round` is half-away-from-zero). For `|scale_pow|` outside
/// the f64 exact-power-of-ten range, fall through to raw — same guard
/// shape as `to_fixed_f` line 116 / `to_precision_f` line 187.
fn round_e_form(n: f64, mantissa_digits: usize) -> f64 {
    if !n.is_finite() || n == 0.0 {
        return n;
    }
    let raw = format!("{n:e}");
    // Rust `{:e}` always emits an 'e'; unchecked keeps the panic path out
    // of the user binary (mirrors `to_precision_f` line 173 polish A3).
    let e_pos = unsafe { raw.find('e').unwrap_unchecked() };
    let x: i32 = raw[e_pos + 1..].parse().unwrap_or(0);
    let scale_pow = mantissa_digits as i32 - x;
    if (-15..=15).contains(&scale_pow) {
        let scale = 10f64.powi(scale_pow);
        (n * scale).round() / scale
    } else {
        n
    }
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
    // ES §21.1.3.5: `(-0).toPrecision(N)` emits "0[.0...0]" — same
    // signed-zero normalisation as `to_fixed_f` / `to_exp_f`. Rust's
    // formatters preserve the IEEE 754 sign bit otherwise.
    let n = if n == 0.0 { 0.0 } else { n };
    let precision = if digits <= 0 { 6 } else { digits.min(100) };
    let mantissa_digits = (precision - 1).max(0) as usize;
    // Compute the actual decimal exponent X by pre-formatting %e
    // and parsing the suffix — avoids f64::log10 precision wobble
    // around exact powers of 10.
    let e_form = format!("{:.*e}", mantissa_digits, n);
    // Rust {:.N e} always emits an 'e'; use unsafe-unchecked to keep
    // the panic path out of the user binary (polish A3).
    let e_pos = unsafe { e_form.find('e').unwrap_unchecked() };
    let exp_str = &e_form[e_pos + 1..];
    let x: i64 = exp_str.parse().unwrap_or(0);

    let formatted = if x >= -4 && x < precision {
        // %f form: precision - 1 - X digits after the decimal.
        // Pre-round half-away-from-zero (matches `to_fixed_f` lines
        // 116-121): Rust `format!("{:.*}", _, _)` rounds half-to-even
        // (banker's), but ES §22.1.3.36 ties go to the larger m —
        // same wedge `to_fixed_f` already neutralizes. Without this
        // `(1234.5).toPrecision(4)` was "1234" (banker, 4 even)
        // instead of the spec "1235". Guarded `frac_digits < 16` for
        // the same overflow reason as `to_fixed_f`.
        let frac_digits = (precision - 1 - x).max(0) as usize;
        let value = if frac_digits < 16 {
            let scale = 10f64.powi(frac_digits as i32);
            (n * scale).round() / scale
        } else {
            n
        };
        format!("{:.*}", frac_digits, value).into_bytes()
    } else {
        // %e form: mirror the %f arm's pre-round (lines 187-192) in
        // mantissa basis. Without this `(125).toPrecision(2)` was
        // "1.2e+2" (Rust banker, mantissa 1.25 → 2 even) instead of the
        // spec "1.3e+2". Same `round_e_form` helper as `to_exp_f`.
        let value = round_e_form(n, mantissa_digits);
        format!("{:.*e}", mantissa_digits, value).into_bytes()
    };
    // Per ES §22.1.3.36 — toPrecision MUST emit exactly `precision`
    // significant digits, including trailing zeros. Do NOT strip
    // trailing zeros (that would turn `(1e-7).toPrecision(2)` from
    // the spec-correct "1.0e-7" into "1e-7"; `(0).toPrecision(3)`
    // from "0.00" into "0"; `(100).toPrecision(2)` from "1.0e+2"
    // into "1e+2").
    normalize_exp(&formatted)
}

/// `n.toPrecision(digits)` core for integer receivers.
pub fn to_precision_i(n: i64, digits: i64) -> Vec<u8> {
    to_precision_f(n as f64, digits)
}

/// S139 — `n.toLocaleString()` en-US default (`,` group sep, `.`
/// decimal). Per ES §21.1.3.4 the locale defaults to the host's
/// DefaultLocale; bun / Node on macOS reports en-US, so the canonical
/// `1,000,000` / `1,234.5` / `-1,000` output is what test262 + user
/// programs expect. Locale-aware variants (`toLocaleString('de-DE')`
/// etc.) are a separate Intl substrate item.
pub fn to_locale_f(n: f64) -> Vec<u8> {
    if let Some(s) = special_value(n) {
        return s;
    }
    // ECMA-402 default `maximumFractionDigits = 3` + `minimumFractionDigits = 0`.
    // Magnitudes strictly less than half a milli round to a single zero
    // digit (per half-away-from-zero). Sign-preserve so tiny negatives
    // render as `"-0"` — bun's behavior for `(-1e-6).toLocaleString()`
    // is `"-0"`, not `"-0.000001"`.
    //
    // Wider half-away-from-zero rounding (e.g. `(1.1235).toLocaleString()`
    // → `"1.124"` instead of bun's exact match) is L3b: Rust's `{:.3}`
    // uses banker's rounding and `f64` precision drift around the half-
    // case breaks byte-equality — a spec-correct fix needs string-based
    // rounding from the literal source. Tiny-magnitude collapse is the
    // narrow path that user code most often observes.
    if n != 0.0 && n.abs() < 5e-4 {
        return if n.is_sign_negative() {
            b"-0".to_vec()
        } else {
            b"0".to_vec()
        };
    }
    let raw = if n.fract() == 0.0 && n.abs() < 1e16 {
        // Integer-valued double: drop the trailing `.0` so
        // `(1000.0).toLocaleString() === "1,000"`, matching bun.
        format!("{}", n as i64).into_bytes()
    } else {
        format!("{n}").into_bytes()
    };
    insert_grouping(&raw)
}

pub fn to_locale_i(n: i64) -> Vec<u8> {
    let raw = format!("{n}").into_bytes();
    insert_grouping(&raw)
}

/// Walk the integer part (chars before any `.` or `e`) right-to-left
/// and insert `,` every three digits. Leading sign byte is skipped.
fn insert_grouping(raw: &[u8]) -> Vec<u8> {
    let sign_len = if raw.first() == Some(&b'-') { 1 } else { 0 };
    let int_end = raw[sign_len..]
        .iter()
        .position(|&b| b == b'.' || b == b'e' || b == b'E')
        .map(|i| sign_len + i)
        .unwrap_or(raw.len());
    let int_part = &raw[sign_len..int_end];
    let tail = &raw[int_end..];
    let n = int_part.len();
    let mut out = Vec::with_capacity(raw.len() + n / 3 + 1);
    if sign_len == 1 {
        out.push(b'-');
    }
    for (i, &b) in int_part.iter().enumerate() {
        let remaining = n - i;
        if i > 0 && remaining % 3 == 0 {
            out.push(b',');
        }
        out.push(b);
    }
    out.extend_from_slice(tail);
    out
}

// ============================================================
// extern "C" wrappers
// ============================================================

/// `n.toFixed(digits)` for f64 receivers.
///
/// ES §22.1.3.32 step 3 — `digits` must be in `[0, 100]`; otherwise
/// `RangeError` is thrown. Pre-fix tr's helper clamped silently via
/// `digits.clamp(0, 20)` so `(1.5).toFixed(-1)` returned `"2"`
/// instead of throwing. Throws propagate non-locally via TLS — the
/// SSA arm calls `emit_throw_check(None)` after every toFixed Call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_num_to_fixed_f(n: f64, digits: i64) -> *mut u8 {
    if !(0..=100).contains(&digits) {
        unsafe {
            __torajs_throw_range_error(b"toFixed() argument must be between 0 and 100\0".as_ptr());
        }
        return alloc_str(b"");
    }
    alloc_str(&to_fixed_f(n, digits))
}

/// `n.toFixed(digits)` for i64 receivers. Same RangeError gate as
/// the f64 variant.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_num_to_fixed_i(n: i64, digits: i64) -> *mut u8 {
    if !(0..=100).contains(&digits) {
        unsafe {
            __torajs_throw_range_error(b"toFixed() argument must be between 0 and 100\0".as_ptr());
        }
        return alloc_str(b"");
    }
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

/// `n.toLocaleString()` en-US default for f64 receivers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_num_to_locale_f(n: f64) -> *mut u8 {
    alloc_str(&to_locale_f(n))
}

/// `n.toLocaleString()` en-US default for i64 receivers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_num_to_locale_i(n: i64) -> *mut u8 {
    alloc_str(&to_locale_i(n))
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
        assert_eq!(normalize_exp(b"NaN"), b"NaN".to_vec());
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
        // digits < 0 → 0 (defensive clamp inside the pure-Rust core;
        // extern wrappers route negatives to RangeError before
        // reaching here).
        assert_eq!(to_fixed_f(3.14, -5), b"3".to_vec());
        // Spec upper bound is 100 — full precision available.
        let r = to_fixed_f(1.0, 100);
        assert_eq!(r.len(), "1.".len() + 100);
        assert!(r.starts_with(b"1."));
        // digits > 100 → defensive clamp to 100 (extern wrappers
        // already route out-of-range to RangeError).
        let r2 = to_fixed_f(1.0, 200);
        assert_eq!(r2.len(), "1.".len() + 100);
    }

    #[test]
    fn to_fixed_integer_path() {
        assert_eq!(to_fixed_i(42, 2), b"42.00".to_vec());
        assert_eq!(to_fixed_i(42, 0), b"42".to_vec());
        assert_eq!(to_fixed_i(-7, 3), b"-7.000".to_vec());
    }

    #[test]
    fn to_fixed_special_values() {
        assert_eq!(to_fixed_f(f64::NAN, 2), b"NaN".to_vec());
        assert_eq!(to_fixed_f(f64::INFINITY, 2), b"Infinity".to_vec());
        assert_eq!(to_fixed_f(-f64::INFINITY, 2), b"-Infinity".to_vec());
    }

    // ---- to_exp ----

    #[test]
    fn to_exp_basic_positive_exp() {
        assert_eq!(to_exp_f(100.0, 0), b"1e+2".to_vec());
        assert_eq!(to_exp_f(100.0, 2), b"1.00e+2".to_vec());
        // 12345 = 1.2345e4; ES §22.1.3.5 ties go to the larger m
        // (half-away-from-zero), giving "1.235e+4" (not banker
        // "1.234"). Pre-round neutralizes Rust `{:.*e}`'s default
        // half-to-even.
        assert_eq!(to_exp_f(12345.0, 3), b"1.235e+4".to_vec());
        // 12355 = 1.2355e4; banker and away agree → "1.236e+4".
        assert_eq!(to_exp_f(12355.0, 3), b"1.236e+4".to_vec());
    }

    #[test]
    fn to_exp_half_away_from_zero() {
        // ES §22.1.3.5 tie rounding — Rust `{:.*e}` default would
        // pick the banker neighbour (even mantissa digit stays).
        // 1.25 → 1 frac: banker "1.2e+0" / spec "1.3e+0".
        assert_eq!(to_exp_f(1.25, 1), b"1.3e+0".to_vec());
        assert_eq!(to_exp_f(-1.25, 1), b"-1.3e+0".to_vec());
        // 2.5 → 0 frac: banker "2e+0" / spec "3e+0".
        assert_eq!(to_exp_f(2.5, 0), b"3e+0".to_vec());
        assert_eq!(to_exp_f(-2.5, 0), b"-3e+0".to_vec());
        // 125 mantissa 1.25 with x=2: banker "1.2e+2" / spec "1.3e+2".
        assert_eq!(to_exp_f(125.0, 1), b"1.3e+2".to_vec());
        // Non-tie regression guard: 1.6 → 1 frac is not a tie at
        // mantissa position 1, both rules round to "1.6e+0".
        assert_eq!(to_exp_f(1.6, 1), b"1.6e+0".to_vec());
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
        assert_eq!(to_exp_f(f64::NAN, 2), b"NaN".to_vec());
        assert_eq!(to_exp_f(f64::INFINITY, 2), b"Infinity".to_vec());
        assert_eq!(to_exp_f(-f64::INFINITY, 2), b"-Infinity".to_vec());
    }

    // ---- to_precision ----

    #[test]
    fn to_exponential_negative_digits_picks_shortest() {
        // ES §22.1.3.5 — `n.toExponential()` (no arg) returns
        // shortest representation. `digits < 0` sentinel routes
        // through Rust's ryu-shortest `{:e}` formatter.
        assert_eq!(to_exp_f(1234567890.0, -1), b"1.23456789e+9".to_vec());
        assert_eq!(to_exp_f(0.1, -1), b"1e-1".to_vec());
        assert_eq!(to_exp_f(1234.5678, -1), b"1.2345678e+3".to_vec());
        assert_eq!(to_exp_f(1.5, -1), b"1.5e+0".to_vec());
        // Special values pass through.
        assert_eq!(to_exp_f(f64::NAN, -1), b"NaN".to_vec());
        assert_eq!(to_exp_f(f64::INFINITY, -1), b"Infinity".to_vec());
        // Positive digits path unchanged.
        assert_eq!(to_exp_f(1234.5678, 2), b"1.23e+3".to_vec());
    }

    #[test]
    fn to_precision_uses_f_form_in_range() {
        // X = 2 (since 123.456 = 1.23456e2), precision = 6,
        // 2 in [-4, 6) → %f form, 3 digits after decimal.
        assert_eq!(to_precision_f(123.456, 6), b"123.456".to_vec());
        // X = 0, precision = 3 → %f, 2 digits. Per JS spec keep
        // trailing zero (`(1.5).toPrecision(3) == "1.50"`).
        assert_eq!(to_precision_f(1.5, 3), b"1.50".to_vec());
        // Integer at fixed form: precision exhausted by integer
        // digits, no decimal portion needed.
        assert_eq!(to_precision_f(100.0, 3), b"100".to_vec());
    }

    #[test]
    fn to_precision_uses_e_form_out_of_range() {
        // X = 6, precision = 3, 6 NOT in [-4, 3) → %e form.
        assert_eq!(to_precision_f(1234567.0, 3), b"1.23e+6".to_vec());
        // X = -5, precision = 6, -5 NOT in [-4, 6) → %e form.
        // 0.00001234 = 1.234e-5 → mantissa 6 digits = "1.23400".
        assert_eq!(to_precision_f(0.00001234, 6), b"1.23400e-5".to_vec());
    }

    #[test]
    fn to_precision_e_form_rounds_half_away_from_zero() {
        // ES §22.1.3.36 ties go to the larger m on the %e branch
        // too. Follow-up to the %f-form fix (a5255701) — Rust
        // `{:.*e}` rounds half-to-even (banker); pre-round in
        // mantissa basis neutralizes it. Without the fix `(125)
        // .toPrecision(2)` returned banker "1.2e+2" instead of the
        // spec "1.3e+2".
        assert_eq!(to_precision_f(125.0, 2), b"1.3e+2".to_vec());
        assert_eq!(to_precision_f(-125.0, 2), b"-1.3e+2".to_vec());
        // 1.245e+3 at 2 frac → banker "1.24e+3" / spec "1.25e+3".
        assert_eq!(to_precision_f(1245.0, 3), b"1.25e+3".to_vec());
        assert_eq!(to_precision_f(-1245.0, 3), b"-1.25e+3".to_vec());
        // Non-tie regression guard on the %e path: 1.255 at 1 frac
        // is not a tie, both rules give "1.3e+3".
        assert_eq!(to_precision_f(1255.0, 2), b"1.3e+3".to_vec());
    }

    #[test]
    fn to_precision_rounds_half_away_from_zero() {
        // ES §22.1.3.36 ties go to the larger m. Banker (Rust default
        // `{:.0}`) would round 1234.5 → "1234" (4 even); spec / bun
        // gives "1235". Same wedge `to_fixed_f` already neutralizes
        // via explicit pre-rounding.
        assert_eq!(to_precision_f(1234.5, 4), b"1235".to_vec());
        // 2.5 round-half-to-even → "2"; away-from-zero → "3".
        assert_eq!(to_precision_f(2.5, 1), b"3".to_vec());
        // Negative ties: -1234.5 with banker → "-1234"; away → "-1235".
        assert_eq!(to_precision_f(-1234.5, 4), b"-1235".to_vec());
        // Non-tie value unaffected: 1234.6 rounds to 1235 either way.
        assert_eq!(to_precision_f(1234.6, 4), b"1235".to_vec());
    }

    #[test]
    fn to_precision_default_when_zero() {
        // digits <= 0 → default precision 6 (matches JS spec default
        // and C %g).
        assert_eq!(to_precision_f(123.456, 0), b"123.456".to_vec());
        // precision 6 with X=0 → 5 frac digits → "1.50000".
        assert_eq!(to_precision_f(1.5, -3), b"1.50000".to_vec());
    }

    #[test]
    fn to_precision_zero_value() {
        // Per JS spec §22.1.3.36 — 0 with precision p emits "0" + "."
        // + (p-1) zeros for p > 1; just "0" for p=1.
        assert_eq!(to_precision_f(0.0, 6), b"0.00000".to_vec());
        assert_eq!(to_precision_f(0.0, 1), b"0".to_vec());
    }

    #[test]
    fn to_precision_integer_path() {
        assert_eq!(to_precision_i(100, 3), b"100".to_vec());
        assert_eq!(to_precision_i(1234567, 3), b"1.23e+6".to_vec());
    }

    #[test]
    fn to_precision_special_values() {
        assert_eq!(to_precision_f(f64::NAN, 6), b"NaN".to_vec());
        assert_eq!(to_precision_f(f64::INFINITY, 6), b"Infinity".to_vec());
        assert_eq!(to_precision_f(-f64::INFINITY, 6), b"-Infinity".to_vec());
    }

    #[test]
    fn to_precision_clamps() {
        // digits > 100 → clamped to 100.
        let r = to_precision_f(1.0, 200);
        // 1.0 at precision 100: %f form, 99 digits after decimal —
        // "1." + 99 zeros (JS spec emits the full precision).
        let mut expected = b"1.".to_vec();
        expected.extend(core::iter::repeat(b'0').take(99));
        assert_eq!(r, expected);
    }
}
