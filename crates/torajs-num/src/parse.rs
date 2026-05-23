//! Number parsing — `Number.parseInt(s, radix)` and
//! `Number.parseFloat(s)`.
//!
//! Both take a Str (raw `*const u8` heap pointer for FFI ABI) and
//! return `f64` (NaN on failure). Port of `runtime_str.c` lines
//! 4013-4068 (P3.2-c.2, 2026-05-23). The Rust impl removes the C
//! version's 64-byte input cap on `parse_float`: the C path used
//! `char buf[64]; strtod(buf, ...)` which silently truncated long
//! numbers; the Rust path scans for the longest valid numeric
//! prefix and routes through `f64::from_str` over a Rust slice (no
//! NUL-terminator dance, no fixed-size buffer).
//!
//! `parseInt` is intentionally not delegated to `i64::from_str_radix`
//! because the spec requires partial-prefix parsing (stops at first
//! invalid digit, returns the parsed prefix as f64). Rust's
//! `from_str_radix` rejects any trailing junk.

use crate::layout::{STR_DATA_OFF, STR_LEN_OFF};

// ============================================================
// Layout-aware FFI helpers (sub-module-local)
// ============================================================

#[inline]
unsafe fn str_len(p: *const u8) -> u64 {
    unsafe { (p.add(STR_LEN_OFF) as *const u64).read() }
}

#[inline]
unsafe fn str_bytes<'a>(p: *const u8, len: u64) -> &'a [u8] {
    unsafe { core::slice::from_raw_parts(p.add(STR_DATA_OFF), len as usize) }
}

// ============================================================
// Pure-Rust cores
// ============================================================

#[inline]
fn is_ascii_ws(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

/// Decode one ASCII digit byte to its base-36 value, or `None` if
/// the byte isn't `0..=9 / a..=z / A..=Z`.
#[inline]
fn digit_value(c: u8) -> Option<u32> {
    match c {
        b'0'..=b'9' => Some((c - b'0') as u32),
        b'a'..=b'z' => Some((c - b'a' + 10) as u32),
        b'A'..=b'Z' => Some((c - b'A' + 10) as u32),
        _ => None,
    }
}

/// `Number.parseInt(s, radix)` over a raw byte slice. Returns
/// `NaN` if no digits are parsed (matches the C subset bit-for-bit).
/// Bit-for-bit corner cases preserved:
/// - Trim leading ASCII whitespace
/// - `+` / `-` sign accepted
/// - Auto-detect `0x` / `0X` prefix when `radix == 0` or `radix == 16`
/// - Default radix `10` when `radix == 0`
/// - Reject `radix < 2 || radix > 36` → `NaN`
pub fn parse_int(s: &[u8], radix: i64) -> f64 {
    let mut i = 0usize;
    while i < s.len() && is_ascii_ws(s[i]) {
        i += 1;
    }
    let mut sign = 1i32;
    if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
        if s[i] == b'-' {
            sign = -1;
        }
        i += 1;
    }
    let mut rdx = if radix == 0 { 10 } else { radix as i32 };
    if (radix == 0 || radix == 16)
        && i + 1 < s.len()
        && s[i] == b'0'
        && (s[i + 1] == b'x' || s[i + 1] == b'X')
    {
        rdx = 16;
        i += 2;
    }
    if !(2..=36).contains(&rdx) {
        return f64::NAN;
    }
    let digits_start = i;
    let mut v = 0.0f64;
    while i < s.len() {
        let c = s[i];
        let Some(d) = digit_value(c) else {
            break;
        };
        if (d as i32) >= rdx {
            break;
        }
        v = v * (rdx as f64) + (d as f64);
        i += 1;
    }
    if i == digits_start {
        return f64::NAN;
    }
    if sign < 0 { -v } else { v }
}

/// `Number.parseFloat(s)` over a raw byte slice. Returns `NaN`
/// if no numeric prefix parses. Removes the C version's 64-byte
/// input cap: scans for the longest valid numeric prefix, then
/// parses via `f64::from_str` over the matching slice.
pub fn parse_float(s: &[u8]) -> f64 {
    // Skip leading ASCII whitespace.
    let mut i = 0usize;
    while i < s.len() && is_ascii_ws(s[i]) {
        i += 1;
    }
    let scan_start = i;
    // Optional sign.
    if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
        i += 1;
    }
    // Special case: "Infinity" / "-Infinity" / "+Infinity".
    if s[i..].starts_with(b"Infinity") {
        let prefix = &s[scan_start..i + 8];
        return parse_prefix(prefix);
    }
    // Decimal scan: digits, optional '.' digits, optional exponent.
    let digits_start = i;
    while i < s.len() && s[i].is_ascii_digit() {
        i += 1;
    }
    if i < s.len() && s[i] == b'.' {
        i += 1;
        while i < s.len() && s[i].is_ascii_digit() {
            i += 1;
        }
    }
    if i == digits_start {
        // No digits at all in the numeric body — fail.
        return f64::NAN;
    }
    // Optional exponent: e[+-]?digits
    if i < s.len() && (s[i] == b'e' || s[i] == b'E') {
        let exp_start = i;
        i += 1;
        if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
            i += 1;
        }
        let exp_digits_start = i;
        while i < s.len() && s[i].is_ascii_digit() {
            i += 1;
        }
        if i == exp_digits_start {
            // Malformed exponent — rewind past the `e[+-]?` and use
            // the prefix without it. JS spec lets `"3.5xyz"` parse
            // as `3.5`; same for `"3.5e+xyz"` which should parse the
            // `3.5` prefix.
            i = exp_start;
        }
    }
    parse_prefix(&s[scan_start..i])
}

#[inline]
fn parse_prefix(prefix: &[u8]) -> f64 {
    // Convert to &str for f64::from_str. ASCII-only contract on
    // numeric prefix means the byte slice is always valid UTF-8.
    match core::str::from_utf8(prefix) {
        Ok(s) => s.parse::<f64>().unwrap_or(f64::NAN),
        Err(_) => f64::NAN,
    }
}

// ============================================================
// extern "C" wrappers
// ============================================================

/// `Number.parseInt(str, radix)` — radix-aware integer parse.
/// Returns NaN on failure.
///
/// # Safety
///
/// `s` must be a valid Str heap block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_num_parse_int(s: *const u8, radix: i64) -> f64 {
    let len = unsafe { str_len(s) };
    let bytes = unsafe { str_bytes(s, len) };
    parse_int(bytes, radix)
}

/// `Number.parseFloat(str)` — finds the longest valid numeric
/// prefix and parses it via `f64::from_str`. No buffer-size limit.
///
/// # Safety
///
/// `s` must be a valid Str heap block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_num_parse_float(s: *const u8) -> f64 {
    let len = unsafe { str_len(s) };
    let bytes = unsafe { str_bytes(s, len) };
    parse_float(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_int_decimal() {
        assert_eq!(parse_int(b"42", 10), 42.0);
        assert_eq!(parse_int(b"  42  ", 10), 42.0);
        assert_eq!(parse_int(b"-42", 10), -42.0);
        assert_eq!(parse_int(b"+42", 10), 42.0);
        assert_eq!(parse_int(b"42xyz", 10), 42.0); // partial prefix
    }

    #[test]
    fn parse_int_hex_auto() {
        assert_eq!(parse_int(b"0xff", 0), 255.0);
        assert_eq!(parse_int(b"0X1A", 0), 26.0);
        assert_eq!(parse_int(b"0xff", 16), 255.0); // explicit radix 16 also strips 0x
        assert_eq!(parse_int(b"-0xff", 0), -255.0);
    }

    #[test]
    fn parse_int_other_radix() {
        assert_eq!(parse_int(b"1010", 2), 10.0);
        assert_eq!(parse_int(b"zz", 36), 35.0 * 36.0 + 35.0);
    }

    #[test]
    fn parse_int_invalid() {
        assert!(parse_int(b"", 10).is_nan());
        assert!(parse_int(b"abc", 10).is_nan());
        assert!(parse_int(b"42", 1).is_nan());
        assert!(parse_int(b"42", 37).is_nan());
    }

    #[test]
    fn parse_float_basic() {
        assert_eq!(parse_float(b"3.14"), 3.14);
        assert_eq!(parse_float(b"  3.14  "), 3.14);
        assert_eq!(parse_float(b"-3.14"), -3.14);
        assert_eq!(parse_float(b"3.14e2"), 314.0);
        assert_eq!(parse_float(b"3.14e+2"), 314.0);
        assert_eq!(parse_float(b"3.14e-2"), 0.0314);
        assert_eq!(parse_float(b"100"), 100.0);
    }

    #[test]
    fn parse_float_partial_prefix() {
        assert_eq!(parse_float(b"3.14xyz"), 3.14);
        assert_eq!(parse_float(b"42abc"), 42.0);
    }

    #[test]
    fn parse_float_infinity() {
        assert_eq!(parse_float(b"Infinity"), f64::INFINITY);
        assert_eq!(parse_float(b"-Infinity"), -f64::INFINITY);
        assert_eq!(parse_float(b"+Infinity"), f64::INFINITY);
    }

    #[test]
    fn parse_float_invalid() {
        assert!(parse_float(b"").is_nan());
        assert!(parse_float(b"xyz").is_nan());
        assert!(parse_float(b"   ").is_nan());
    }

    #[test]
    fn parse_float_long_input_no_truncation() {
        // C version capped at 64 bytes; Rust version handles arbitrary.
        // 1234567890.0987654321 × 1e5 ≈ 1.23456789009876544e+14
        let big = b"1234567890.0987654321e+5";
        let r = parse_float(big);
        let expected = b"1234567890.0987654321".iter().fold(0.0f64, |acc, &c| {
            if c == b'.' {
                acc
            } else {
                acc * 10.0 + (c - b'0') as f64
            }
        }) * 1e5
            / 1e10; // wait, simpler: just round-trip via Rust's parser
        let _ = expected;
        let direct = "1234567890.0987654321e+5".parse::<f64>().unwrap();
        assert!((r - direct).abs() / r < 1e-13);
        // Stress: trailing junk well past the C version's 64-byte cap.
        // Must still parse the numeric prefix without truncation issues.
        let stress = b"3.14159265358979 plus a long trailing tag that overruns the C 64-byte buffer cap and would have been silently truncated by the old strtod path";
        assert!((parse_float(stress) - 3.14159265358979).abs() < 1e-14);
    }
}
