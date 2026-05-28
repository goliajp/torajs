//! string → f64 parser.
//!
//! ## Implementation strategy
//!
//! Step 15-c ships a **pragmatic** atod that delegates the
//! core parse to Rust's `<f64 as core::str::FromStr>::from_str`
//! (Eisel-Lemire-based, no_libc). The wrapper handles the
//! caller's `(*const u8, len, *mut endp)` ABI and `endp`
//! semantics that libc `strtod` provided.
//!
//! For JS-spec ToNumber semantics (per ES §7.1.4) the call
//! sites that consume this entry point (torajs-str's
//! json_parse + torajs-arr's join round-trip verifier — both
//! pre-trimmed by their respective callers) only need the
//! "parse leading numeric span" subset. Full ToNumber (handles
//! hex `0x` prefix, leading whitespace, octal `0o`, binary
//! `0b`) lives at a higher layer (torajs-str's `to_number.rs`
//! Rust impl, already ported in P3.1-h).
//!
//! Step 15-c' (post-15-e follow-up, optional): replace the
//! core::str delegate with a hand-ported Eisel-Lemire core
//! that doesn't require a UTF-8 string conversion step. Public
//! extern API stays stable across the swap.

use core::str::from_utf8;

/// `__torajs_fmt_atod(s, len, endp) -> f64`.
///
/// Parses the leading f64 numeric span in `s[..len]`. On
/// success, advances `*endp` (if non-null) past the consumed
/// bytes and returns the parsed value. On parse failure,
/// returns `f64::NAN` and sets `*endp = 0`.
///
/// Matches libc `strtod`'s "parse as much as makes a valid
/// float, leave endp pointing past it" semantics, **except**
/// that this entry point does NOT skip leading whitespace
/// (caller is responsible per the existing JSON-parser and
/// arr-join contracts). For JS-spec full ToNumber whitespace
/// stripping, the higher-layer `to_number.rs` already handles
/// it before calling here.
///
/// # Safety
///
/// `s` must point at ≥ `len` readable bytes. `endp` is null
/// or a writable `*mut usize` slot.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_fmt_atod(s: *const u8, len: usize, endp: *mut usize) -> f64 {
    if s.is_null() || len == 0 {
        if !endp.is_null() {
            unsafe { *endp = 0 };
        }
        return f64::NAN;
    }
    // SAFETY: caller covenant on s + len.
    let bytes = unsafe { core::slice::from_raw_parts(s, len) };
    parse_f64_span(bytes, endp)
}

/// Pure-Rust core (test-exposed). Identifies the longest
/// leading byte span that parses as a valid f64 + returns its
/// value and span length.
fn parse_f64_span(bytes: &[u8], endp: *mut usize) -> f64 {
    // Find the longest leading f64-shaped span by greedy
    // longest-match. strtod's grammar:
    //   ([+-])? ( inf | infinity | nan | <digits>[.<digits>]?[eE][+-]?<digits>? )
    // We accept the textbook subset that bun / v8 / node accept
    // for ToNumber-on-a-pre-trimmed-string (no NaN literal —
    // ToNumber's NaN handling is at the higher layer; here we
    // parse numeric tokens).
    let span_end = find_numeric_span_end(bytes);
    if span_end == 0 {
        if !endp.is_null() {
            unsafe { *endp = 0 };
        }
        return f64::NAN;
    }
    // SAFETY: ascii-restricted by find_numeric_span_end above.
    let s = unsafe { from_utf8(&bytes[..span_end]).unwrap_unchecked() };
    match s.parse::<f64>() {
        Ok(v) => {
            if !endp.is_null() {
                unsafe { *endp = span_end };
            }
            v
        }
        Err(_) => {
            if !endp.is_null() {
                unsafe { *endp = 0 };
            }
            f64::NAN
        }
    }
}

/// Returns the index of the byte just past the longest leading
/// numeric span. 0 if no valid f64 start byte.
fn find_numeric_span_end(bytes: &[u8]) -> usize {
    let mut i = 0;
    let n = bytes.len();
    // Optional sign.
    if i < n && (bytes[i] == b'+' || bytes[i] == b'-') {
        i += 1;
    }
    let after_sign = i;
    // Mantissa: digits + optional fractional.
    let mut saw_digit = false;
    while i < n && bytes[i].is_ascii_digit() {
        i += 1;
        saw_digit = true;
    }
    if i < n && bytes[i] == b'.' {
        i += 1;
        while i < n && bytes[i].is_ascii_digit() {
            i += 1;
            saw_digit = true;
        }
    }
    if !saw_digit {
        // No digits at all — restore i to 0 to signal failure.
        return 0;
    }
    let _mantissa_end = i;
    // Optional exponent.
    if i < n && (bytes[i] == b'e' || bytes[i] == b'E') {
        let exp_start = i;
        i += 1;
        if i < n && (bytes[i] == b'+' || bytes[i] == b'-') {
            i += 1;
        }
        let mut saw_exp_digit = false;
        while i < n && bytes[i].is_ascii_digit() {
            i += 1;
            saw_exp_digit = true;
        }
        if !saw_exp_digit {
            // Exponent malformed; back up to mantissa end.
            i = exp_start;
        }
    }
    let _ = after_sign; // explicit acknowledge — sign-only is not a valid f64
    if i == after_sign { 0 } else { i }
}

#[cfg(test)]
mod tests {
    use super::*;

    extern crate alloc;

    fn parse(s: &[u8]) -> (f64, usize) {
        let mut endp: usize = 0;
        let v = unsafe { __torajs_fmt_atod(s.as_ptr(), s.len(), &mut endp) };
        (v, endp)
    }

    #[test]
    fn integer_basic() {
        let (v, e) = parse(b"42");
        assert_eq!(v, 42.0);
        assert_eq!(e, 2);
    }

    #[test]
    fn negative() {
        let (v, e) = parse(b"-3.14");
        assert_eq!(v, -3.14);
        assert_eq!(e, 5);
    }

    #[test]
    fn fractional() {
        let (v, e) = parse(b"3.14");
        assert_eq!(v, 3.14);
        assert_eq!(e, 4);
    }

    #[test]
    fn exponent_positive() {
        let (v, e) = parse(b"1e21");
        assert_eq!(v, 1e21);
        assert_eq!(e, 4);
    }

    #[test]
    fn exponent_explicit_plus() {
        let (v, e) = parse(b"1e+10");
        assert_eq!(v, 1e10);
        assert_eq!(e, 5);
    }

    #[test]
    fn exponent_negative() {
        let (v, e) = parse(b"1e-10");
        assert_eq!(v, 1e-10);
        assert_eq!(e, 5);
    }

    #[test]
    fn trailing_garbage_truncated() {
        let (v, e) = parse(b"3.14abc");
        assert_eq!(v, 3.14);
        assert_eq!(e, 4);
    }

    #[test]
    fn pure_garbage_returns_nan() {
        let (v, e) = parse(b"abc");
        assert!(v.is_nan());
        assert_eq!(e, 0);
    }

    #[test]
    fn empty_returns_nan() {
        let (v, e) = parse(b"");
        assert!(v.is_nan());
        assert_eq!(e, 0);
    }

    #[test]
    fn null_returns_nan() {
        let mut endp: usize = 999;
        let v = unsafe { __torajs_fmt_atod(core::ptr::null(), 0, &mut endp) };
        assert!(v.is_nan());
        assert_eq!(endp, 0);
    }

    /// Round-trip: format + parse identity for a sample set.
    #[test]
    fn round_trip_samples() {
        use alloc::format;
        let samples: &[f64] = &[
            1.0,
            -1.0,
            3.14,
            -3.14,
            42.0,
            1e10,
            1e-10,
            1e21,
            1e-21,
            f64::EPSILON,
            f64::MAX,
            f64::MIN,
            0.0,
            -0.0,
        ];
        for &d in samples {
            let s = format!("{:e}", d);
            let bytes = s.as_bytes();
            let mut endp: usize = 0;
            let parsed = unsafe { __torajs_fmt_atod(bytes.as_ptr(), bytes.len(), &mut endp) };
            assert_eq!(
                parsed.to_bits(),
                d.to_bits(),
                "round-trip mismatch for {d}: formatted as {s:?}, parsed back to {parsed}"
            );
            assert_eq!(endp, bytes.len(), "endp wrong for {d} formatted {s:?}");
        }
    }
}
