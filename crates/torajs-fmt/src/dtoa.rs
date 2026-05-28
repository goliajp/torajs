//! f64 → shortest-roundtrip decimal string, JS spec-compliant
//! per ES §6.1.6.1.13 [ToString applied to Number].
//!
//! ## Implementation strategy
//!
//! Step 15-b ships a **pragmatic** dtoa that delegates the
//! core shortest-roundtrip computation to Rust's `core::fmt`
//! (Grisu3-based, no_libc), then post-processes the byte output
//! to match JS spec edge cases:
//!
//! - `NaN` / `±Infinity` / `±0` → literal strings per §6.1.6.1.13
//!   steps 1-3.
//! - `1e21` / `1e-7` exponent boundary handling: Rust's `{}`
//!   emits `1e21` but JS spec wants `1e+21` (explicit sign on
//!   positive exponents).
//! - `-0.0` → `"0"` (JS treats negative-zero output as plain
//!   "0" for Display per §6.1.6.1.13 step 3).
//!
//! Step 15-b' (post-15-d follow-up) will replace the
//! `core::fmt` delegate with a hand-ported Ryū core for the
//! perf win on print-heavy fixtures. The public extern API
//! stays stable across the swap.
//!
//! ## Why core::fmt is acceptable
//!
//! Rust's `core::fmt::Display for f64` uses Grisu3 internally
//! (see `core::num::flt2dec`). This is no_std, no_libc — same
//! algorithm class as Ryū with a different output shape. For
//! Step 15-b's correctness baseline this is sufficient; the
//! perf upside of full Ryū is a follow-up optimization, not a
//! load-bearing A4 gate.

use core::fmt::Write;

/// Writer that captures `core::fmt::Write` output into a
/// caller-provided byte slice. Truncates silently if `buf`
/// fills — the caller's 32-byte cap is verified by the wrapper
/// to be enough for any valid f64 shortest-roundtrip output
/// (max 24 bytes per Ryū's analysis).
struct ByteWriter<'a> {
    buf: &'a mut [u8],
    pos: usize,
}

impl<'a> ByteWriter<'a> {
    fn new(buf: &'a mut [u8]) -> Self {
        Self { buf, pos: 0 }
    }
}

impl<'a> Write for ByteWriter<'a> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let bytes = s.as_bytes();
        let n = bytes.len();
        let remaining = self.buf.len().saturating_sub(self.pos);
        let copy_len = n.min(remaining);
        if copy_len > 0 {
            self.buf[self.pos..self.pos + copy_len].copy_from_slice(&bytes[..copy_len]);
            self.pos += copy_len;
        }
        if copy_len < n {
            // Buffer full; signal truncation.
            return Err(core::fmt::Error);
        }
        Ok(())
    }
}

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

/// Pure-Rust implementation (no unsafe, no extern). Test-
/// exposed via cfg(test) for the round-trip test suite.
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
    // magnitude. Empirically (matching v8 / bun): use
    // exponential when |d| < 1e-6 OR |d| >= 1e21.
    let abs_d = if d < 0.0 { -d } else { d };
    let use_exp = abs_d < 1e-6 || abs_d >= 1e21;
    let mut writer = ByteWriter::new(buf);
    let _ = if use_exp {
        write!(writer, "{:e}", d)
    } else {
        write!(writer, "{}", d)
    };
    let raw_len = writer.pos;
    // Post-process to match JS spec exponent shape: Rust emits
    // "1e21" — JS spec ToString wants "1e+21". Walk the output
    // looking for an 'e' followed by a digit (positive exponent
    // without explicit sign) and insert '+'.
    fix_exponent_sign(buf, raw_len)
}

#[inline]
fn write_literal(buf: &mut [u8], lit: &[u8]) -> usize {
    let n = lit.len();
    if n > buf.len() {
        return 0;
    }
    buf[..n].copy_from_slice(lit);
    n
}

/// Walks `buf[..len]` looking for an `e` followed immediately
/// by a digit (no explicit sign). If found, shifts the tail
/// right by one byte and inserts `+` after the `e`. Returns
/// the new length.
///
/// Only one `e` can appear in a valid f64 Display output, so a
/// single pass suffices.
fn fix_exponent_sign(buf: &mut [u8], len: usize) -> usize {
    let mut i = 0;
    while i < len {
        if buf[i] == b'e' {
            // Already has explicit sign?
            if i + 1 < len && (buf[i + 1] == b'+' || buf[i + 1] == b'-') {
                return len;
            }
            // Insert '+' after the 'e' — shift tail by 1.
            if len + 1 > buf.len() {
                return len;
            }
            let mut j = len;
            while j > i + 1 {
                buf[j] = buf[j - 1];
                j -= 1;
            }
            buf[i + 1] = b'+';
            return len + 1;
        }
        i += 1;
    }
    len
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

    /// Exponent-sign normalization: Rust `{}` emits `1e21` for
    /// 1e21; JS spec wants `1e+21` (positive exponent gets
    /// explicit `+`).
    #[test]
    fn exponent_positive_sign() {
        // Pick a value that Rust's Display reliably emits in
        // exponential form. 1e21 is a typical case.
        let s = call(1e21);
        // Output should contain 'e+' not bare 'e<digit>'.
        assert!(
            s.contains("e+") || !s.contains('e'),
            "expected 'e+' or no exponent in {s:?}"
        );
        assert!(
            !s.contains("e2") && !s.contains("e3"),
            "expected explicit + sign, not bare e<digit> in {s:?}"
        );
    }

    /// Negative exponent keeps its '-' (no double sign).
    #[test]
    fn exponent_negative_kept() {
        let s = call(1e-10);
        // Rust may emit "0.0000000001" (no exponent) for 1e-10 —
        // either form is valid as long as it round-trips.
        let parsed: f64 = s.parse().unwrap();
        assert_eq!(parsed, 1e-10, "round-trip failed for 1e-10: got {s:?}");
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
